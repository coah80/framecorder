//! Commands from the dashboard tab, one per line on stdin, for a recorder
//! that stays running between recordings (clipping keeps it up).

use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Pause,
    Resume,
    /// Start a recording into this file.
    Record(PathBuf),
    StopRecord,
    /// Save the last `secs` seconds, into this file or a default one.
    Clip { secs: Option<u32>, path: Option<PathBuf> },
    Quit,
}

pub fn parse(line: &str) -> Result<Command, String> {
    let line = line.trim();
    let (word, rest) = line.split_once(' ').map_or((line, ""), |(w, r)| (w, r.trim()));
    let path = |s: &str| (!s.is_empty()).then(|| PathBuf::from(s));
    match word {
        "pause" => Ok(Command::Pause),
        "resume" => Ok(Command::Resume),
        "record" => path(rest).map(Command::Record).ok_or_else(|| "record needs a file".into()),
        "stop-record" => Ok(Command::StopRecord),
        "clip" => {
            let (secs, file) = rest.split_once(' ').map_or((rest, ""), |(s, f)| (s, f.trim()));
            let secs = match secs {
                "" => None,
                s => Some(s.parse().map_err(|_| format!("bad clip length {s:?}"))?),
            };
            Ok(Command::Clip { secs, path: path(file) })
        }
        "quit" => Ok(Command::Quit),
        _ => Err(format!("unknown command {line:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse("pause\n"), Ok(Command::Pause));
        assert_eq!(parse("record /tmp/a b.mp4"), Ok(Command::Record("/tmp/a b.mp4".into())));
        assert_eq!(parse("clip"), Ok(Command::Clip { secs: None, path: None }));
        assert_eq!(parse("clip 30"), Ok(Command::Clip { secs: Some(30), path: None }));
        assert_eq!(parse("clip 30 /tmp/c.mp4"), Ok(Command::Clip { secs: Some(30), path: Some("/tmp/c.mp4".into()) }));
        assert!(parse("record").is_err());
        assert!(parse("clip soon").is_err());
        assert!(parse("dance").is_err());
    }
}
