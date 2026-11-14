//! All that's kept: how many times each thing happened on each day. No
//! request is stored, so there's nothing here about any one install.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

pub type Day = BTreeMap<String, u64>;

pub struct Store {
    path: PathBuf,
    pub days: BTreeMap<String, Day>,
    dirty: bool,
}

impl Store {
    pub fn load(path: PathBuf) -> Result<Self> {
        let days = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("reading {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
        };
        Ok(Self { path, days, dirty: false })
    }

    pub fn add(&mut self, day: &str, key: &str, n: u64) {
        *self.days.entry(day.to_owned()).or_default().entry(key.to_owned()).or_default() += n;
        self.dirty = true;
    }

    /// Written whole and renamed over, so a crash can't leave half a file.
    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&self.days)?).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).with_context(|| format!("saving {}", self.path.display()))?;
        self.dirty = false;
        Ok(())
    }
}

/// The UTC date `secs` after 1970 falls on, as 2026-11-14.
pub fn day(secs: u64) -> String {
    // civil-from-days, Howard Hinnant's
    let z = (secs / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_come_out_right_across_leap_years() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(951_782_400), "2000-02-29");
        assert_eq!(day(1_794_700_800 - 1), "2026-11-14");
        assert_eq!(day(1_794_700_800), "2026-11-15");
    }
}
