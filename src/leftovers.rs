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
    /// Scheduled task definitions (`%SystemRoot%\System32\Tasks`).
    pub tasks_dir: Option<String>,
    /// Also look for COM registrations, file types, shell extensions,
    /// environment variables, firewall rules, scheduled tasks and empty
    /// folders tied to the install folder.
    pub deep: bool,
    /// Registry and task data for the deep scan, read once and shared by
    /// every program scanned with this scope.
    pub index: std::sync::OnceLock<DeepIndex>,
}

/// A registration that runs or loads a file: where it lives, and the file.
#[derive(Debug, Clone)]
pub struct Registration {
    pub key: String,
    /// The command or file path it points to.
    pub target: String,
    /// For COM classes, the class id.
    pub clsid: Option<String>,
    pub note: &'static str,
}

/// Everything the deep scan compares against, collected in one pass.
#[derive(Debug, Default)]
pub struct DeepIndex {
    /// COM classes, type libraries, file types, "Open with" and context-menu verbs.
    pub registrations: Vec<Registration>,
    /// Shell extension handler keys and the class id each one loads.
    pub handlers: Vec<(String, String)>,
    /// File extensions: (key, default file type, OpenWithProgids names).
    pub extensions: Vec<(String, Option<String>, Vec<String>)>,
    pub tasks: Vec<crate::startup::TaskEntry>,
    /// Every registered COM class id (folded), with or without a server.
    pub clsids: BTreeSet<String>,
}

impl DeepIndex {
    pub fn build(reg: &dyn Registry, tasks_dir: Option<&str>) -> Self {
        let mut ix = DeepIndex::default();
        let default_of = |path: &str| reg.read_key(path).and_then(|k| k.get_string(""));
        for base in CLASSES {
            let root = format!("{base}\\CLSID");
            for id in reg.subkeys(&root) {
                ix.clsids.insert(fold(&id));
                let key = format!("{root}\\{id}");
                for srv in ["InprocServer32", "LocalServer32"] {
                    if let Some(target) = default_of(&format!("{key}\\{srv}")) {
                        ix.registrations.push(Registration {
                            key: key.clone(),
                            target,
                            clsid: Some(fold(&id)),
                            note: "COM registration",
                        });
                    }
                }
            }
            let root = format!("{base}\\TypeLib");
            for id in reg.subkeys(&root) {
                let key = format!("{root}\\{id}");
                for ver in reg.subkeys(&key) {
                    for lcid in reg.subkeys(&format!("{key}\\{ver}")) {
                        for plat in ["win32", "win64"] {
                            if let Some(target) =
                                default_of(&format!("{key}\\{ver}\\{lcid}\\{plat}"))
                            {
                                ix.registrations.push(Registration {
                                    key: key.clone(),
                                    target,
                                    clsid: None,
                                    note: "Type library",
                                });
                            }
                        }
                    }
                }
            }
            for name in reg.subkeys(base) {
                let key = format!("{base}\\{name}");
                if name.starts_with('.') {
                    let owp = reg
                        .read_key(&format!("{key}\\OpenWithProgids"))
                        .map(|k| k.values.values().map(|v| v.name.clone()).collect())
                        .unwrap_or_default();
                    ix.extensions
                        .push((key, default_of(&format!("{base}\\{name}")), owp));
                    continue;
                }
                if name.starts_with('{') || name.eq_ignore_ascii_case("CLSID") {
                    continue;
                }
                if let Some(target) = default_of(&format!("{key}\\shell\\open\\command")) {
                    ix.registrations.push(Registration {
                        key,
                        target,
                        clsid: None,
                        note: "File type",
                    });
                }
            }
            let apps = format!("{base}\\Applications");
            for name in reg.subkeys(&apps) {
                let key = format!("{apps}\\{name}");
                if let Some(target) = default_of(&format!("{key}\\shell\\open\\command")) {
                    ix.registrations.push(Registration {
                        key,
                        target,
                        clsid: None,
                        note: "Open with entry",
                    });
                }
            }
            for t in SHELL_TARGETS {
                let shell = format!("{base}\\{t}\\shell");
                for verb in reg.subkeys(&shell) {
                    let key = format!("{shell}\\{verb}");
                    if let Some(target) = default_of(&format!("{key}\\command")) {
                        ix.registrations.push(Registration {
                            key,
                            target,
                            clsid: None,
                            note: "Context menu entry",
                        });
                    }
                }
                for kind in [
                    "ContextMenuHandlers",
                    "DragDropHandlers",
                    "PropertySheetHandlers",
                ] {
                    let root = format!("{base}\\{t}\\shellex\\{kind}");
                    for h in reg.subkeys(&root) {
                        let key = format!("{root}\\{h}");
                        let id = default_of(&key).unwrap_or_else(|| h.clone());
                        ix.handlers.push((key, fold(id.trim())));
                    }
                }
            }
        }
        if let Some(dir) = tasks_dir {
            ix.tasks = crate::startup::list_tasks_in(dir)
                .into_iter()
                .filter(|t| !t.microsoft)
                .collect();
        }
        ix
    }
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
            tasks_dir: Some(expand_env("%SystemRoot%\\System32\\Tasks"))
                .filter(|s| !s.contains('%')),
            deep: true,
            index: Default::default(),
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

    if scope.deep {
        let mut extra = Vec::new();
        deep_scan(p, reg, scope, &mut extra);
        for (action, note) in extra {
            add(&mut plan, action, note, &mut seen);
        }
        for (action, note) in empty_parents(&plan, scope, &protected) {
            add(&mut plan, action, note, &mut seen);
        }
    }

    plan.items
        .sort_by(|a, b| a.action.category().cmp(b.action.category()));
    plan
}

/// Registry roots holding COM classes and file types.
const CLASSES: [&str; 3] = [
    "HKLM\\SOFTWARE\\Classes",
    "HKLM\\SOFTWARE\\WOW6432Node\\Classes",
    "HKCU\\Software\\Classes",
];

/// Where Explorer looks for context-menu verbs and handlers.
const SHELL_TARGETS: [&str; 6] = [
    "*",
    "Directory",
    "Directory\\Background",
    "Folder",
    "Drive",
    "AllFilesystemObjects",
];

/// Everything else a program registers that points into its install folder:
/// COM classes and type libraries, file types and "Open with" entries,
/// context-menu verbs and handlers, environment variables and PATH entries,
/// firewall rules and scheduled tasks. Only entries that clearly point into
/// the program's own folder are reported.
fn deep_scan(
    p: &InstalledProgram,
    reg: &dyn Registry,
    scope: &LeftoverScope,
    out: &mut Vec<(Action, &'static str)>,
) {
    let loc = p.install_location.trim().trim_end_matches('\\');
    // A drive root or very short path would match far too much.
    if loc.len() <= 3 || !loc.contains('\\') && !loc.contains('/') {
        return;
    }
    // Some entries record a shared folder (Program Files, Windows, a user
    // profile) as their location; everything under it would match.
    if is_protected_dir(loc, &protected_dirs()) {
        return;
    }
    let floc = format!("{}\\", fold(&expand_env(loc)));
    let floc_bare = fold(&expand_env(loc));
    let points_in = |s: &str| {
        let f = fold(&expand_env(s));
        f.contains(&floc) || f.trim_matches('"').trim_end_matches('\\') == floc_bare
    };
    let ix = scope
        .index
        .get_or_init(|| DeepIndex::build(reg, scope.tasks_dir.as_deref()));

    // COM classes, type libraries, file types, "Open with" and verbs.
    let mut clsids: BTreeSet<String> = BTreeSet::new();
    let mut progids: BTreeSet<String> = BTreeSet::new();
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for r in &ix.registrations {
        if !points_in(&r.target) || !keys.insert(fold(&r.key)) {
            continue;
        }
        if let Some(id) = &r.clsid {
            clsids.insert(id.clone());
        }
        if r.note == "File type" {
            if let Some((_, name)) = r.key.rsplit_once('\\') {
                progids.insert(fold(name));
            }
        }
        out.push((
            Action::DeleteKey {
                path: r.key.clone(),
            },
            r.note,
        ));
    }
    // Extensions still pointing at a removed file type.
    for (key, default, owp) in &ix.extensions {
        if default.as_ref().is_some_and(|v| progids.contains(&fold(v))) {
            out.push((
                Action::DeleteValue {
                    key: key.clone(),
                    name: String::new(),
                },
                "File association",
            ));
        }
        for name in owp {
            if progids.contains(&fold(name)) {
                out.push((
                    Action::DeleteValue {
                        key: format!("{key}\\OpenWithProgids"),
                        name: name.clone(),
                    },
                    "Open with entry",
                ));
            }
        }
    }
    // Shell extensions loading a removed COM class.
    for (key, id) in &ix.handlers {
        if clsids.contains(id) {
            out.push((Action::DeleteKey { path: key.clone() }, "Shell extension"));
        }
    }

    // Environment variables and PATH entries.
    for key in [
        "HKCU\\Environment",
        "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
    ] {
        let Some(k) = reg.read_key(key) else { continue };
        for v in k.values.values() {
            let Some(data) = v.as_string() else { continue };
            if v.name.eq_ignore_ascii_case("Path") {
                let remove: Vec<String> = data
                    .split(';')
                    .filter(|e| !e.trim().is_empty() && points_in(e.trim()))
                    .map(|e| e.trim().to_string())
                    .collect();
                if !remove.is_empty() {
                    out.push((
                        Action::EditList {
                            key: key.into(),
                            name: v.name.clone(),
                            remove,
                            add_back: Vec::new(),
                        },
                        "PATH entry",
                    ));
                }
            } else if points_in(&data) {
                out.push((
                    Action::DeleteValue {
                        key: key.into(),
                        name: v.name.clone(),
                    },
                    "Environment variable",
                ));
            }
        }
    }

    // Firewall rules for the program's executables. Windows Firewall reads
    // them at start-up, so removal fully applies after a restart.
    let fw = "HKLM\\SYSTEM\\CurrentControlSet\\Services\\SharedAccess\\Parameters\\FirewallPolicy\\FirewallRules";
    if let Some(k) = reg.read_key(fw) {
        for v in k.values.values() {
            let Some(rule) = v.as_string() else { continue };
            let app = rule.split('|').find_map(|part| {
                part.strip_prefix("App=")
                    .or_else(|| part.strip_prefix("app="))
            });
            if app.is_some_and(&points_in) {
                out.push((
                    Action::DeleteValue {
                        key: fw.into(),
                        name: v.name.clone(),
                    },
                    "Firewall rule",
                ));
            }
        }
    }

    // Scheduled tasks that run something from the install folder.
    for task in &ix.tasks {
        if points_in(&task.command) {
            out.push((
                Action::DeleteTask {
                    name: task.name.clone(),
                },
                "Scheduled task",
            ));
        }
    }
}

/// Folders that would be left empty once the planned folders are removed
/// (typically a publisher folder around the program's folder).
fn empty_parents(
    plan: &Plan,
    scope: &LeftoverScope,
    protected: &[String],
) -> Vec<(Action, &'static str)> {
    let originals: Vec<String> = plan
        .items
        .iter()
        .filter_map(|i| match &i.action {
            Action::DeleteTree { path }
            | Action::DeleteDir { path }
            | Action::DeleteFile { path } => Some(path.trim_end_matches('\\').to_string()),
            _ => None,
        })
        .collect();
    let removed: BTreeSet<String> = originals.iter().map(|p| fold(p)).collect();
    let roots: BTreeSet<String> = scope
        .folders
        .iter()
        .chain(&scope.shortcut_dirs)
        .map(|f| fold(f.trim_end_matches('\\')))
        .collect();
    let mut out = Vec::new();
    let mut parents: std::collections::BTreeMap<String, String> = Default::default();
    for orig in &originals {
        let Some((parent, _)) = orig.rsplit_once(['\\', '/']) else {
            continue;
        };
        let fp = fold(parent);
        if parent.len() <= 3 || roots.contains(&fp) || removed.contains(&fp) {
            continue;
        }
        parents.entry(fp).or_insert_with(|| parent.to_string());
    }
    for parent in parents.into_values() {
        let Ok(rd) = std::fs::read_dir(&parent) else {
            continue;
        };
        let all_removed = rd
            .flatten()
            .all(|e| removed.contains(&fold(e.path().to_string_lossy().trim_end_matches('\\'))));
        if all_removed && !is_protected_dir(&parent, protected) {
            // DeleteDir only removes the folder if it really is empty by then.
            out.push((
                Action::DeleteDir { path: parent },
                "Empty folder left behind",
            ));
        }
    }
    out
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
            tasks_dir: None,
            deep: true,
            index: Default::default(),
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
        // (Match the full folder path: the random temp name may contain "pf".)
        let pf_s = pf.to_string_lossy().to_string();
        assert_eq!(
            d.iter()
                .filter(|x| x.contains("Foo Editor") && x.contains(&pf_s) && !x.contains("Pro"))
                .count(),
            1,
            "{d:#?}"
        );
    }

    #[test]
    fn deep_scan_finds_registrations_pointing_into_the_install_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let pf = tmp.path().join("pf");
        let loc_path = pf.join("FooCorp").join("Foo Editor");
        std::fs::create_dir_all(&loc_path).unwrap();
        // A sibling whose name starts the same must never match.
        std::fs::create_dir_all(pf.join("FooCorp").join("Foo Editor2")).unwrap();
        let loc = loc_path.to_string_lossy().to_string();
        let sib = pf
            .join("FooCorp")
            .join("Foo Editor2")
            .to_string_lossy()
            .to_string();
        let tasks = tmp.path().join("tasks");
        std::fs::create_dir_all(tasks.join("FooCorp")).unwrap();
        std::fs::write(
            tasks.join("FooCorp").join("Updater"),
            format!(
                "<Task><Actions><Exec><Command>{loc}\\update.exe</Command></Exec></Actions></Task>"
            ),
        )
        .unwrap();
        std::fs::write(
            tasks.join("Other"),
            format!("<Task><Actions><Exec><Command>{sib}\\x.exe</Command></Exec></Actions></Task>"),
        )
        .unwrap();

        let reg = MemRegistry::new();
        let c = "HKLM\\SOFTWARE\\Classes";
        let put = |path: &str, v: RegValue| reg.put(KeyData::new(path).with(v));
        put(
            &format!("{c}\\CLSID\\{{AAAA}}\\InprocServer32"),
            RegValue::string("", &format!("{loc}\\ext.dll")),
        );
        put(
            &format!("{c}\\CLSID\\{{BBBB}}\\InprocServer32"),
            RegValue::string("", &format!("{sib}\\ext.dll")),
        );
        put(
            &format!("{c}\\TypeLib\\{{CCCC}}\\1.0\\0\\win64"),
            RegValue::string("", &format!("{loc}\\foo.tlb")),
        );
        put(
            &format!("{c}\\FooEditor.Document\\shell\\open\\command"),
            RegValue::string("", &format!("\"{loc}\\foo.exe\" \"%1\"")),
        );
        put(
            &format!("{c}\\.foo"),
            RegValue::string("", "FooEditor.Document"),
        );
        put(
            &format!("{c}\\.txt\\OpenWithProgids"),
            RegValue::string("FooEditor.Document", ""),
        );
        put(
            &format!("{c}\\*\\shell\\EditWithFoo\\command"),
            RegValue::string("", &format!("\"{loc}\\foo.exe\" \"%1\"")),
        );
        put(
            &format!("{c}\\*\\shellex\\ContextMenuHandlers\\FooMenu"),
            RegValue::string("", "{AAAA}"),
        );
        put(
            &format!("{c}\\*\\shellex\\ContextMenuHandlers\\Other"),
            RegValue::string("", "{BBBB}"),
        );
        reg.put(
            KeyData::new("HKCU\\Environment")
                .with(RegValue::string(
                    "Path",
                    &format!("C:\\Tools;{loc}\\bin;{sib}\\bin"),
                ))
                .with(RegValue::string("FOO_HOME", &loc))
                .with(RegValue::string("OTHER_HOME", &sib)),
        );
        put(
            "HKLM\\SYSTEM\\CurrentControlSet\\Services\\SharedAccess\\Parameters\\FirewallPolicy\\FirewallRules",
            RegValue::string("{RULE1}", &format!("v2.31|Action=Allow|Active=TRUE|Dir=In|App={loc}\\foo.exe|Name=Foo|")),
        );

        let p = InstalledProgram {
            key: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Gone".into(),
            name: "Foo Editor".into(),
            publisher: "FooCorp".into(),
            install_location: loc.clone(),
            ..Default::default()
        };
        let scope = LeftoverScope {
            folders: vec![pf.to_string_lossy().into()],
            shortcut_dirs: vec![],
            reg_bases: vec![],
            tasks_dir: Some(tasks.to_string_lossy().into()),
            deep: true,
            index: Default::default(),
        };
        let plan = find_leftovers(&p, &reg, &scope);
        let d: Vec<String> = plan.items.iter().map(|i| i.action.describe()).collect();
        let has = |s: &str| d.iter().any(|x| x.contains(s));
        for want in [
            "CLSID\\{AAAA}",
            "TypeLib\\{CCCC}",
            "Classes\\FooEditor.Document",
            "Classes\\.foo",
            "OpenWithProgids\\FooEditor.Document",
            "shell\\EditWithFoo",
            "ContextMenuHandlers\\FooMenu",
            "FOO_HOME",
            "{RULE1}",
            "FooCorp\\Updater",
        ] {
            assert!(has(want), "missing {want}: {d:#?}");
        }
        for unwanted in [
            "{BBBB}",
            "ContextMenuHandlers\\Other",
            "OTHER_HOME",
            "\\Other",
        ] {
            assert!(!has(unwanted), "unexpected {unwanted}: {d:#?}");
        }
        let path_edit = plan
            .items
            .iter()
            .find_map(|i| match &i.action {
                Action::EditList { remove, .. } => Some(remove.clone()),
                _ => None,
            })
            .expect("PATH edit");
        assert_eq!(path_edit, vec![format!("{loc}\\bin")]);
        // FooCorp still holds "Foo Editor2", so it isn't offered as empty.
        assert!(
            !plan
                .items
                .iter()
                .any(|i| matches!(i.action, Action::DeleteDir { .. })),
            "{d:#?}"
        );

        // Once the sibling is gone, the publisher folder would be left empty.
        std::fs::remove_dir_all(pf.join("FooCorp").join("Foo Editor2")).unwrap();
        let plan = find_leftovers(&p, &reg, &scope);
        let fooc = pf.join("FooCorp").to_string_lossy().to_string();
        assert!(
            plan.items
                .iter()
                .any(|i| i.action == Action::DeleteDir { path: fooc.clone() }),
            "{:#?}",
            plan.items
                .iter()
                .map(|i| i.action.describe())
                .collect::<Vec<_>>()
        );
    }
}
