use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const HOT: f64 = 95.0;
pub const COOL: f64 = 88.0;
const CHECK_EVERY: Duration = Duration::from_secs(2);
const HOT_CHECKS: u32 = 2;
const COOL_CHECKS: u32 = 15;

pub struct Thermal {
    zones: Vec<PathBuf>,
    bcl_alarms: Vec<PathBuf>,
}

impl Thermal {
    pub fn system() -> Self {
        Self::find(Path::new("/sys/class/thermal"), Path::new("/sys/class/hwmon"))
    }

    pub fn find(thermal_base: &Path, hwmon_base: &Path) -> Self {
        let zones = fs::read_dir(thermal_base)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("thermal_zone")))
            .filter(|p| {
                let kind = fs::read_to_string(p.join("type")).unwrap_or_default();
                kind.starts_with("cpu") || kind.starts_with("gpuss") || kind.starts_with("video")
            })
            .map(|p| p.join("temp"))
            .collect();

        let bcl_alarms = fs::read_dir(hwmon_base)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_bcl_hwmon(p))
            .flat_map(|p| {
                fs::read_dir(p)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|f| f.file_name().is_some_and(|n| n.to_string_lossy().ends_with("_alarm")))
            })
            .collect();

        Self { zones, bcl_alarms }
    }

    pub fn hottest(&self) -> Option<f64> {
        self.zones
            .iter()
            .filter_map(|p| fs::read_to_string(p).ok()?.trim().parse::<i64>().ok())
            .max()
            .map(|milli| milli as f64 / 1000.0)
    }

    pub fn bcl_alarm(&self) -> bool {
        self.bcl_alarms
            .iter()
            .any(|p| fs::read_to_string(p).ok().and_then(|s| s.trim().parse::<i64>().ok()).is_some_and(|v| v > 0))
    }
}

fn is_bcl_hwmon(dir: &Path) -> bool {
    [dir.join("uevent"), dir.join("device/uevent")].iter().any(|p| {
        fs::read_to_string(p).is_ok_and(|text| {
            text.lines().any(|line| line.starts_with("OF_COMPATIBLE_") && line.contains("bcl"))
        })
    })
}

#[derive(Debug, PartialEq)]
pub enum Change {
    Hot(f64),
    PowerLimit,
    Cooled(Option<f64>),
}

pub struct Backoff {
    thermal: Thermal,
    backed_off: bool,
    hot_streak: u32,
    cool_streak: u32,
    next_check: Instant,
}

impl Backoff {
    pub fn new(thermal: Thermal) -> Self {
        Self { thermal, backed_off: false, hot_streak: 0, cool_streak: 0, next_check: Instant::now() }
    }

    pub fn check(&mut self, now: Instant) -> Option<Change> {
        if now < self.next_check {
            return None;
        }
        self.next_check = now + CHECK_EVERY;
        let alarm = self.thermal.bcl_alarm();
        let celsius = self.thermal.hottest();
        if !self.backed_off {
            if alarm {
                self.backed_off = true;
                self.hot_streak = 0;
                self.cool_streak = 0;
                return Some(Change::PowerLimit);
            }
            if let Some(c) = celsius {
                if c >= HOT {
                    self.hot_streak += 1;
                    if self.hot_streak >= HOT_CHECKS {
                        self.backed_off = true;
                        self.hot_streak = 0;
                        self.cool_streak = 0;
                        return Some(Change::Hot(c));
                    }
                    return None;
                }
            }
            self.hot_streak = 0;
            return None;
        }

        if !alarm && celsius.is_none_or(|c| c <= COOL) {
            self.cool_streak += 1;
            if self.cool_streak >= COOL_CHECKS {
                self.backed_off = false;
                self.cool_streak = 0;
                return Some(Change::Cooled(celsius));
            }
        } else {
            self.cool_streak = 0;
        }
        None
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

    fn bcl(hwmon_base: &Path, n: u32, compatible: &str, alarm: i64) -> PathBuf {
        let dir = hwmon_base.join(format!("hwmon{n}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("uevent"), format!("OF_COMPATIBLE_0={compatible}\n")).unwrap();
        let path = dir.join("in0_min_alarm");
        fs::write(&path, format!("{alarm}\n")).unwrap();
        path
    }

    #[test]
    fn debounces_spikes_holds_cooldown_and_reacts_to_bcl_alarms() {
        let root = std::env::temp_dir().join(format!("fc-thermal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let tbase = root.join("thermal");
        let hbase = root.join("hwmon");
        zone(&tbase, 0, "cpu7-middle-thermal", 81_000);
        zone(&tbase, 1, "gpuss-0-thermal", 70_000);
        zone(&tbase, 2, "video-thermal", 72_000);
        zone(&tbase, 3, "battery", 99_000);
        zone(&tbase, 4, "pm8550-thermal", 120_000);
        fs::create_dir_all(tbase.join("cooling_device0")).unwrap();
        let alarm_path = bcl(&hbase, 1, "qcom,pm8550-bcl", 0);
        let _ignored = bcl(&hbase, 2, "other,sensor", 1);
        let cpu = tbase.join("thermal_zone0/temp");
        let vid = tbase.join("thermal_zone2/temp");

        let mut backoff = Backoff::new(Thermal::find(&tbase, &hbase));
        let mut at = Instant::now();
        let mut step = |cpu_m: i64, vid_m: i64, alarm: i64| {
            fs::write(&cpu, cpu_m.to_string()).unwrap();
            fs::write(&vid, vid_m.to_string()).unwrap();
            fs::write(&alarm_path, alarm.to_string()).unwrap();
            let change = backoff.check(at);
            at += CHECK_EVERY;
            change
        };

        assert_eq!(step(81_000, 72_000, 0), None);
        assert_eq!(step(95_400, 72_000, 0), None);
        assert_eq!(step(83_000, 72_000, 0), None);

        assert_eq!(step(95_200, 72_000, 0), None);
        assert_eq!(step(95_600, 72_000, 0), Some(Change::Hot(95.6)));

        assert_eq!(step(86_000, 72_000, 0), None);
        assert_eq!(step(92_000, 72_000, 0), None);
        for _ in 0..(COOL_CHECKS - 1) {
            assert_eq!(step(87_500, 72_000, 0), None);
        }
        assert_eq!(step(87_500, 72_000, 0), Some(Change::Cooled(Some(87.5))));

        assert_eq!(step(75_000, 72_000, 1), Some(Change::PowerLimit));
        assert_eq!(step(75_000, 72_000, 1), None);
        for _ in 0..(COOL_CHECKS - 1) {
            assert_eq!(step(75_000, 72_000, 0), None);
        }
        assert_eq!(step(75_000, 72_000, 0), Some(Change::Cooled(Some(75.0))));

        assert_eq!(step(75_000, 95_000, 0), None);
        assert_eq!(step(75_000, 95_500, 0), Some(Change::Hot(95.5)));

        fs::remove_dir_all(&root).unwrap();
    }
}
