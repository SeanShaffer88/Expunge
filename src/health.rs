//! System health: free disk space (with a daily history for the trend),
//! memory use, and services that stopped unexpectedly.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DriveSpace {
    /// e.g. "C:\".
    pub root: String,
    pub total: u64,
    pub free: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
}

impl Memory {
    pub fn used_percent(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        ((self.total - self.available.min(self.total)) * 100 / self.total) as u32
    }
}

/// A service that stopped unexpectedly, with how often and when last.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Crash {
    pub service: String,
    pub count: u32,
    /// ISO time of the most recent stop.
    pub last: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Health {
    pub drives: Vec<DriveSpace>,
    pub memory: Option<Memory>,
    pub crashes: Vec<Crash>,
}

/// Free space per day per drive: date ("YYYY-MM-DD") -> root -> free bytes.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct History {
    #[serde(default)]
    pub days: BTreeMap<String, BTreeMap<String, u64>>,
}

const KEEP_DAYS: usize = 120;

impl History {
    pub fn load(data_root: &Path) -> Self {
        std::fs::read_to_string(data_root.join("health.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_root: &Path) {
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(data_root.join("health.json"), s);
        }
    }

    /// Record today's free space (one entry per day; the latest wins).
    pub fn record(&mut self, today: &str, drives: &[DriveSpace]) {
        let day = self.days.entry(today.to_string()).or_default();
        for d in drives {
            day.insert(d.root.clone(), d.free);
        }
        while self.days.len() > KEEP_DAYS {
            let first = self.days.keys().next().cloned().unwrap_or_default();
            self.days.remove(&first);
        }
    }

    /// Free space of one drive over time, oldest first.
    pub fn series(&self, root: &str) -> Vec<(String, u64)> {
        self.days
            .iter()
            .filter_map(|(d, m)| m.get(root).map(|v| (d.clone(), *v)))
            .collect()
    }

    /// Change in free space over (up to) the last `days` recorded days.
    pub fn change(&self, root: &str, days: usize) -> Option<i64> {
        let s = self.series(root);
        if s.len() < 2 {
            return None;
        }
        let from = s.len().saturating_sub(days.max(2));
        Some(s[s.len() - 1].1 as i64 - s[from].1 as i64)
    }
}

/// Fixed drives with their size and free space.
pub fn drives() -> Vec<DriveSpace> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW};
        const DRIVE_FIXED: u32 = 3;
        let mut v = Vec::new();
        for c in b'A'..=b'Z' {
            let root = format!("{}:\\", c as char);
            let w = crate::platform::windows::wide(&root);
            unsafe {
                if GetDriveTypeW(w.as_ptr()) != DRIVE_FIXED {
                    continue;
                }
                let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
                if GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) != 0
                    && total > 0
                {
                    v.push(DriveSpace {
                        root,
                        total,
                        free: avail,
                    });
                }
            }
        }
        v
    }
    #[cfg(not(windows))]
    {
        // Development builds: what `df` reports for the root file system.
        let Ok((0, out)) = crate::platform::run_hidden("df", &["-B1", "/"]) else {
            return Vec::new();
        };
        out.lines()
            .nth(1)
            .and_then(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                Some(DriveSpace {
                    root: "/".into(),
                    total: f.get(1)?.parse().ok()?,
                    free: f.get(3)?.parse().ok()?,
                })
            })
            .into_iter()
            .collect()
    }
}

pub fn memory() -> Option<Memory> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        unsafe {
            let mut m: MEMORYSTATUSEX = std::mem::zeroed();
            m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut m) == 0 {
                return None;
            }
            Some(Memory {
                total: m.ullTotalPhys,
                available: m.ullAvailPhys,
            })
        }
    }
    #[cfg(not(windows))]
    {
        let s = std::fs::read_to_string("/proc/meminfo").ok()?;
        let get = |k: &str| -> Option<u64> {
            s.lines()
                .find(|l| l.starts_with(k))?
                .split_whitespace()
                .nth(1)?
                .parse::<u64>()
                .ok()
                .map(|kb| kb * 1024)
        };
        Some(Memory {
            total: get("MemTotal:")?,
            available: get("MemAvailable:")?,
        })
    }
}

/// Services that stopped unexpectedly in the last week (Service Control
/// Manager events 7031 and 7034).
pub fn crashed_services() -> Vec<Crash> {
    let Ok((0, xml)) = crate::platform::run_hidden(
        "wevtutil",
        &[
            "qe",
            "System",
            "/q:*[System[Provider[@Name='Service Control Manager'] and (EventID=7031 or EventID=7034) and TimeCreated[timediff(@SystemTime) <= 604800000]]]",
            "/f:xml",
            "/c:200",
            "/rd:true",
        ],
    ) else {
        return Vec::new();
    };
    parse_crashes(&xml)
}

pub fn parse_crashes(xml: &str) -> Vec<Crash> {
    let mut by: BTreeMap<String, Crash> = BTreeMap::new();
    for ev in xml.split("<Event ").skip(1) {
        let name = ev
            .split("<Data Name='param1'>")
            .nth(1)
            .or_else(|| ev.split("<Data Name=\"param1\">").nth(1))
            .and_then(|r| r.split("</Data>").next())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let time = ev
            .split("SystemTime='")
            .nth(1)
            .or_else(|| ev.split("SystemTime=\"").nth(1))
            .and_then(|r| r.split(['\'', '"']).next())
            .unwrap_or("")
            .to_string();
        let c = by.entry(name.clone()).or_insert_with(|| Crash {
            service: name,
            ..Default::default()
        });
        c.count += 1;
        if time > c.last {
            c.last = time;
        }
    }
    let mut v: Vec<Crash> = by.into_values().collect();
    v.sort_by(|a, b| b.last.cmp(&a.last));
    v
}

/// Gather everything, and add today's free space to the history.
pub fn check(data_root: &Path) -> (Health, History) {
    let h = Health {
        drives: drives(),
        memory: memory(),
        crashes: crashed_services(),
    };
    let mut hist = History::load(data_root);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    hist.record(&today, &h.drives);
    hist.save(data_root);
    (h, hist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_and_trend() {
        let mut h = History::default();
        let d = |free| {
            vec![DriveSpace {
                root: "C:\\".into(),
                total: 100,
                free,
            }]
        };
        h.record("2026-10-01", &d(50));
        h.record("2026-10-02", &d(45));
        h.record("2026-10-02", &d(40)); // same day: latest wins
        h.record("2026-10-03", &d(30));
        assert_eq!(h.series("C:\\").len(), 3);
        assert_eq!(h.change("C:\\", 30), Some(-20));
        assert_eq!(h.change("D:\\", 30), None);
        for i in 0..200 {
            h.record(&format!("2027-{:03}", i), &d(1));
        }
        assert_eq!(h.days.len(), KEEP_DAYS);
    }

    #[test]
    fn crash_events() {
        let xml = "<Events><Event xmlns='x'><System><Provider Name='Service Control Manager'/><EventID>7031</EventID><TimeCreated SystemTime='2026-10-10T08:00:00.000Z'/></System><EventData><Data Name='param1'>Print Spooler</Data><Data Name='param2'>1</Data></EventData></Event><Event xmlns='x'><System><TimeCreated SystemTime='2026-10-12T09:00:00.000Z'/></System><EventData><Data Name='param1'>Print Spooler</Data></EventData></Event><Event xmlns='x'><System><TimeCreated SystemTime='2026-10-11T09:00:00.000Z'/></System><EventData><Data Name='param1'>Acme Updater</Data></EventData></Event></Events>";
        let c = parse_crashes(xml);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].service, "Print Spooler");
        assert_eq!(c[0].count, 2);
        assert_eq!(c[0].last, "2026-10-12T09:00:00.000Z");
        let m = Memory {
            total: 1000,
            available: 250,
        };
        assert_eq!(m.used_percent(), 75);
    }
}
