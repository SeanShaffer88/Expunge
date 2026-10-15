//! Merging install logs: one record for a program installed in several steps
//! (an installer plus its update, a program plus its plugins).
//!
//! Logs are applied in order, so later changes win: a file added by the first
//! and removed by the second is not in the result at all, a value changed
//! twice keeps its first "before" and last "after", and so on.

use crate::model::{
    fold, ChangeSet, FileChange, FileEntry, InstallLog, KeyData, RegValue, ValueChange, ValueRef,
};
use std::collections::BTreeMap;

enum Net<T> {
    Added(T),
    Removed(T),
    Modified(T, T),
}

fn apply<T: Clone>(map: &mut BTreeMap<String, Net<T>>, key: String, change: Net<T>) {
    use Net::*;
    let next = match (map.remove(&key), change) {
        (None, c) => Some(c),
        // Added, then removed: as if it never happened.
        (Some(Added(_)), Removed(_)) => None,
        (Some(Added(_)), Modified(_, after)) => Some(Added(after)),
        (Some(Added(_)), Added(n)) => Some(Added(n)),
        // Removed, then added back: a change from the original.
        (Some(Removed(before)), Added(after)) => Some(Modified(before, after)),
        (Some(Removed(b)), _) => Some(Removed(b)),
        (Some(Modified(before, _)), Modified(_, after)) => Some(Modified(before, after)),
        (Some(Modified(before, _)), Removed(_)) => Some(Removed(before)),
        (Some(Modified(before, _)), Added(after)) => Some(Modified(before, after)),
    };
    if let Some(n) = next {
        map.insert(key, n);
    }
}

/// The combined changes of `sets`, applied in order.
pub fn merge_changes(sets: &[&ChangeSet]) -> ChangeSet {
    let mut files: BTreeMap<String, Net<FileEntry>> = BTreeMap::new();
    let mut keys: BTreeMap<String, Net<KeyData>> = BTreeMap::new();
    let mut values: BTreeMap<String, Net<(String, RegValue)>> = BTreeMap::new();
    let vkey = |k: &str, v: &RegValue| format!("{}\u{0}{}", fold(k), fold(&v.name));
    for cs in sets {
        for f in &cs.files_added {
            apply(&mut files, fold(&f.path), Net::Added(f.clone()));
        }
        for f in &cs.files_removed {
            apply(&mut files, fold(&f.path), Net::Removed(f.clone()));
        }
        for f in &cs.files_modified {
            apply(
                &mut files,
                fold(&f.after.path),
                Net::Modified(f.before.clone(), f.after.clone()),
            );
        }
        for k in &cs.keys_added {
            apply(&mut keys, fold(&k.path), Net::Added(k.clone()));
        }
        for k in &cs.keys_removed {
            apply(&mut keys, fold(&k.path), Net::Removed(k.clone()));
        }
        for v in &cs.values_added {
            apply(
                &mut values,
                vkey(&v.key, &v.value),
                Net::Added((v.key.clone(), v.value.clone())),
            );
        }
        for v in &cs.values_removed {
            apply(
                &mut values,
                vkey(&v.key, &v.value),
                Net::Removed((v.key.clone(), v.value.clone())),
            );
        }
        for v in &cs.values_modified {
            apply(
                &mut values,
                vkey(&v.key, &v.after),
                Net::Modified(
                    (v.key.clone(), v.before.clone()),
                    (v.key.clone(), v.after.clone()),
                ),
            );
        }
    }
    let mut out = ChangeSet::default();
    for n in files.into_values() {
        match n {
            Net::Added(f) => out.files_added.push(f),
            Net::Removed(f) => out.files_removed.push(f),
            Net::Modified(before, after) => out.files_modified.push(FileChange { before, after }),
        }
    }
    for n in keys.into_values() {
        match n {
            Net::Added(k) => out.keys_added.push(k),
            Net::Removed(k) => out.keys_removed.push(k),
            // A key removed and added back: keep it as added (its values are current).
            Net::Modified(_, k) => out.keys_added.push(k),
        }
    }
    for n in values.into_values() {
        match n {
            Net::Added((key, value)) => out.values_added.push(ValueRef { key, value }),
            Net::Removed((key, value)) => out.values_removed.push(ValueRef { key, value }),
            Net::Modified((key, before), (_, after)) => {
                if before != after {
                    out.values_modified.push(ValueChange { key, before, after })
                }
            }
        }
    }
    out
}

/// One log made from several, oldest first. The program details come from
/// the first log that has them.
pub fn merge_logs(logs: &[&InstallLog], name: &str) -> InstallLog {
    let mut sorted: Vec<&InstallLog> = logs.to_vec();
    sorted.sort_by(|a, b| a.created.cmp(&b.created));
    let pick = |f: fn(&InstallLog) -> &String| {
        sorted
            .iter()
            .map(|l| f(l))
            .find(|s| !s.is_empty())
            .cloned()
            .unwrap_or_default()
    };
    let mut tags: Vec<String> = Vec::new();
    for l in &sorted {
        for t in &l.tags {
            if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                tags.push(t.clone());
            }
        }
    }
    let sets: Vec<&ChangeSet> = sorted.iter().map(|l| &l.changes).collect();
    InstallLog {
        id: crate::util::new_id(),
        name: if name.trim().is_empty() {
            pick(|l| &l.name)
        } else {
            name.trim().to_string()
        },
        publisher: pick(|l| &l.publisher),
        // The newest version wins.
        version: sorted
            .iter()
            .rev()
            .map(|l| &l.version)
            .find(|s| !s.is_empty())
            .cloned()
            .unwrap_or_default(),
        created: crate::util::now_string(),
        installer: sorted
            .iter()
            .map(|l| l.installer.as_str())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" + "),
        uninstall_key: pick(|l| &l.uninstall_key),
        notes: format!(
            "Merged from: {}",
            sorted
                .iter()
                .map(|l| format!("{} ({})", l.name, l.created))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        group: pick(|l| &l.group),
        tags,
        changes: merge_changes(&sets),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(p: &str, size: u64) -> FileEntry {
        FileEntry {
            path: p.into(),
            is_dir: false,
            size,
            mtime: 0,
        }
    }

    #[test]
    fn later_changes_win() {
        let a = ChangeSet {
            files_added: vec![file("C:\\App\\a.exe", 1), file("C:\\App\\tmp.dat", 1)],
            keys_added: vec![KeyData::new("HKLM\\SOFTWARE\\App")],
            values_modified: vec![ValueChange {
                key: "HKCU\\Environment".into(),
                before: RegValue::string("Path", "x"),
                after: RegValue::string("Path", "x;y"),
            }],
            ..Default::default()
        };
        let b = ChangeSet {
            files_removed: vec![file("C:\\App\\tmp.dat", 1)],
            files_modified: vec![FileChange {
                before: file("C:\\App\\a.exe", 1),
                after: file("C:\\App\\a.exe", 2),
            }],
            files_added: vec![file("C:\\App\\plugin.dll", 3)],
            values_modified: vec![ValueChange {
                key: "HKCU\\Environment".into(),
                before: RegValue::string("Path", "x;y"),
                after: RegValue::string("Path", "x;y;z"),
            }],
            ..Default::default()
        };
        let m = merge_changes(&[&a, &b]);
        let paths: Vec<_> = m
            .files_added
            .iter()
            .map(|f| (f.path.as_str(), f.size))
            .collect();
        assert_eq!(
            paths,
            vec![("C:\\App\\a.exe", 2), ("C:\\App\\plugin.dll", 3)]
        );
        assert!(m.files_removed.is_empty() && m.files_modified.is_empty());
        assert_eq!(m.keys_added.len(), 1);
        assert_eq!(m.values_modified.len(), 1);
        assert_eq!(m.values_modified[0].before, RegValue::string("Path", "x"));
        assert_eq!(
            m.values_modified[0].after,
            RegValue::string("Path", "x;y;z")
        );
    }

    #[test]
    fn merged_log_details() {
        let a = InstallLog {
            name: "App".into(),
            version: "1.0".into(),
            created: "2026-01-01 10:00".into(),
            tags: vec!["x".into()],
            ..Default::default()
        };
        let b = InstallLog {
            name: "App update".into(),
            version: "1.1".into(),
            created: "2026-02-01 10:00".into(),
            tags: vec!["X".into(), "y".into()],
            ..Default::default()
        };
        let m = merge_logs(&[&b, &a], "");
        assert_eq!(m.name, "App");
        assert_eq!(m.version, "1.1");
        assert_eq!(m.tags, vec!["x", "y"]);
        assert!(m.notes.starts_with("Merged from: App (2026-01-01"));
    }
}
