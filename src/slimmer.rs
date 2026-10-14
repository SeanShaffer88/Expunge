//! Deep, one-time Windows cleanup (the kind of space Auslogics Windows
//! Slimmer reclaims), always done through Windows' own tools:
//!
//! - **Previous Windows installation**: `Windows.old` and upgrade leftovers.
//! - **Outdated Windows components**: superseded versions in the component
//!   store (WinSxS), removed with `DISM /StartComponentCleanup`.
//! - **Files of turned-off Windows features**: payloads kept so a feature
//!   can be turned on offline; removed with `DISM /Disable-Feature /Remove`.
//! - **Old restore points**: every System Restore point except the newest.
//!
//! Each step is irreversible in its own way (for example, after removing
//! Windows.old you can't go back to the previous Windows version), so the
//! app explains and confirms each one separately, in Advanced mode only.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeepItem {
    PreviousWindows,
    ComponentStore,
    FeaturePayloads,
    RestorePoints,
}

impl DeepItem {
    pub const ALL: [DeepItem; 4] = [
        DeepItem::PreviousWindows,
        DeepItem::ComponentStore,
        DeepItem::FeaturePayloads,
        DeepItem::RestorePoints,
    ];

    pub fn title(self) -> &'static str {
        match self {
            DeepItem::PreviousWindows => "Previous Windows installation",
            DeepItem::ComponentStore => "Outdated Windows components",
            DeepItem::FeaturePayloads => "Files of turned-off Windows features",
            DeepItem::RestorePoints => "Old restore points",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            DeepItem::PreviousWindows => "Windows.old and other leftovers of a Windows upgrade. Afterwards you can no longer go back to the previous version of Windows.",
            DeepItem::ComponentStore => "Older versions of Windows files replaced by updates (WinSxS), removed with Windows' own DISM tool. Installed updates can no longer be uninstalled afterwards. Can take 10 minutes or more.",
            DeepItem::FeaturePayloads => "Windows keeps the files of features that are turned off. Removing them saves space; turning such a feature on later downloads it again.",
            DeepItem::RestorePoints => "Deletes every System Restore point except the newest one.",
        }
    }
}

/// What an analysis found.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    /// Space that can be freed, when Windows can tell.
    pub bytes: Option<u64>,
    /// Number of things (packages, features, restore points), when relevant.
    pub count: Option<u64>,
    /// Whether there is anything to clean.
    pub applicable: bool,
}

/// Parse sizes such as "2.03 GB", "205.84 MB", "512 KB" or "1,234 bytes".
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, unit) = s.split_once(char::is_whitespace).unwrap_or((s, "bytes"));
    let n: f64 = num.replace(',', "").parse().ok()?;
    let mult = match unit.trim().to_ascii_uppercase().as_str() {
        "BYTES" | "B" => 1.0,
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        "TB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((n * mult) as u64)
}

/// Parse `dism /English /Online /Cleanup-Image /AnalyzeComponentStore`:
/// reclaimable bytes (backups and disabled features plus cache) and whether
/// DISM recommends a cleanup.
pub fn parse_component_store(output: &str) -> Analysis {
    let mut bytes = 0u64;
    let mut recommended = false;
    let mut count = None;
    for line in output.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        if k.starts_with("backups and disabled features")
            || k.starts_with("cache and temporary data")
        {
            bytes += parse_size(v).unwrap_or(0);
        } else if k.starts_with("component store cleanup recommended") {
            recommended = v.trim().eq_ignore_ascii_case("yes");
        } else if k.starts_with("number of reclaimable packages") {
            count = v.trim().parse().ok();
        }
    }
    Analysis {
        bytes: (bytes > 0).then_some(bytes),
        count,
        applicable: recommended || count.is_some_and(|c| c > 0),
    }
}

/// Folders a Windows upgrade leaves behind on the system drive.
pub fn previous_windows_dirs() -> Vec<String> {
    let drive = crate::platform::expand_env("%SystemDrive%");
    if drive.contains('%') {
        return Vec::new();
    }
    ["Windows.old", "$Windows.~BT", "$Windows.~WS"]
        .iter()
        .map(|d| format!("{drive}\\{d}"))
        .filter(|p| std::path::Path::new(p).is_dir())
        .collect()
}

fn powershell(script: &str) -> anyhow::Result<String> {
    let (code, out) = crate::platform::run_hidden(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )?;
    if code != 0 {
        anyhow::bail!(
            "PowerShell exit code {code}: {}",
            out.lines().last().unwrap_or("")
        );
    }
    Ok(out)
}

/// Shadow copies (restore points) on the system drive, and their storage.
const SHADOW_INFO: &str =
    "$v=(Get-CimInstance Win32_Volume -Filter \"DriveLetter='$($env:SystemDrive)'\").DeviceID; \
$s=@(Get-CimInstance Win32_ShadowCopy | Where-Object { $_.VolumeName -eq $v }); \
$u=(Get-CimInstance Win32_ShadowStorage | Measure-Object -Property UsedSpace -Sum).Sum; \
Write-Output \"$($s.Count) $([int64]$u)\"";

/// Parse "count usedbytes" from [`SHADOW_INFO`].
pub fn parse_shadow_info(output: &str) -> Option<(u64, u64)> {
    let line = output.lines().rev().find(|l| !l.trim().is_empty())?;
    let mut it = line.split_whitespace();
    let count = it.next()?.parse().ok()?;
    let used = it.next().and_then(|u| u.parse().ok()).unwrap_or(0);
    Some((count, used))
}

pub fn analyze(item: DeepItem) -> anyhow::Result<Analysis> {
    match item {
        DeepItem::PreviousWindows => {
            let dirs = previous_windows_dirs();
            let bytes: u64 = dirs.iter().map(|d| crate::programs::dir_size(d)).sum();
            Ok(Analysis {
                bytes: (bytes > 0).then_some(bytes),
                count: None,
                applicable: !dirs.is_empty(),
            })
        }
        DeepItem::ComponentStore => {
            let (_, out) = crate::platform::run_hidden(
                "dism.exe",
                &[
                    "/English",
                    "/Online",
                    "/Cleanup-Image",
                    "/AnalyzeComponentStore",
                ],
            )?;
            Ok(parse_component_store(&out))
        }
        DeepItem::FeaturePayloads => {
            let n = disabled_with_payload()?.len() as u64;
            Ok(Analysis {
                bytes: None,
                count: Some(n),
                applicable: n > 0,
            })
        }
        DeepItem::RestorePoints => {
            let (count, used) = parse_shadow_info(&powershell(SHADOW_INFO)?).unwrap_or((0, 0));
            Ok(Analysis {
                // Roughly everything but the newest point is freed.
                bytes: (count > 1 && used > 0).then(|| used - used / count),
                count: Some(count),
                applicable: count > 1,
            })
        }
    }
}

/// Turned-off features whose files are still on disk.
fn disabled_with_payload() -> anyhow::Result<Vec<String>> {
    Ok(crate::features::list_features()?
        .into_iter()
        .filter(|f| f.state.eq_ignore_ascii_case("Disabled"))
        .map(|f| f.name)
        .collect())
}

/// Run the cleanup. `progress` receives a short phase description.
pub fn clean(item: DeepItem, progress: &dyn Fn(String)) -> anyhow::Result<()> {
    match item {
        DeepItem::PreviousWindows => {
            for dir in previous_windows_dirs() {
                progress(dir.clone());
                // Upgrade folders belong to TrustedInstaller: take ownership
                // for Administrators first, then delete.
                let _ = crate::platform::run_hidden(
                    "takeown.exe",
                    &["/F", &dir, "/R", "/A", "/D", "Y"],
                );
                let _ = crate::platform::run_hidden(
                    "icacls.exe",
                    &[&dir, "/grant", "*S-1-5-32-544:F", "/T", "/C", "/Q"],
                );
                if std::fs::remove_dir_all(&dir).is_err() {
                    let _ = crate::platform::run_hidden("cmd.exe", &["/C", "rd", "/S", "/Q", &dir]);
                }
            }
            let left = previous_windows_dirs();
            if !left.is_empty() {
                anyhow::bail!("Some files could not be removed: {}", left.join(", "));
            }
            Ok(())
        }
        DeepItem::ComponentStore => {
            progress("DISM /StartComponentCleanup".into());
            let (code, out) = crate::platform::run_hidden(
                "dism.exe",
                &[
                    "/English",
                    "/Online",
                    "/Cleanup-Image",
                    "/StartComponentCleanup",
                ],
            )?;
            match code {
                0 | 3010 => Ok(()),
                _ => anyhow::bail!(
                    "DISM exit code {code}: {}",
                    out.lines().last().unwrap_or("")
                ),
            }
        }
        DeepItem::FeaturePayloads => {
            let mut failed = Vec::new();
            for name in disabled_with_payload()? {
                progress(
                    crate::features::friendly_name(&name)
                        .unwrap_or(&name)
                        .to_string(),
                );
                let arg = format!("/FeatureName:{name}");
                let ok = crate::platform::run_hidden(
                    "dism.exe",
                    &[
                        "/English",
                        "/Online",
                        "/Disable-Feature",
                        &arg,
                        "/Remove",
                        "/NoRestart",
                    ],
                )
                .is_ok_and(|(c, _)| c == 0 || c == 3010);
                if !ok {
                    failed.push(name);
                }
            }
            if failed.is_empty() {
                Ok(())
            } else {
                anyhow::bail!("Could not remove: {}", failed.join(", "))
            }
        }
        DeepItem::RestorePoints => {
            let (count, _) = parse_shadow_info(&powershell(SHADOW_INFO)?).unwrap_or((0, 0));
            let drive = crate::platform::expand_env("%SystemDrive%");
            for i in 1..count {
                progress(format!("{i}/{}", count - 1));
                let for_arg = format!("/For={drive}");
                let (code, out) = crate::platform::run_hidden(
                    "vssadmin.exe",
                    &["Delete", "Shadows", &for_arg, "/Oldest", "/Quiet"],
                )?;
                if code != 0 {
                    anyhow::bail!(
                        "vssadmin exit code {code}: {}",
                        out.lines().last().unwrap_or("")
                    );
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("2 GB"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(
            parse_size(" 205.5 MB"),
            Some((205.5 * 1024.0 * 1024.0) as u64)
        );
        assert_eq!(parse_size("1,234 bytes"), Some(1234));
        assert_eq!(parse_size("12 parsecs"), None);
    }

    #[test]
    fn component_store() {
        let out = "\
Component Store (WinSxS) information:\n\
\n\
Windows Explorer Reported Size of Component Store : 8.02 GB\n\
\n\
Actual Size of Component Store : 7.86 GB\n\
\n\
    Shared with Windows : 5.62 GB\n\
    Backups and Disabled Features : 2.00 GB\n\
    Cache and Temporary Data :  200.00 MB\n\
\n\
Date of Last Cleanup : 2026-09-30 10:12:44\n\
\n\
Number of Reclaimable Packages : 3\n\
Component Store Cleanup Recommended : Yes\n\
\n\
The operation completed successfully.\n";
        let a = parse_component_store(out);
        assert!(a.applicable);
        assert_eq!(a.count, Some(3));
        assert_eq!(a.bytes, Some(2 * 1024 * 1024 * 1024 + 200 * 1024 * 1024));
        let clean = parse_component_store(
            "Number of Reclaimable Packages : 0\nComponent Store Cleanup Recommended : No\n",
        );
        assert!(!clean.applicable);
    }

    #[test]
    fn shadow_info() {
        assert_eq!(parse_shadow_info("\n3 5368709120\n"), Some((3, 5368709120)));
        assert_eq!(parse_shadow_info("0 0"), Some((0, 0)));
        assert_eq!(parse_shadow_info(""), None);
    }
}
