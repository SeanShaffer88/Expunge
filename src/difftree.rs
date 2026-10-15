//! A map of where an installation put things: every change grouped by the
//! kind of place it landed (program folders, hidden AppData, the Windows
//! folder, startup entries, services, scheduled tasks, shell integration…)
//! and then as a folder / registry tree, with counts and sizes per branch.

use crate::model::{fold, ChangeSet};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Area {
    Startup,
    Services,
    Tasks,
    Shell,
    Windows,
    ProgramFiles,
    AppDataRoaming,
    AppDataLocal,
    ProgramData,
    Shortcuts,
    UserFolders,
    Temp,
    MachineRegistry,
    UserRegistry,
    Other,
}

impl Area {
    pub const ALL: [Area; 15] = [
        Area::Startup,
        Area::Services,
        Area::Tasks,
        Area::Shell,
        Area::Windows,
        Area::ProgramFiles,
        Area::AppDataRoaming,
        Area::AppDataLocal,
        Area::ProgramData,
        Area::Shortcuts,
        Area::UserFolders,
        Area::Temp,
        Area::MachineRegistry,
        Area::UserRegistry,
        Area::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Area::Startup => "Starts with Windows",
            Area::Services => "Services and drivers",
            Area::Tasks => "Scheduled tasks",
            Area::Shell => "Explorer and file types",
            Area::Windows => "Windows folder",
            Area::ProgramFiles => "Program folders",
            Area::AppDataRoaming => "Hidden AppData (Roaming)",
            Area::AppDataLocal => "Hidden AppData (Local)",
            Area::ProgramData => "ProgramData (all users)",
            Area::Shortcuts => "Shortcuts",
            Area::UserFolders => "Your folders",
            Area::Temp => "Temporary files",
            Area::MachineRegistry => "Registry: all users",
            Area::UserRegistry => "Registry: your account",
            Area::Other => "Elsewhere",
        }
    }

    /// A short explanation shown on hover.
    pub fn hint(self) -> &'static str {
        match self {
            Area::Startup => "Run keys and Startup folders: these start every time you sign in.",
            Area::Services => "Background services and drivers, often started with Windows.",
            Area::Tasks => "Tasks Windows runs on a schedule, such as updaters.",
            Area::Shell => {
                "Right-click menus, shell extensions, COM classes and file associations."
            }
            Area::Windows => "Files placed inside Windows itself, such as System32.",
            Area::ProgramFiles => "Where the program itself lives.",
            Area::AppDataRoaming => {
                "Hidden settings folders in your profile that follow your account."
            }
            Area::AppDataLocal => "Hidden caches and data in your profile on this PC.",
            Area::ProgramData => "Data shared by every user of this PC.",
            Area::Shortcuts => "Start menu and desktop shortcuts.",
            Area::UserFolders => "Documents, Desktop and other folders in your profile.",
            Area::Temp => "Temporary files the installer left behind.",
            Area::MachineRegistry => "Registry settings for every user (HKLM).",
            Area::UserRegistry => "Registry settings for your account (HKCU).",
            Area::Other => "Anything else.",
        }
    }

    /// Places worth a closer look are listed first and drawn in warmer colors.
    pub fn attention(self) -> u8 {
        match self {
            Area::Startup | Area::Services | Area::Tasks => 3,
            Area::Shell | Area::Windows => 2,
            Area::AppDataRoaming | Area::AppDataLocal | Area::ProgramData | Area::Temp => 1,
            _ => 0,
        }
    }
}

fn env_lower(k: &str) -> String {
    let v = std::env::var(k).unwrap_or_default();
    // Windows always sets these; the defaults keep paths classified when
    // logs are viewed elsewhere (tests, the demo).
    let v = if v.is_empty() {
        match k {
            "windir" => "C:\\Windows".to_string(),
            "ProgramFiles" | "ProgramW6432" => "C:\\Program Files".to_string(),
            "ProgramFiles(x86)" => "C:\\Program Files (x86)".to_string(),
            "ProgramData" => "C:\\ProgramData".to_string(),
            _ => v,
        }
    } else {
        v
    };
    v.trim_end_matches('\\').to_lowercase()
}

/// Folders used to classify paths, read once.
#[derive(Clone, Debug, Default)]
pub struct Places {
    pub windir: String,
    pub program_files: Vec<String>,
    pub program_data: String,
    pub roaming: String,
    pub local: String,
    pub profile: String,
    pub temp: Vec<String>,
}

impl Places {
    pub fn current() -> Self {
        let mut pf = vec![
            env_lower("ProgramFiles"),
            env_lower("ProgramFiles(x86)"),
            env_lower("ProgramW6432"),
        ];
        pf.retain(|p| !p.is_empty());
        pf.dedup();
        let local = env_lower("LOCALAPPDATA");
        Places {
            windir: env_lower("windir"),
            program_files: pf,
            program_data: env_lower("ProgramData"),
            roaming: env_lower("APPDATA"),
            local: local.clone(),
            profile: env_lower("USERPROFILE"),
            temp: [env_lower("TEMP"), format!("{local}\\temp")]
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect(),
        }
    }

    fn under(path: &str, dir: &str) -> bool {
        !dir.is_empty()
            && path.len() > dir.len()
            && path.starts_with(dir)
            && path.as_bytes()[dir.len()] == b'\\'
    }

    /// The area and the root folder the tree starts from.
    pub fn file_area(&self, path: &str) -> (Area, String) {
        let l = path.to_lowercase();
        let root = |dir: &str| path[..dir.len()].to_string();
        let startup = ["\\microsoft\\windows\\start menu\\programs\\startup"];
        if startup.iter().any(|s| l.contains(s)) {
            let i = l.find(startup[0]).unwrap() + startup[0].len();
            return (Area::Startup, path[..i].to_string());
        }
        if l.contains("\\microsoft\\windows\\start menu")
            || l.ends_with(".lnk") && (l.contains("\\desktop\\"))
        {
            let i = l
                .find("\\start menu")
                .map(|i| i + "\\start menu".len())
                .unwrap_or_else(|| l.rfind('\\').unwrap_or(0));
            return (Area::Shortcuts, path[..i].to_string());
        }
        if Self::under(&l, &format!("{}\\system32\\tasks", self.windir)) {
            return (
                Area::Tasks,
                root(&format!("{}\\system32\\tasks", self.windir)),
            );
        }
        if l.ends_with(".sys") && Self::under(&l, &self.windir) {
            return (Area::Services, root(&self.windir));
        }
        for t in &self.temp {
            if Self::under(&l, t) {
                return (Area::Temp, root(t));
            }
        }
        if Self::under(&l, &self.windir) {
            return (Area::Windows, root(&self.windir));
        }
        for pf in &self.program_files {
            if Self::under(&l, pf) {
                return (Area::ProgramFiles, root(pf));
            }
        }
        let local_programs = format!("{}\\programs", self.local);
        if Self::under(&l, &local_programs) {
            return (Area::ProgramFiles, root(&local_programs));
        }
        if Self::under(&l, &self.roaming) {
            return (Area::AppDataRoaming, root(&self.roaming));
        }
        if Self::under(&l, &self.local) {
            return (Area::AppDataLocal, root(&self.local));
        }
        let low = format!("{}\\appdata\\locallow", self.profile);
        if Self::under(&l, &low) {
            return (Area::AppDataLocal, root(&low));
        }
        if Self::under(&l, &self.program_data) {
            return (Area::ProgramData, root(&self.program_data));
        }
        if Self::under(&l, &self.profile) {
            return (Area::UserFolders, root(&self.profile));
        }
        let drive = path.get(..2).unwrap_or("").to_string();
        (Area::Other, drive)
    }
}

/// The area and root of a registry path.
pub fn key_area(path: &str) -> (Area, String) {
    let l = path.to_lowercase();
    let prefix = |n: usize| path[..n.min(path.len())].to_string();
    for run in [
        "\\microsoft\\windows\\currentversion\\runonce",
        "\\microsoft\\windows\\currentversion\\run",
        "\\microsoft\\windows\\currentversion\\policies\\explorer\\run",
        "\\wow6432node\\microsoft\\windows\\currentversion\\run",
    ] {
        if let Some(i) = l.find(run) {
            let end = i + run.len();
            if l.len() == end || l.as_bytes()[end] == b'\\' {
                return (Area::Startup, prefix(end));
            }
        }
    }
    if let Some(i) = l.find("\\currentcontrolset\\services") {
        return (
            Area::Services,
            prefix(i + "\\currentcontrolset\\services".len()),
        );
    }
    if let Some(i) = l.find("\\schedule\\taskcache") {
        return (Area::Tasks, prefix(i + "\\schedule\\taskcache".len()));
    }
    for shell in [
        "\\software\\classes",
        "\\microsoft\\windows\\currentversion\\explorer",
        "\\microsoft\\windows\\currentversion\\shell extensions",
    ] {
        if let Some(i) = l.find(shell) {
            return (Area::Shell, prefix(i + shell.len()));
        }
    }
    let (area, root) = if l.starts_with("hklm") {
        (Area::MachineRegistry, "HKLM")
    } else if l.starts_with("hkcu") || l.starts_with("hku") {
        (
            Area::UserRegistry,
            if l.starts_with("hkcu") { "HKCU" } else { "HKU" },
        )
    } else {
        (Area::Other, "")
    };
    (area, prefix(root.len()))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub added: u32,
    pub changed: u32,
    pub removed: u32,
    pub bytes: u64,
}

impl Counts {
    pub fn total(&self) -> u32 {
        self.added + self.changed + self.removed
    }
    fn add(&mut self, o: &Counts) {
        self.added += o.added;
        self.changed += o.changed;
        self.removed += o.removed;
        self.bytes += o.bytes;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Changed,
    Removed,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    /// One path segment, or several joined with `\` when a branch has a
    /// single child (so deep, narrow trees stay readable).
    pub name: String,
    pub full: String,
    pub is_key: bool,
    /// What happened to this entry itself (None for folders/keys that only
    /// lead to changes further down).
    pub change: Option<Change>,
    /// Totals for this node and everything under it.
    pub counts: Counts,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, Default)]
pub struct AreaTree {
    pub area: Option<Area>,
    pub root: Node,
}

#[derive(Clone, Debug, Default)]
pub struct DiffTree {
    pub areas: Vec<AreaTree>,
    pub total: Counts,
}

#[derive(Default)]
struct Builder {
    name: String,
    full: String,
    is_key: bool,
    change: Option<Change>,
    own: Counts,
    children: BTreeMap<String, Builder>,
}

impl Builder {
    fn insert(&mut self, segments: &[&str], is_key: bool, change: Change, bytes: u64) {
        let Some((first, rest)) = segments.split_first() else {
            self.change = Some(change);
            match change {
                Change::Added => self.own.added += 1,
                Change::Changed => self.own.changed += 1,
                Change::Removed => self.own.removed += 1,
            }
            self.own.bytes += bytes;
            return;
        };
        let child_full = if self.full.is_empty() {
            first.to_string()
        } else {
            format!("{}\\{}", self.full, first)
        };
        let child = self.children.entry(fold(first)).or_insert_with(|| Builder {
            name: first.to_string(),
            full: child_full,
            is_key,
            ..Default::default()
        });
        child.insert(rest, is_key, change, bytes);
    }

    fn finish(self) -> Node {
        let mut children: Vec<Node> = self.children.into_values().map(Builder::finish).collect();
        let mut counts = self.own;
        for c in &children {
            counts.add(&c.counts);
        }
        // Biggest branches first, then by name.
        children.sort_by(|a, b| {
            b.counts
                .total()
                .cmp(&a.counts.total())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let mut node = Node {
            name: self.name,
            full: self.full,
            is_key: self.is_key,
            change: self.change,
            counts,
            children,
        };
        // Collapse "a > b > c" chains that only lead somewhere.
        while node.change.is_none()
            && node.children.len() == 1
            && !node.children[0].children.is_empty()
        {
            let c = node.children.remove(0);
            node.name = format!("{}\\{}", node.name, c.name);
            node.full = c.full;
            node.change = c.change;
            node.children = c.children;
        }
        node
    }
}

/// Build the map for a set of changes.
pub fn build(cs: &ChangeSet, places: &Places) -> DiffTree {
    // (area, root, full path, is key, change, bytes)
    let mut items: Vec<(Area, String, String, bool, Change, u64)> = Vec::new();
    let mut file = |path: &str, change: Change, bytes: u64| {
        let (area, root) = places.file_area(path);
        items.push((area, root, path.to_string(), false, change, bytes));
    };
    for f in &cs.files_added {
        file(&f.path, Change::Added, f.size);
    }
    for f in &cs.files_modified {
        file(&f.after.path, Change::Changed, 0);
    }
    for f in &cs.files_removed {
        file(&f.path, Change::Removed, 0);
    }
    let value = |v_key: &str, name: &str| {
        format!(
            "{v_key}\\{}",
            if name.is_empty() { "(Default)" } else { name }
        )
    };
    let keys = cs
        .keys_added
        .iter()
        .map(|k| (k.path.clone(), Change::Added))
        .chain(
            cs.keys_removed
                .iter()
                .map(|k| (k.path.clone(), Change::Removed)),
        )
        .chain(
            cs.values_added
                .iter()
                .map(|v| (value(&v.key, &v.value.name), Change::Added)),
        )
        .chain(
            cs.values_modified
                .iter()
                .map(|v| (value(&v.key, &v.after.name), Change::Changed)),
        )
        .chain(
            cs.values_removed
                .iter()
                .map(|v| (value(&v.key, &v.value.name), Change::Removed)),
        );
    for (path, change) in keys {
        let (area, root) = key_area(&path);
        items.push((area, root, path, true, change, 0));
    }
    let mut areas: BTreeMap<(u8, Area), Builder> = BTreeMap::new();
    for (area, root, full, is_key, change, bytes) in items {
        let b = areas
            .entry((3 - area.attention(), area))
            .or_insert_with(|| Builder {
                name: area.label().into(),
                full: root.clone(),
                is_key,
                ..Default::default()
            });
        let rel = full.get(root.len()..).unwrap_or(&full);
        let segs: Vec<&str> = rel.split('\\').filter(|s| !s.is_empty()).collect();
        b.insert(&segs, is_key, change, bytes);
    }
    let mut tree = DiffTree::default();
    for ((_, area), b) in areas {
        let mut root = b.finish();
        // The area keeps its friendly name; the collapsed path shows below it.
        if root.name != area.label() {
            let rest = root
                .name
                .split_once('\\')
                .map(|(_, r)| r.to_string())
                .unwrap_or_default();
            root.name = if rest.is_empty() {
                area.label().to_string()
            } else {
                format!("{}  ›  {rest}", area.label())
            };
        }
        tree.total.add(&root.counts);
        tree.areas.push(AreaTree {
            area: Some(area),
            root,
        });
    }
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FileEntry, KeyData, RegValue, ValueRef};

    fn places() -> Places {
        Places {
            windir: "c:\\windows".into(),
            program_files: vec!["c:\\program files".into(), "c:\\program files (x86)".into()],
            program_data: "c:\\programdata".into(),
            roaming: "c:\\users\\sean\\appdata\\roaming".into(),
            local: "c:\\users\\sean\\appdata\\local".into(),
            profile: "c:\\users\\sean".into(),
            temp: vec!["c:\\users\\sean\\appdata\\local\\temp".into()],
        }
    }

    fn f(p: &str, size: u64) -> FileEntry {
        FileEntry {
            path: p.into(),
            is_dir: false,
            size,
            mtime: 0,
        }
    }

    #[test]
    fn classifies_places() {
        let p = places();
        assert_eq!(
            p.file_area("C:\\Program Files\\App\\a.exe").0,
            Area::ProgramFiles
        );
        assert_eq!(
            p.file_area("C:\\Users\\Sean\\AppData\\Roaming\\App\\x.ini")
                .0,
            Area::AppDataRoaming
        );
        assert_eq!(
            p.file_area("C:\\Users\\Sean\\AppData\\Local\\Programs\\App\\a.exe")
                .0,
            Area::ProgramFiles
        );
        assert_eq!(
            p.file_area("C:\\Users\\Sean\\AppData\\Local\\Temp\\x.tmp")
                .0,
            Area::Temp
        );
        assert_eq!(
            p.file_area("C:\\Windows\\System32\\Tasks\\AppUpdate").0,
            Area::Tasks
        );
        assert_eq!(
            p.file_area("C:\\Windows\\System32\\drivers\\app.sys").0,
            Area::Services
        );
        assert_eq!(
            p.file_area("C:\\Windows\\System32\\app.dll").0,
            Area::Windows
        );
        assert_eq!(p.file_area("C:\\Users\\Sean\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\App.lnk").0, Area::Startup);
        assert_eq!(
            p.file_area("C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\\App.lnk")
                .0,
            Area::Shortcuts
        );
        assert_eq!(
            key_area("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\\App").0,
            Area::Startup
        );
        assert_eq!(
            key_area("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Runtime").0,
            Area::UserRegistry
        );
        assert_eq!(
            key_area("HKLM\\SYSTEM\\CurrentControlSet\\Services\\AppSvc").0,
            Area::Services
        );
        assert_eq!(key_area("HKLM\\SOFTWARE\\Classes\\.foo").0, Area::Shell);
        assert_eq!(key_area("HKLM\\SOFTWARE\\Acme").0, Area::MachineRegistry);
    }

    #[test]
    fn builds_a_compact_tree() {
        let cs = ChangeSet {
            files_added: vec![
                f("C:\\Program Files\\Acme\\App\\a.exe", 100),
                f("C:\\Program Files\\Acme\\App\\b.dll", 50),
                f("C:\\Users\\Sean\\AppData\\Roaming\\Acme\\settings.ini", 1),
            ],
            keys_added: vec![KeyData::new("HKLM\\SOFTWARE\\Acme\\App")],
            values_added: vec![ValueRef {
                key: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
                value: RegValue::string("Acme", "x"),
            }],
            ..Default::default()
        };
        let t = build(&cs, &places());
        assert_eq!(t.total.added, 5);
        assert_eq!(t.total.bytes, 151);
        // Startup comes first: it needs the most attention.
        assert_eq!(t.areas[0].area, Some(Area::Startup));
        let pf = t
            .areas
            .iter()
            .find(|a| a.area == Some(Area::ProgramFiles))
            .unwrap();
        assert_eq!(pf.root.counts.added, 2);
        // "Acme\App" collapsed into one branch holding the two files.
        assert!(pf.root.name.contains("Acme\\App"), "{}", pf.root.name);
        assert_eq!(pf.root.children.len(), 2);
        assert_eq!(pf.root.children[0].name, "a.exe");
        assert_eq!(
            pf.root.children[0].full,
            "C:\\Program Files\\Acme\\App\\a.exe"
        );
    }
}
