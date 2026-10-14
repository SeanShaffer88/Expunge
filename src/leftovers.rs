//! Leftover scanning for programs that were installed *without* monitoring:
//! after the vendor's uninstaller runs, find what it left behind.
//!
//! Matching is deliberately conservative: folders and keys must match the
//! program's name (or publisher\name) exactly after normalisation, or live
//! inside its recorded install location. Nothing is ever removed without the
//! user seeing it first, and everything goes through the backup.

use crate::model::fold;
use crate::platform::{expand_env, Registry};
use crate::programs::InstalledProgram;
use crate::revert::{Action, Plan, PlanItem};
use crate::util::{is_protected_dir, is_protected_key, protected_dirs};
use std::collections::BTreeSet;
use std::path::Path;

/// Where to look.
pub struct LeftoverScope {
    pub folders: Vec<String>,
    pub shortcut_dirs: Vec<String>,
    pub reg_bases: Vec<String>,
}

impl Default for LeftoverScope {
    fn default() -> Self {
        let f = |v: &[&str]| {
            v.iter()
                .map(|s| expand_env(s))
                .filter(|s| !s.contains('%'))
                .collect::<Vec<_>>()
        };
        LeftoverScope {
            folders: f(&[
                "%ProgramFiles%",
                "%ProgramFiles(x86)%",
                "%ProgramData%",
                "%APPDATA%",
                "%LOCALAPPDATA%",
                "%LOCALAPPDATA%\\Programs",
                "%USERPROFILE%\\AppData\\LocalLow",
                "%CommonProgramFiles%",
                "%CommonProgramFiles(x86)%",
                "%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs",
                "%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs",
            ]),
            shortcut_dirs: f(&[
                "%USERPROFILE%\\Desktop",
                "%PUBLIC%\\Desktop",
                "%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs",
                "%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs",
                "%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup",
            ]),
            reg_bases: vec![
                "HKLM\\SOFTWARE".into(),
                "HKLM\\SOFTWARE\\WOW6432Node".into(),
                "HKCU\\Software".into(),
            ],
        }
    }
}

/// Lower-case alphanumerics only.
pub fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Program name with version numbers, architecture and edition tags removed.
pub fn base_name(name: &str) -> String {
    let mut words: Vec<&str> = Vec::new();
    for w in name.split_whitespace() {
        let lw = w.to_lowercase();
        let lw = lw.trim_matches(|c: char| "()[]-,".contains(c));
        let is_version = lw.starts_with('v')
            && lw[1..].chars().next().is_some_and(|c| c.is_ascii_digit())
            || lw.chars().next().is_some_and(|c| c.is_ascii_digit())
                && lw.chars().all(|c| c.is_ascii_digit() || c == '.');
        let is_tag = matches!(
            lw,
            "x64"
                | "x86"
                | "64-bit"
                | "32-bit"
                | "64bit"
                | "32bit"
                | "amd64"
                | "arm64"
                | "(x64)"
                | "(x86)"
                | "-"
        );
        if is_version || is_tag || lw.is_empty() {
            continue;
        }
        words.push(w);
    }
    words.join(" ")
}

fn name_keys(p: &InstalledProgram) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for n in [norm(&p.name), norm(&base_name(&p.name))] {
        if n.len() >= 3 {
            s.insert(n);
        }
    }
    s
}

/// Find what a removed (or about-to-be-removed) program left behind.
pub fn find_leftovers(p: &InstalledProgram, reg: &dyn Registry, scope: &LeftoverScope) -> Plan {
    let mut plan = Plan::default();
    let protected = protected_dirs();
    let names = name_keys(p);
    let publisher = norm(&p.publisher);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let add = |plan: &mut Plan, action: Action, note: &str, seen: &mut BTreeSet<String>| {
        let id = fold(&action.describe());
        if seen.insert(id) {
            plan.items.push(PlanItem {
                action,
                selected: true,
                exists: true,
                note: note.into(),
            });
        }
    };

    // The Uninstall entry itself, if the vendor uninstaller left it.
    if p.source != crate::programs::Source::Store && reg.exists(&p.key) && !is_protected_key(&p.key)
    {
        add(
            &mut plan,
            Action::DeleteKey {
                path: p.key.clone(),
            },
            "Uninstall entry",
            &mut seen,
        );
    }

    // Recorded install location.
    let loc = p.install_location.trim();
    if !loc.is_empty() && Path::new(loc).is_dir() && !is_protected_dir(loc, &protected) {
        add(
            &mut plan,
            Action::DeleteTree {
                path: loc.to_string(),
            },
            "Install location",
            &mut seen,
        );
    }

    // Folders named after the program, or Publisher\Program.
    for base in &scope.folders {
        let Ok(rd) = std::fs::read_dir(base) else {
            continue;
        };
        for e in rd.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let child = e.file_name().to_string_lossy().to_string();
            let nc = norm(&child);
            let path = e.path().to_string_lossy().to_string();
            if names.contains(&nc) {
                if !is_protected_dir(&path, &protected) {
                    add(
                        &mut plan,
                        Action::DeleteTree { path },
                        "Name match",
                        &mut seen,
                    );
                }
            } else if !publisher.is_empty() && nc == publisher {
                if let Ok(inner) = std::fs::read_dir(e.path()) {
                    for ie in inner.flatten() {
                        if ie.file_type().is_ok_and(|t| t.is_dir())
                            && names.contains(&norm(&ie.file_name().to_string_lossy()))
                        {
                            let ip = ie.path().to_string_lossy().to_string();
                            if !is_protected_dir(&ip, &protected) {
                                add(
                                    &mut plan,
                                    Action::DeleteTree { path: ip },
                                    "Publisher\\name match",
                                    &mut seen,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Shortcuts named after the program, or pointing into its folder.
    for dir in &scope.shortcut_dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("lnk") || x.eq_ignore_ascii_case("url"))
            {
                let stem = norm(&path.file_stem().unwrap_or_default().to_string_lossy());
                let target_match = !loc.is_empty()
                    && std::fs::read(&path)
                        .map(|b| contains_utf16_or_ascii(&b, loc))
                        .unwrap_or(false);
                if names.contains(&stem) || target_match {
                    add(
                        &mut plan,
                        Action::DeleteFile {
                            path: path.to_string_lossy().to_string(),
                        },
                        "Shortcut",
                        &mut seen,
                    );
                }
            }
        }
    }

    // Registry: Software\Name and Software\Publisher\Name.
    for base in &scope.reg_bases {
        for sub in reg.subkeys(base) {
            let ns = norm(&sub);
            let path = format!("{base}\\{sub}");
            if names.contains(&ns) && !is_protected_key(&path) {
                add(
                    &mut plan,
                    Action::DeleteKey { path },
                    "Name match",
                    &mut seen,
                );
            } else if !publisher.is_empty() && ns == publisher {
                for inner in reg.subkeys(&path) {
                    if names.contains(&norm(&inner)) {
                        let ip = format!("{path}\\{inner}");
                        if !is_protected_key(&ip) {
                            add(
                                &mut plan,
                                Action::DeleteKey { path: ip },
                                "Publisher\\name match",
                                &mut seen,
                            );
                        }
                    }
                }
            }
        }
    }

    // Autostart values and App Paths that point into the install folder.
    if !loc.is_empty() && loc.len() > 3 {
        let floc = fold(loc);
        for base in [
            "HKLM\\SOFTWARE",
            "HKLM\\SOFTWARE\\WOW6432Node",
            "HKCU\\Software",
        ] {
            let run = format!("{base}\\Microsoft\\Windows\\CurrentVersion\\Run");
            if let Some(k) = reg.read_key(&run) {
                for v in k.values.values() {
                    if v.as_string().is_some_and(|s| fold(&s).contains(&floc)) {
                        add(
                            &mut plan,
                            Action::DeleteValue {
                                key: run.clone(),
                                name: v.name.clone(),
                            },
                            "Autostart entry",
                            &mut seen,
                        );
                    }
                }
            }
            let app_paths = format!("{base}\\Microsoft\\Windows\\CurrentVersion\\App Paths");
            for sub in reg.subkeys(&app_paths) {
                let kp = format!("{app_paths}\\{sub}");
                if let Some(k) = reg.read_key(&kp) {
                    if k.values
                        .values()
                        .any(|v| v.as_string().is_some_and(|s| fold(&s).contains(&floc)))
                    {
                        add(
                            &mut plan,
                            Action::DeleteKey { path: kp },
                            "App Paths entry",
                            &mut seen,
                        );
                    }
                }
            }
        }
        // Services whose executable lives in the install folder.
        let services = "HKLM\\SYSTEM\\CurrentControlSet\\Services";
        for sub in reg.subkeys(services) {
            if let Some(k) = reg.read_key(&format!("{services}\\{sub}")) {
                if k.get_string("ImagePath")
                    .is_some_and(|s| fold(&expand_env(&s)).contains(&floc))
                {
                    add(
                        &mut plan,
                        Action::StopDeleteService { name: sub.clone() },
                        "Service from install folder",
                        &mut seen,
                    );
                }
            }
        }
    }

    plan.items
        .sort_by(|a, b| a.action.category().cmp(b.action.category()));
    plan
}

fn contains_utf16_or_ascii(bytes: &[u8], needle: &str) -> bool {
    let lower = needle.to_lowercase();
    let ascii = String::from_utf8_lossy(bytes).to_lowercase();
    if ascii.contains(&lower) {
        return true;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
        .to_lowercase()
        .contains(&lower)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KeyData, RegValue};
    use crate::platform::MemRegistry;

    #[test]
    fn names() {
        assert_eq!(base_name("Foo Editor 7.1.2 (x64)"), "Foo Editor");
        assert_eq!(base_name("Bar v2.0 64-bit"), "Bar");
        assert_eq!(norm("Foo Editor"), "fooeditor");
    }

    #[test]
    fn finds_leftovers_conservatively() {
        let tmp = tempfile::tempdir().unwrap();
        let pf = tmp.path().join("pf");
        let appdata = tmp.path().join("appdata");
        let desk = tmp.path().join("desk");
        for d in [
            pf.join("Foo Editor"),
            pf.join("Foo Editor Pro"),
            appdata.join("FooCorp").join("Foo Editor"),
            appdata.join("FooCorp").join("Other"),
            desk.clone(),
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(desk.join("Foo Editor.lnk"), "x").unwrap();
        std::fs::write(desk.join("Unrelated.lnk"), "x").unwrap();

        let reg = MemRegistry::new();
        reg.put(KeyData::new("HKCU\\Software\\FooCorp\\Foo Editor").with(RegValue::dword("a", 1)));
        reg.put(KeyData::new("HKCU\\Software\\FooCorp\\Shared"));
        reg.put(KeyData::new("HKLM\\SOFTWARE\\Foo Editor"));
        let loc = pf.join("Foo Editor").to_string_lossy().to_string();
        reg.put(
            KeyData::new("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run")
                .with(RegValue::string("FooTray", &format!("\"{loc}\\tray.exe\"")))
                .with(RegValue::string("Other", "C:\\other.exe")),
        );

        let p = InstalledProgram {
            key: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Gone".into(),
            name: "Foo Editor 3.2".into(),
            publisher: "FooCorp".into(),
            install_location: loc.clone(),
            ..Default::default()
        };
        let scope = LeftoverScope {
            folders: vec![
                pf.to_string_lossy().into(),
                appdata.to_string_lossy().into(),
            ],
            shortcut_dirs: vec![desk.to_string_lossy().into()],
            reg_bases: vec!["HKLM\\SOFTWARE".into(), "HKCU\\Software".into()],
        };
        let plan = find_leftovers(&p, &reg, &scope);
        let d: Vec<String> = plan.items.iter().map(|i| i.action.describe()).collect();
        let has = |s: &str| d.iter().any(|x| x.contains(s));
        assert!(has(&format!("contents {loc}")), "{d:#?}");
        assert!(has("FooCorp") && has("appdata"), "{d:#?}");
        assert!(has("Foo Editor.lnk"));
        assert!(has("HKCU\\Software\\FooCorp\\Foo Editor"));
        assert!(has("HKLM\\SOFTWARE\\Foo Editor"));
        assert!(has("FooTray"));
        assert!(!has("Foo Editor Pro"), "{d:#?}");
        assert!(!has("Other"));
        assert!(!has("Shared"));
        assert!(!has("Unrelated"));
        // Install location found once even though it also matches by name.
        assert_eq!(
            d.iter()
                .filter(|x| x.contains("Foo Editor") && x.contains("pf") && !x.contains("Pro"))
                .count(),
            1,
            "{d:#?}"
        );
    }
}
