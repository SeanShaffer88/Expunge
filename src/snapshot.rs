//! Capturing system state and comparing two captures.

use crate::model::*;
use crate::platform::{self, expand_env, Registry};
use crate::settings::Settings;
use crate::util::is_excluded;
use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

/// Shared progress/cancel state for long-running work on a background thread.
#[derive(Default)]
pub struct Progress {
    pub phase: Mutex<String>,
    pub items: AtomicU64,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn set_phase(&self, s: impl Into<String>) {
        *self.phase.lock().unwrap() = s.into();
    }
    pub fn phase(&self) -> String {
        self.phase.lock().unwrap().clone()
    }
    pub fn tick(&self) {
        self.items.fetch_add(1, Ordering::Relaxed);
    }
    pub fn count(&self) -> u64 {
        self.items.load(Ordering::Relaxed)
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Capture the configured registry roots and folders.
pub fn take_snapshot(
    reg: &dyn Registry,
    settings: &Settings,
    progress: &Progress,
) -> Result<Snapshot> {
    let mut snap = Snapshot {
        created: crate::util::now_string(),
        computer: platform::computer_name(),
        user: platform::user_name(),
        ..Default::default()
    };
    let reg_excl: Vec<String> = settings.registry_exclusions.clone();
    for root in &settings.registry_roots {
        let root = platform::normalize_reg_path(root);
        progress.set_phase(format!("Registry: {root}"));
        capture_registry(reg, &root, &reg_excl, &mut snap, progress);
        if progress.cancelled() {
            anyhow::bail!("cancelled");
        }
    }
    let file_excl: Vec<String> = settings
        .file_exclusions
        .iter()
        .map(|s| expand_env(s))
        .collect();
    for root in &settings.file_roots {
        let path = expand_env(&root.path);
        if path.contains('%') || !Path::new(&path).exists() {
            continue;
        }
        progress.set_phase(format!("Files: {path}"));
        capture_files(
            Path::new(&path),
            root.recursive,
            &file_excl,
            &mut snap,
            progress,
        );
        if progress.cancelled() {
            anyhow::bail!("cancelled");
        }
    }
    progress.set_phase("Done");
    Ok(snap)
}

/// Depth-first walk of a registry subtree, skipping excluded branches.
pub fn capture_registry(
    reg: &dyn Registry,
    root: &str,
    excl: &[String],
    snap: &mut Snapshot,
    progress: &Progress,
) {
    let mut stack = vec![root.to_string()];
    while let Some(path) = stack.pop() {
        if progress.cancelled() {
            return;
        }
        if is_excluded(&path, excl) {
            continue;
        }
        match reg.read_key(&path) {
            Some(k) => {
                snap.insert_key(k);
                progress.tick();
            }
            None => {
                snap.warnings.push(format!("Could not read {path}"));
                continue;
            }
        }
        for sub in reg.subkeys(&path) {
            stack.push(format!("{path}\\{sub}"));
        }
    }
}

fn mtime_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Record every file and folder under `root` (not following links/junctions).
pub fn capture_files(
    root: &Path,
    recursive: bool,
    excl: &[String],
    snap: &mut Snapshot,
    progress: &Progress,
) {
    let walker = walkdir::WalkDir::new(root)
        .min_depth(1)
        .max_depth(if recursive { usize::MAX } else { 1 })
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_excluded(&e.path().to_string_lossy(), excl));
    for entry in walker {
        if progress.cancelled() {
            return;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                if let Some(p) = e.path() {
                    snap.warnings
                        .push(format!("Could not read {}", p.display()));
                }
                continue;
            }
        };
        if entry.path_is_symlink() {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let is_dir = meta.is_dir();
        snap.insert_file(FileEntry {
            path: entry.path().to_string_lossy().to_string(),
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
            mtime: mtime_of(&meta),
        });
        progress.tick();
    }
}

/// Compare two snapshots. Results are sorted by path.
pub fn diff(before: &Snapshot, after: &Snapshot) -> ChangeSet {
    let mut cs = ChangeSet::default();

    for (fk, ak) in &after.registry {
        match before.registry.get(fk) {
            None => cs.keys_added.push(ak.clone()),
            Some(bk) => {
                for (fv, av) in &ak.values {
                    match bk.values.get(fv) {
                        None => cs.values_added.push(ValueRef {
                            key: ak.path.clone(),
                            value: av.clone(),
                        }),
                        Some(bv) if bv.kind != av.kind || bv.data != av.data => {
                            cs.values_modified.push(ValueChange {
                                key: ak.path.clone(),
                                before: bv.clone(),
                                after: av.clone(),
                            })
                        }
                        _ => {}
                    }
                }
                for (fv, bv) in &bk.values {
                    if !ak.values.contains_key(fv) {
                        cs.values_removed.push(ValueRef {
                            key: ak.path.clone(),
                            value: bv.clone(),
                        });
                    }
                }
            }
        }
    }
    for (fk, bk) in &before.registry {
        if !after.registry.contains_key(fk) {
            cs.keys_removed.push(bk.clone());
        }
    }

    for (fp, af) in &after.files {
        match before.files.get(fp) {
            None => cs.files_added.push(af.clone()),
            Some(bf) => {
                // Folder timestamps change whenever their contents do; only
                // files count as modified.
                if !af.is_dir && (bf.size != af.size || bf.mtime != af.mtime || bf.is_dir) {
                    cs.files_modified.push(FileChange {
                        before: bf.clone(),
                        after: af.clone(),
                    });
                }
            }
        }
    }
    for (fp, bf) in &before.files {
        if !after.files.contains_key(fp) {
            cs.files_removed.push(bf.clone());
        }
    }
    cs
}

/// Drop changes matching exclusion patterns after the fact (used when the
/// user adds an exclusion while reviewing a capture).
pub fn filter_changes(cs: &mut ChangeSet, reg_excl: &[String], file_excl: &[String]) {
    let file_excl: Vec<String> = file_excl.iter().map(|s| expand_env(s)).collect();
    cs.keys_added.retain(|k| !is_excluded(&k.path, reg_excl));
    cs.keys_removed.retain(|k| !is_excluded(&k.path, reg_excl));
    cs.values_added.retain(|v| !is_excluded(&v.key, reg_excl));
    cs.values_removed.retain(|v| !is_excluded(&v.key, reg_excl));
    cs.values_modified
        .retain(|v| !is_excluded(&v.key, reg_excl));
    cs.files_added.retain(|f| !is_excluded(&f.path, &file_excl));
    cs.files_removed
        .retain(|f| !is_excluded(&f.path, &file_excl));
    cs.files_modified
        .retain(|f| !is_excluded(&f.after.path, &file_excl));
}

/// Program identity found in a change set (from a new Uninstall entry).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedProgram {
    pub key: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
}

pub fn is_uninstall_entry(path: &str) -> bool {
    let f = fold(path);
    let marker = "\\microsoft\\windows\\currentversion\\uninstall\\";
    match f.find(marker) {
        Some(i) => !f[i + marker.len()..].contains('\\'),
        None => false,
    }
}

/// Find the Uninstall entry an installation created (or updated).
pub fn detect_program(cs: &ChangeSet) -> Option<DetectedProgram> {
    let from_key = |k: &KeyData| {
        k.get_string("DisplayName").map(|name| DetectedProgram {
            key: k.path.clone(),
            name,
            publisher: k.get_string("Publisher").unwrap_or_default(),
            version: k.get_string("DisplayVersion").unwrap_or_default(),
        })
    };
    let mut found: Vec<DetectedProgram> = cs
        .keys_added
        .iter()
        .filter(|k| is_uninstall_entry(&k.path))
        .filter_map(from_key)
        .collect();
    if found.is_empty() {
        // An upgrade rewrites an existing entry instead.
        let mut keys: std::collections::BTreeMap<String, KeyData> = Default::default();
        for v in &cs.values_modified {
            if is_uninstall_entry(&v.key) {
                keys.entry(fold(&v.key))
                    .or_insert_with(|| KeyData::new(&v.key))
                    .values
                    .insert(fold(&v.after.name), v.after.clone());
            }
        }
        for v in &cs.values_added {
            if is_uninstall_entry(&v.key) {
                keys.entry(fold(&v.key))
                    .or_insert_with(|| KeyData::new(&v.key))
                    .values
                    .insert(fold(&v.value.name), v.value.clone());
            }
        }
        found = keys.values().filter_map(from_key).collect();
    }
    // Prefer the entry that isn't a hidden component / update.
    found.into_iter().next()
}

/// Save a snapshot as gzip-compressed JSON.
pub fn save_snapshot(path: &Path, snap: &Snapshot) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let f = std::fs::File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut gz =
        flate2::write::GzEncoder::new(std::io::BufWriter::new(f), flate2::Compression::fast());
    serde_json::to_writer(&mut gz, snap)?;
    gz.finish()?.flush()?;
    Ok(())
}

pub fn load_snapshot(path: &Path) -> Result<Snapshot> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut s = String::new();
    flate2::read::GzDecoder::new(std::io::BufReader::new(f)).read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::MemRegistry;

    fn settings_for(root: &str) -> Settings {
        Settings {
            registry_roots: vec!["HKLM\\SOFTWARE".into()],
            registry_exclusions: vec!["HKLM\\SOFTWARE\\Noise".into()],
            file_roots: vec![crate::settings::FileRoot {
                path: root.into(),
                recursive: true,
            }],
            file_exclusions: vec!["*cache".into()],
            ..Settings::default()
        }
    }

    #[test]
    fn snapshot_diff_detects_install() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().to_string();
        let reg = MemRegistry::new();
        reg.put(
            KeyData::new("HKLM\\SOFTWARE\\Existing")
                .with(RegValue::dword("a", 1))
                .with(RegValue::dword("gone", 9)),
        );
        reg.put(KeyData::new("HKLM\\SOFTWARE\\Old"));
        std::fs::write(tmp.path().join("keep.txt"), "x").unwrap();
        let s = settings_for(&root);
        let p = Progress::default();
        let before = take_snapshot(&reg, &s, &p).unwrap();

        // "Install"
        let un = "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Foo";
        reg.put(
            KeyData::new(un)
                .with(RegValue::string("DisplayName", "Foo App"))
                .with(RegValue::string("Publisher", "FooCorp")),
        );
        reg.put(
            KeyData::new("HKLM\\SOFTWARE\\FooCorp\\Foo").with(RegValue::string("Path", "C:\\Foo")),
        );
        reg.set_value("HKLM\\SOFTWARE\\Existing", &RegValue::dword("a", 2))
            .unwrap();
        reg.set_value("HKLM\\SOFTWARE\\Existing", &RegValue::dword("b", 3))
            .unwrap();
        reg.delete_value("HKLM\\SOFTWARE\\Existing", "gone")
            .unwrap();
        reg.delete_tree("HKLM\\SOFTWARE\\Old").unwrap();
        reg.put(KeyData::new("HKLM\\SOFTWARE\\Noise\\X"));
        std::fs::create_dir_all(tmp.path().join("Foo").join("cache")).unwrap();
        std::fs::write(tmp.path().join("Foo").join("foo.exe"), "bin").unwrap();
        std::fs::write(tmp.path().join("Foo").join("cache").join("c.bin"), "c").unwrap();

        let after = take_snapshot(&reg, &s, &p).unwrap();
        let cs = diff(&before, &after);

        let added: Vec<&str> = cs.keys_added.iter().map(|k| k.path.as_str()).collect();
        assert!(added.contains(&un));
        assert!(added.contains(&"HKLM\\SOFTWARE\\FooCorp\\Foo"));
        assert!(!added.iter().any(|k| k.contains("Noise")));
        assert_eq!(cs.values_added.len(), 1);
        assert_eq!(cs.values_modified.len(), 1);
        assert_eq!(cs.values_removed.len(), 1);
        assert_eq!(cs.keys_removed.len(), 1);
        let files: Vec<String> = cs.files_added.iter().map(|f| f.path.clone()).collect();
        assert_eq!(files.len(), 2, "{files:?}"); // Foo dir + foo.exe, cache excluded

        let d = detect_program(&cs).unwrap();
        assert_eq!(d.name, "Foo App");
        assert_eq!(d.publisher, "FooCorp");
    }

    #[test]
    fn snapshot_save_load() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Snapshot::default();
        s.insert_key(KeyData::new("HKCU\\Software\\A").with(RegValue::string("x", "y")));
        let p = tmp.path().join("a.snap");
        save_snapshot(&p, &s).unwrap();
        let l = load_snapshot(&p).unwrap();
        assert_eq!(l.registry, s.registry);
    }

    #[test]
    fn uninstall_entry_detection() {
        assert!(is_uninstall_entry(
            "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\X"
        ));
        assert!(!is_uninstall_entry(
            "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\X\\Sub"
        ));
    }
}
