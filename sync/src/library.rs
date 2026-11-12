//! What's in ~/Videos/framecorder right now. Kept in memory and updated from
//! inotify, so listing never touches the disk.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::config::{hex, Paths};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Clip,
    Recording,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Clip {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub size: u64,
    pub duration_s: Option<f64>,
    pub created: i64,
    #[serde(skip)]
    pub path: PathBuf,
}

/// The kinds of file the recorder saves.
const EXTENSIONS: [&str; 2] = [".mp4", ".mkv"];

/// Only finished files count. Writers use `<name>.mp4.part` (or .mkv) until done.
pub fn is_finished_video(name: &str) -> bool {
    EXTENSIONS.iter().any(|ext| name.ends_with(ext)) && !name.starts_with('.') && name.len() > 4
}

/// Stable, URL-safe id for a file. Readable for the names the recorder
/// makes, hex for anything odd someone copied in.
pub fn clip_id(kind: Kind, file_name: &str) -> String {
    let stem = EXTENSIONS.iter().find_map(|ext| file_name.strip_suffix(ext)).unwrap_or(file_name);
    let prefix = match kind {
        Kind::Clip => 'c',
        Kind::Recording => 'r',
    };
    let safe = !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if safe {
        format!("{prefix}-{stem}")
    } else {
        format!("{prefix}~{}", hex(stem.as_bytes()))
    }
}

pub struct Library {
    paths: Paths,
    clips: Mutex<HashMap<String, Clip>>,
}

pub enum Change {
    New(Clip),
    Removed(String),
}

impl Library {
    pub fn new(paths: &Paths) -> Self {
        Self { paths: paths.clone(), clips: Mutex::new(HashMap::new()) }
    }

    fn kind_of(&self, path: &Path) -> Option<Kind> {
        let parent = path.parent()?;
        if parent == self.paths.clips {
            Some(Kind::Clip)
        } else if parent == self.paths.videos {
            Some(Kind::Recording)
        } else {
            None
        }
    }

    /// Reads both folders from scratch and reports what differs from before.
    pub fn rescan(&self) -> Vec<Change> {
        let mut found = HashMap::new();
        for (dir, kind) in [(&self.paths.videos, Kind::Recording), (&self.paths.clips, Kind::Clip)] {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if !is_finished_video(name) {
                    continue;
                }
                let known = self.clips.lock().unwrap().get(&clip_id(kind, name)).cloned();
                if let Some(clip) = probe(&entry.path(), kind, known.as_ref()) {
                    found.insert(clip.id.clone(), clip);
                }
            }
        }

        let mut clips = self.clips.lock().unwrap();
        let mut changes: Vec<Change> = clips
            .keys()
            .filter(|id| !found.contains_key(*id))
            .map(|id| Change::Removed(id.clone()))
            .collect();
        changes.extend(found.values().filter(|c| clips.get(&c.id) != Some(c)).map(|c| Change::New(c.clone())));
        *clips = found;
        changes
    }

    /// A file showed up or finished. Returns it if it's news.
    pub fn add(&self, path: &Path) -> Option<Clip> {
        let kind = self.kind_of(path)?;
        let name = path.file_name()?.to_str()?;
        if !is_finished_video(name) {
            return None;
        }
        let known = self.clips.lock().unwrap().get(&clip_id(kind, name)).cloned();
        let clip = probe(path, kind, known.as_ref())?;
        let mut clips = self.clips.lock().unwrap();
        if clips.get(&clip.id) == Some(&clip) {
            return None;
        }
        clips.insert(clip.id.clone(), clip.clone());
        Some(clip)
    }

    pub fn remove(&self, path: &Path) -> Option<String> {
        let kind = self.kind_of(path)?;
        let id = clip_id(kind, path.file_name()?.to_str()?);
        self.clips.lock().unwrap().remove(&id).map(|c| c.id)
    }

    /// Forgets everything in one folder, for when it went away.
    pub fn drop_dir(&self, dir: &Path) -> Vec<String> {
        let mut clips = self.clips.lock().unwrap();
        let gone: Vec<String> = clips.values().filter(|c| c.path.parent() == Some(dir)).map(|c| c.id.clone()).collect();
        for id in &gone {
            clips.remove(id);
        }
        gone
    }

    pub fn get(&self, id: &str) -> Option<Clip> {
        self.clips.lock().unwrap().get(id).cloned()
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Clip> {
        let mut list: Vec<Clip> = self.clips.lock().unwrap().values().cloned().collect();
        list.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.name.cmp(&a.name)));
        list
    }
}

fn probe(path: &Path, kind: Kind, known: Option<&Clip>) -> Option<Clip> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let name = path.file_name()?.to_str()?.to_string();
    let created = meta
        .created()
        .or_else(|_| meta.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // the file's done by the time we see it, so only re-read when it changed
    let duration_s = match known {
        Some(k) if k.size == meta.len() && k.created == created => k.duration_s,
        _ if name.ends_with(".mkv") => mkv_duration(path).ok().flatten(),
        _ => mp4_duration(path).ok().flatten(),
    };
    Some(Clip { id: clip_id(kind, &name), kind, name, size: meta.len(), duration_s, created, path: path.to_path_buf() })
}

/// Reads the movie header for the length. Only box headers get read on the
/// way there, so a multi-GB file with moov at the end costs a few seeks.
pub fn mp4_duration(path: &Path) -> io::Result<Option<f64>> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let Some((moov, moov_end)) = find_box(&mut f, 0, len, b"moov")? else { return Ok(None) };
    let Some((mvhd, _)) = find_box(&mut f, moov, moov_end, b"mvhd")? else { return Ok(None) };

    f.seek(SeekFrom::Start(mvhd))?;
    let mut head = [0u8; 4];
    f.read_exact(&mut head)?;
    let (timescale, duration) = if head[0] == 1 {
        let mut b = [0u8; 28];
        f.read_exact(&mut b)?;
        (u32::from_be_bytes(b[16..20].try_into().unwrap()), u64::from_be_bytes(b[20..28].try_into().unwrap()))
    } else {
        let mut b = [0u8; 16];
        f.read_exact(&mut b)?;
        (u32::from_be_bytes(b[8..12].try_into().unwrap()), u32::from_be_bytes(b[12..16].try_into().unwrap()) as u64)
    };
    let duration = if duration == 0 || duration == u64::MAX || duration == u32::MAX as u64 {
        // fragmented: the total lives in mvex/mehd, if the muxer wrote one
        fragment_duration(&mut f, moov, moov_end)?
    } else {
        Some(duration)
    };
    Ok(duration.filter(|_| timescale > 0).map(|d| d as f64 / timescale as f64))
}

fn fragment_duration(f: &mut File, moov: u64, moov_end: u64) -> io::Result<Option<u64>> {
    let Some((mvex, mvex_end)) = find_box(f, moov, moov_end, b"mvex")? else { return Ok(None) };
    let Some((mehd, _)) = find_box(f, mvex, mvex_end, b"mehd")? else { return Ok(None) };
    f.seek(SeekFrom::Start(mehd))?;
    let mut head = [0u8; 4];
    f.read_exact(&mut head)?;
    let d = if head[0] == 1 {
        let mut b = [0u8; 8];
        f.read_exact(&mut b)?;
        u64::from_be_bytes(b)
    } else {
        let mut b = [0u8; 4];
        f.read_exact(&mut b)?;
        u32::from_be_bytes(b) as u64
    };
    Ok((d > 0).then_some(d))
}

/// Reads the segment info for the length: Duration, in TimestampScale
/// nanoseconds. It sits ahead of the video, so this reads a few hundred
/// bytes. An MKV the recorder never finished has none.
pub fn mkv_duration(path: &Path) -> io::Result<Option<f64>> {
    const EBML: u32 = 0x1A45_DFA3;
    const SEGMENT: u32 = 0x1853_8067;
    const INFO: u32 = 0x1549_A966;
    const CLUSTER: u32 = 0x1F43_B675;
    const SCALE: u32 = 0x2A_D7B1;
    const DURATION: u32 = 0x4489;

    let mut f = io::BufReader::new(File::open(path)?);
    let (id, size) = (ebml_id(&mut f)?, ebml_size(&mut f)?);
    if id != EBML {
        return Ok(None);
    }
    f.seek_relative(size.unwrap_or(0) as i64)?;
    if ebml_id(&mut f)? != SEGMENT {
        return Ok(None);
    }
    ebml_size(&mut f)?;
    // the seek head, maybe some padding, then the info
    for _ in 0..16 {
        let id = ebml_id(&mut f)?;
        let Some(size) = ebml_size(&mut f)? else { return Ok(None) };
        if id == CLUSTER {
            return Ok(None);
        }
        if id != INFO {
            f.seek_relative(size as i64)?;
            continue;
        }
        let mut info = vec![0u8; size.min(4096) as usize];
        f.read_exact(&mut info)?;
        let mut r = io::Cursor::new(info.as_slice());
        let (mut scale, mut duration) = (1_000_000.0, None);
        while (r.position() as usize) < info.len() {
            let id = ebml_id(&mut r)?;
            let Some(size) = ebml_size(&mut r)? else { break };
            let start = r.position() as usize;
            let Some(v) = info.get(start..start + size as usize) else { break };
            r.set_position((start + v.len()) as u64);
            match (id, v.len()) {
                (SCALE, 1..=8) => scale = v.iter().fold(0u64, |n, b| n << 8 | *b as u64) as f64,
                (DURATION, 4) => duration = Some(f32::from_be_bytes(v.try_into().unwrap()) as f64),
                (DURATION, 8) => duration = Some(f64::from_be_bytes(v.try_into().unwrap())),
                _ => {}
            }
        }
        return Ok(duration.filter(|d| *d > 0.0).map(|d| d * scale / 1e9));
    }
    Ok(None)
}

/// An element ID, its length marker kept, the way the spec writes them.
fn ebml_id(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    let len = b[0].leading_zeros() as usize + 1;
    if len > 4 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut id = b[0] as u32;
    for _ in 1..len {
        r.read_exact(&mut b)?;
        id = id << 8 | b[0] as u32;
    }
    Ok(id)
}

/// An element's size, None when it's "unknown" (all ones).
fn ebml_size(r: &mut impl Read) -> io::Result<Option<u64>> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    let len = b[0].leading_zeros() as usize + 1;
    if len > 8 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut size = (b[0] as u64) & (0xFF >> len);
    let mut unknown = size == 0xFF >> len;
    for _ in 1..len {
        r.read_exact(&mut b)?;
        size = size << 8 | b[0] as u64;
        unknown &= b[0] == 0xFF;
    }
    Ok((!unknown).then_some(size))
}

/// Finds a box between `start` and `end`, returning where its payload
/// starts and where the box ends.
fn find_box(f: &mut File, start: u64, end: u64, want: &[u8; 4]) -> io::Result<Option<(u64, u64)>> {
    let mut pos = start;
    while pos + 8 <= end {
        f.seek(SeekFrom::Start(pos))?;
        let mut h = [0u8; 8];
        f.read_exact(&mut h)?;
        let mut size = u32::from_be_bytes(h[..4].try_into().unwrap()) as u64;
        let mut header = 8;
        if size == 1 {
            let mut big = [0u8; 8];
            f.read_exact(&mut big)?;
            size = u64::from_be_bytes(big);
            header = 16;
        } else if size == 0 {
            size = end - pos;
        }
        if size < header {
            return Ok(None);
        }
        let box_end = pos.saturating_add(size).min(end);
        if &h[4..8] == want {
            return Ok(Some((pos + header, box_end)));
        }
        pos = box_end;
    }
    Ok(None)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }

    fn mvhd(timescale: u32, duration: u32) -> Vec<u8> {
        let mut p = vec![0u8; 4 + 8];
        p.extend_from_slice(&timescale.to_be_bytes());
        p.extend_from_slice(&duration.to_be_bytes());
        p.extend_from_slice(&[0u8; 80]);
        bx(b"mvhd", &p)
    }

    /// A file shaped like ours: ftyp, a big mdat, moov at the end.
    pub fn fake_mp4(path: &Path, secs: u32) {
        let mut data = bx(b"ftyp", b"isomiso2");
        data.extend(bx(b"mdat", &vec![0u8; 4096]));
        data.extend(bx(b"moov", &mvhd(1000, secs * 1000)));
        std::fs::write(path, data).unwrap();
    }

    fn setup() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(&dir.path().join("videos"), &dir.path().join("state"));
        std::fs::create_dir_all(&paths.clips).unwrap();
        (dir, Library::new(&paths))
    }

    #[test]
    fn scanner_skips_unfinished_and_side_files() {
        let (dir, lib) = setup();
        let v = dir.path().join("videos");
        fake_mp4(&v.join("2026-09-26_14-00-00.mp4"), 3);
        fake_mp4(&v.join("clips/2026-09-26_14-05-00.mp4"), 30);
        fake_mp4(&v.join("2026-09-26_15-00-00.mp4.part"), 1);
        std::fs::write(v.join("2026-09-26_14-00-00.perf.csv"), "x").unwrap();
        std::fs::write(v.join("2026-09-26_14-00-00.log"), "x").unwrap();
        std::fs::create_dir(v.join("weird.mp4")).unwrap();

        let changes = lib.rescan();
        assert_eq!(changes.len(), 2);
        let list = lib.list();
        let ids: Vec<_> = list.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"r-2026-09-26_14-00-00"));
        assert!(ids.contains(&"c-2026-09-26_14-05-00"));
        let clip = lib.get("c-2026-09-26_14-05-00").unwrap();
        assert_eq!(clip.kind, Kind::Clip);
        assert_eq!(clip.duration_s, Some(30.0));

        // nothing changed, nothing to report
        assert!(lib.rescan().is_empty());
        std::fs::remove_file(v.join("2026-09-26_14-00-00.mp4")).unwrap();
        let changes = lib.rescan();
        assert!(matches!(&changes[..], [Change::Removed(id)] if id == "r-2026-09-26_14-00-00"));
    }

    #[test]
    fn add_and_remove_by_path() {
        let (dir, lib) = setup();
        let clips = dir.path().join("videos/clips");
        let part = clips.join("a.mp4.part");
        fake_mp4(&part, 2);
        assert!(lib.add(&part).is_none());
        let done = clips.join("a.mp4");
        std::fs::rename(&part, &done).unwrap();
        assert_eq!(lib.add(&done).unwrap().id, "c-a");
        assert!(lib.add(&done).is_none(), "same file twice isn't news");
        assert!(lib.add(&dir.path().join("elsewhere.mp4")).is_none());
        assert_eq!(lib.remove(&done).as_deref(), Some("c-a"));
        assert!(lib.remove(&done).is_none());
    }

    #[test]
    fn ids_are_url_safe() {
        assert_eq!(clip_id(Kind::Clip, "2026-09-26_14-00-00.mp4"), "c-2026-09-26_14-00-00");
        assert_eq!(clip_id(Kind::Recording, "x.mp4"), "r-x");
        assert_eq!(clip_id(Kind::Clip, "my clip/é.mp4"), format!("c~{}", hex("my clip/é".as_bytes())));
        assert_eq!(clip_id(Kind::Clip, "2026-09-26_14-00-00.mkv"), "c-2026-09-26_14-00-00");
        assert!(is_finished_video("a.mp4"));
        assert!(is_finished_video("a.mkv"));
        assert!(!is_finished_video("a.mp4.part"));
        assert!(!is_finished_video("a.mkv.part"));
        assert!(!is_finished_video(".mp4"));
        assert!(!is_finished_video("a.perf.csv"));
    }

    #[test]
    fn duration_from_64bit_and_fragmented_headers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frag.mp4");
        let mut mehd = vec![0u8; 4];
        mehd.extend_from_slice(&90_000u32.to_be_bytes());
        let moov = [mvhd(90_000, 0), bx(b"mvex", &bx(b"mehd", &mehd))].concat();
        std::fs::write(&path, [bx(b"ftyp", b"iso6"), bx(b"moov", &moov)].concat()).unwrap();
        assert_eq!(mp4_duration(&path).unwrap(), Some(1.0));

        // no moov at all, e.g. a broken file
        std::fs::write(&path, bx(b"mdat", &[0u8; 16])).unwrap();
        assert_eq!(mp4_duration(&path).unwrap(), None);
    }

    /// An element with an 8 byte size, the way FFmpeg writes the big ones.
    fn el(id: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut v = id.to_vec();
        v.push(0x01);
        v.extend_from_slice(&(payload.len() as u64).to_be_bytes()[1..]);
        v.extend_from_slice(payload);
        v
    }

    /// Laid out like the recorder's: a seek head and padding ahead of the
    /// info, which holds a CRC and strings longer than a number either side
    /// of the duration.
    fn fake_mkv(duration: Option<f64>) -> Vec<u8> {
        let mut info = [&[0xBF, 0x84][..], &[0; 4], &[0x2A, 0xD7, 0xB1, 0x83, 0x0F, 0x42, 0x40], &[0x4D, 0x80, 0x8D], b"Lavf62.3.100."].concat();
        if let Some(d) = duration {
            info.extend([0x44, 0x89, 0x88]);
            info.extend(d.to_be_bytes());
        }
        info.extend([&[0x57, 0x41, 0x8D][..], b"Lavf62.3.100."].concat());
        let segment = [el(&[0x11, 0x4D, 0x9B, 0x74], &[0; 12]), el(&[0xEC], &[0; 83]), el(&[0x15, 0x49, 0xA9, 0x66], &info), el(&[0x1F, 0x43, 0xB6, 0x75], &[0; 64])].concat();
        [el(&[0x1A, 0x45, 0xDF, 0xA3], &[0x42, 0x82, 0x88, b'm', b'a', b't', b'r', b'o', b's', b'k', b'a']), el(&[0x18, 0x53, 0x80, 0x67], &segment)].concat()
    }

    #[test]
    fn duration_from_mkv_and_none_from_an_unfinished_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mkv");
        std::fs::write(&path, fake_mkv(Some(4035.0))).unwrap();
        assert_eq!(mkv_duration(&path).unwrap(), Some(4.035));
        std::fs::write(&path, fake_mkv(None)).unwrap();
        assert_eq!(mkv_duration(&path).unwrap(), None);
        // an mp4 someone named .mkv
        std::fs::write(&path, fake_mp4_bytes()).unwrap();
        assert_eq!(mkv_duration(&path).ok().flatten(), None);
    }

    fn fake_mp4_bytes() -> Vec<u8> {
        [bx(b"ftyp", b"isomiso2"), bx(b"moov", &mvhd(1000, 1000))].concat()
    }
}
