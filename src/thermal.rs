use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const HOT: f64 = 95.0;
pub const COOL: f64 = 88.0;
const CHECK_EVERY: Duration = Duration::from_secs(2);

pub struct Thermal {
    zones: Vec<PathBuf>,
}

impl Thermal {
    pub fn find(base: &Path) -> Self {
        let zones = fs::read_dir(base)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("thermal_zone")))
            .filter(|p| {
                let kind = fs::read_to_string(p.join("type")).unwrap_or_default();
                kind.starts_with("cpu") || kind.starts_with("gpuss")
            })
            .map(|p| p.join("temp"))
            .collect();
        Self { zones }
    }

    pub fn hottest(&self) -> Option<f64> {
        self.zones
            .iter()
            .filter_map(|p| fs::read_to_string(p).ok()?.trim().parse::<i64>().ok())
            .max()
            .map(|milli| milli as f64 / 1000.0)
    }
}

pub enum Change {
    Hot(f64),
    Cooled(f64),
}

pub struct Backoff {
    thermal: Thermal,
    hot: bool,
    next_check: Instant,
}

impl Backoff {
    pub fn new(thermal: Thermal) -> Self {
        Self { thermal, hot: false, next_check: Instant::now() }
    }

    pub fn check(&mut self, now: Instant) -> Option<Change> {
        if now < self.next_check {
            return None;
        }
        self.next_check = now + CHECK_EVERY;
        let celsius = self.thermal.hottest()?;
        match (self.hot, celsius) {
            (false, c) if c >= HOT => {
                self.hot = true;
                Some(Change::Hot(c))
            }
            (true, c) if c <= COOL => {
                self.hot = false;
                Some(Change::Cooled(c))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(base: &Path, n: u32, kind: &str, milli: i64) {
        let dir = base.join(format!("thermal_zone{n}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("type"), format!("{kind}\n")).unwrap();
        fs::write(dir.join("temp"), format!("{milli}\n")).unwrap();
    }

    #[test]
    fn backs_off_at_the_socs_limit_and_comes_back_once_it_has_cooled() {
        let base = std::env::temp_dir().join(format!("fc-thermal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        zone(&base, 0, "cpu7-middle-thermal", 81_000);
        zone(&base, 1, "gpuss-0-thermal", 70_000);
        zone(&base, 2, "battery", 99_000);
        zone(&base, 3, "pm8550-bcl-lvl0", 120_000);
        fs::create_dir_all(base.join("cooling_device0")).unwrap();
        let cpu = base.join("thermal_zone0/temp");

        let mut backoff = Backoff::new(Thermal::find(&base));
        let mut at = Instant::now();
        let mut step = |milli: i64| {
            fs::write(&cpu, milli.to_string()).unwrap();
            let change = backoff.check(at);
            at += CHECK_EVERY;
            change.map(|c| match c {
                Change::Hot(c) => format!("hot {c:.1}"),
                Change::Cooled(c) => format!("cooled {c:.1}"),
            })
        };

        assert_eq!(step(81_000), None);
        assert_eq!(step(95_440).as_deref(), Some("hot 95.4"));
        assert_eq!(step(91_000), None);
        assert_eq!(step(96_000), None);
        assert_eq!(step(87_500).as_deref(), Some("cooled 87.5"));
        assert_eq!(step(94_900), None);
        assert_eq!(step(95_000).as_deref(), Some("hot 95.0"));
        fs::remove_dir_all(&base).unwrap();
    }
}
