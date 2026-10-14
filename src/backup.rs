//! Backups: everything Expunge removes or changes is first copied here, so any
//! uninstall, cleanup or startup change can be rolled back.

use crate::model::{KeyData, RegValue};
use crate::platform::{self, Registry};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum BackupItem {
    /// A file copied into the backup before deletion.
    File {
        original: String,
        stored: String,
        size: u64,
    },
    /// A folder that existed (recreated on restore).
    Dir { original: String },
    /// A registry subtree captured before deletion.
    RegTree { keys: Vec<KeyData> },
    /// A value's previous state (`None` = it did not exist).
    RegValue {
        key: String,
        name: String,
        previous: Option<RegValue>,
    },
    /// A key that did not exist before the operation created it.
    KeyAbsent { path: String },
    /// A file that did not exist before the operation created it
    /// (restoring deletes it again).
    FileAbsent { path: String },
    /// A folder the operation created (restoring removes it if empty).
    DirAbsent { path: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Backup {
    pub id: String,
    pub title: String,
    pub created: String,
    pub items: Vec<BackupItem>,
    #[serde(default)]
    pub restored: bool,
}

impl Backup {
    pub fn bytes(&self) -> u64 {
        self.items
            .iter()
            .map(|i| {
                if let BackupItem::File { size, .. } = i {
                    *size
                } else {
                    0
                }
            })
            .sum()
    }

    pub fn summary(&self) -> String {
        let (mut files, mut keys, mut values) = (0, 0, 0);
        for i in &self.items {
            match i {
                BackupItem::File { .. } => files += 1,
                BackupItem::RegTree { keys: k } => keys += k.len(),
                BackupItem::RegValue { .. } => values += 1,
                _ => {}
            }
        }
        format!(
            "{files} files ({}), {keys} keys, {values} values",
            crate::model::human_size(self.bytes())
        )
    }
}

/// Result of one step of an operation, shown to the user afterwards.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ActionResult {
    pub description: String,
    pub ok: bool,
    pub message: String,
}

impl ActionResult {
    pub fn ok(desc: impl Into<String>, msg: impl Into<String>) -> Self {
        ActionResult {
            description: desc.into(),
            ok: true,
            message: msg.into(),
        }
    }
    pub fn err(desc: impl Into<String>, msg: impl Into<String>) -> Self {
        ActionResult {
            description: desc.into(),
            ok: false,
            message: msg.into(),
        }
    }
}

/// Collects backup items while an operation runs.
pub struct BackupWriter {
    dir: PathBuf,
    backup: Backup,
    counter: usize,
}

impl BackupWriter {
    pub fn new(backups_root: &Path, title: &str) -> Result<Self> {
        let id = crate::util::new_id();
        let dir = backups_root.join(&id);
        std::fs::create_dir_all(dir.join("files"))
            .with_context(|| format!("create {}", dir.display()))?;
        let w = BackupWriter {
            dir,
            backup: Backup {
                id,
                title: title.to_string(),
                created: crate::util::now_string(),
                ..Default::default()
            },
            counter: 0,
        };
        w.save()?;
        Ok(w)
    }

    pub fn id(&self) -> &str {
        &self.backup.id
    }

    fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.backup)?;
        std::fs::write(self.dir.join("manifest.json"), json)?;
        Ok(())
    }

    pub fn add_file(&mut self, path: &Path) -> Result<()> {
        let meta = std::fs::metadata(path)?;
        self.counter += 1;
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let stored = format!("{:06}{ext}", self.counter);
        std::fs::copy(path, self.dir.join("files").join(&stored))
            .with_context(|| format!("back up {}", path.display()))?;
        self.backup.items.push(BackupItem::File {
            original: path.to_string_lossy().to_string(),
            stored,
            size: meta.len(),
        });
        self.save()
    }

    /// Back up a whole folder: the folder entries and every file inside.
    pub fn add_dir_tree(&mut self, path: &Path) -> Result<()> {
        for e in walkdir::WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if e.file_type().is_dir() {
                self.backup.items.push(BackupItem::Dir {
                    original: e.path().to_string_lossy().to_string(),
                });
            } else if e.file_type().is_file() {
                self.add_file(e.path())?;
            }
        }
        self.save()
    }

    pub fn add_dir(&mut self, path: &Path) -> Result<()> {
        self.backup.items.push(BackupItem::Dir {
            original: path.to_string_lossy().to_string(),
        });
        self.save()
    }

    pub fn add_tree(&mut self, reg: &dyn Registry, path: &str) -> Result<()> {
        let keys = platform::read_tree(reg, path);
        if !keys.is_empty() {
            self.backup.items.push(BackupItem::RegTree { keys });
        }
        self.save()
    }

    pub fn add_value(&mut self, reg: &dyn Registry, key: &str, name: &str) -> Result<()> {
        let previous = reg.read_key(key).and_then(|k| k.get(name).cloned());
        self.backup.items.push(BackupItem::RegValue {
            key: key.into(),
            name: name.into(),
            previous,
        });
        self.save()
    }

    /// Record that the operation is about to create this file.
    pub fn add_file_absent(&mut self, path: &str) {
        self.backup
            .items
            .push(BackupItem::FileAbsent { path: path.into() });
    }

    /// Record that the operation is about to create this folder.
    pub fn add_dir_absent(&mut self, path: &str) {
        self.backup
            .items
            .push(BackupItem::DirAbsent { path: path.into() });
    }

    pub fn add_key_absent(&mut self, path: &str) -> Result<()> {
        self.backup
            .items
            .push(BackupItem::KeyAbsent { path: path.into() });
        self.save()
    }

    pub fn is_empty(&self) -> bool {
        self.backup.items.is_empty()
    }

    pub fn finish(self) -> Result<Backup> {
        self.save()?;
        // Also export the registry part as a standard .reg file, so it can be
        // restored by double-clicking even without Expunge.
        let reg = backup_to_reg(&self.backup);
        if !reg.is_empty() {
            let mut bytes = vec![0xFF, 0xFE];
            reg.encode_utf16()
                .for_each(|u| bytes.extend_from_slice(&u.to_le_bytes()));
            std::fs::write(
                self.dir.join(format!("registry-{}.reg", self.backup.id)),
                bytes,
            )?;
        }
        Ok(self.backup)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Remove an empty backup (nothing needed saving).
    pub fn discard(self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The registry contents of a backup as a `.reg` file that puts everything
/// back the way it was before the operation. Empty if no registry changes.
pub fn backup_to_reg(b: &Backup) -> String {
    use crate::report::{long_path, reg_data, reg_name};
    let mut body = String::new();
    // Restore in reverse order of the operation, like `restore_backup`.
    for item in b.items.iter().rev() {
        match item {
            BackupItem::RegTree { keys } => {
                for k in keys {
                    body.push_str(&format!("[{}]\r\n", long_path(&k.path)));
                    for v in k.values.values() {
                        body.push_str(&format!("{}={}\r\n", reg_name(&v.name), reg_data(v)));
                    }
                    body.push_str("\r\n");
                }
            }
            BackupItem::RegValue {
                key,
                name,
                previous,
            } => {
                body.push_str(&format!("[{}]\r\n", long_path(key)));
                match previous {
                    Some(v) => {
                        body.push_str(&format!("{}={}\r\n\r\n", reg_name(name), reg_data(v)))
                    }
                    None => body.push_str(&format!("{}=-\r\n\r\n", reg_name(name))),
                }
            }
            BackupItem::KeyAbsent { path } => {
                body.push_str(&format!("[-{}]\r\n\r\n", long_path(path)))
            }
            _ => {}
        }
    }
    if body.is_empty() {
        return body;
    }
    format!(
        "Windows Registry Editor Version 5.00\r\n\r\n; Expunge backup {} — {}\r\n; Created {}. Double-click (as Administrator) to restore these registry entries.\r\n\r\n{body}",
        b.id, b.title, b.created
    )
}

pub fn list_backups(root: &Path) -> Vec<Backup> {
    let mut out: Vec<Backup> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("manifest.json")).ok())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

pub fn delete_backup(root: &Path, id: &str) -> Result<()> {
    let dir = root.join(id);
    if dir.join("manifest.json").exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Undo an operation by replaying its backup in reverse order.
pub fn restore_backup(root: &Path, id: &str, reg: &dyn Registry) -> Result<Vec<ActionResult>> {
    let dir = root.join(id);
    let manifest = dir.join("manifest.json");
    let mut backup: Backup = serde_json::from_str(&std::fs::read_to_string(&manifest)?)?;
    let mut results = Vec::new();

    // Folders first so files have somewhere to go.
    for item in &backup.items {
        if let BackupItem::Dir { original } = item {
            let r = std::fs::create_dir_all(original);
            results.push(match r {
                Ok(()) => ActionResult::ok(format!("Folder {original}"), "recreated"),
                Err(e) => ActionResult::err(format!("Folder {original}"), e.to_string()),
            });
        }
    }
    for item in backup.items.iter().rev() {
        let r = match item {
            BackupItem::Dir { .. } => continue,
            BackupItem::File {
                original, stored, ..
            } => {
                let desc = format!("File {original}");
                let dest = Path::new(original);
                let res = dest
                    .parent()
                    .map(std::fs::create_dir_all)
                    .unwrap_or(Ok(()))
                    .and_then(|_| std::fs::copy(dir.join("files").join(stored), dest).map(|_| ()));
                match res {
                    Ok(()) => ActionResult::ok(desc, "restored"),
                    Err(e) => ActionResult::err(desc, e.to_string()),
                }
            }
            BackupItem::RegTree { keys } => {
                let desc = format!(
                    "Key {}",
                    keys.first().map(|k| k.path.as_str()).unwrap_or("?")
                );
                match platform::write_tree(reg, keys) {
                    Ok(()) => ActionResult::ok(desc, format!("restored {} keys", keys.len())),
                    Err(e) => ActionResult::err(desc, e.to_string()),
                }
            }
            BackupItem::RegValue {
                key,
                name,
                previous,
            } => {
                let desc = format!(
                    "Value {key}\\{}",
                    if name.is_empty() { "(Default)" } else { name }
                );
                let res = match previous {
                    Some(v) => reg.set_value(key, v),
                    None => match reg.read_key(key).and_then(|k| k.get(name).cloned()) {
                        Some(_) => reg.delete_value(key, name),
                        None => Ok(()),
                    },
                };
                match res {
                    Ok(()) => ActionResult::ok(desc, "restored"),
                    Err(e) => ActionResult::err(desc, e.to_string()),
                }
            }
            BackupItem::FileAbsent { path } => {
                let desc = format!("File {path}");
                if Path::new(path).exists() {
                    match platform::delete_file(Path::new(path)) {
                        Ok(()) => ActionResult::ok(desc, "removed again"),
                        Err(e) => ActionResult::err(desc, e.to_string()),
                    }
                } else {
                    ActionResult::ok(desc, "already absent")
                }
            }
            BackupItem::DirAbsent { path } => {
                // Items are undone newest first, so its files are gone by now.
                let desc = format!("Folder {path}");
                match std::fs::remove_dir(path) {
                    Ok(()) => ActionResult::ok(desc, "removed again"),
                    Err(_) if !Path::new(path).exists() => ActionResult::ok(desc, "already absent"),
                    Err(e) => ActionResult::err(desc, e.to_string()),
                }
            }
            BackupItem::KeyAbsent { path } => {
                let desc = format!("Key {path}");
                if reg.exists(path) {
                    match reg.delete_tree(path) {
                        Ok(()) => ActionResult::ok(desc, "removed again"),
                        Err(e) => ActionResult::err(desc, e.to_string()),
                    }
                } else {
                    ActionResult::ok(desc, "already absent")
                }
            }
        };
        results.push(r);
    }
    backup.restored = true;
    std::fs::write(&manifest, serde_json::to_string_pretty(&backup)?)?;
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::MemRegistry;

    #[test]
    fn backup_and_restore_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let broot = tmp.path().join("backups");
        let work = tmp.path().join("work");
        std::fs::create_dir_all(work.join("sub")).unwrap();
        std::fs::write(work.join("sub").join("a.txt"), "hello").unwrap();

        let reg = MemRegistry::new();
        reg.put(KeyData::new("HKCU\\Software\\X\\Y").with(RegValue::string("v", "1")));
        reg.put(KeyData::new("HKCU\\Software\\Z").with(RegValue::dword("n", 5)));

        let mut w = BackupWriter::new(&broot, "test").unwrap();
        w.add_dir_tree(&work).unwrap();
        w.add_tree(&reg, "HKCU\\Software\\X").unwrap();
        w.add_value(&reg, "HKCU\\Software\\Z", "n").unwrap();
        w.add_value(&reg, "HKCU\\Software\\Z", "new").unwrap();
        let b = w.finish().unwrap();
        let reg_file =
            std::fs::read(broot.join(&b.id).join(format!("registry-{}.reg", b.id))).unwrap();
        let text = crate::model::decode_utf16z(&reg_file[2..]);
        assert!(text.starts_with("Windows Registry Editor Version 5.00"));
        assert!(text.contains("[HKEY_CURRENT_USER\\Software\\X\\Y]"));
        assert!(text.contains("\"n\"=dword:00000005"));
        assert!(text.contains("\"new\"=-"));

        // Destroy things.
        std::fs::remove_dir_all(&work).unwrap();
        reg.delete_tree("HKCU\\Software\\X").unwrap();
        reg.set_value("HKCU\\Software\\Z", &RegValue::dword("n", 99))
            .unwrap();
        reg.set_value("HKCU\\Software\\Z", &RegValue::dword("new", 1))
            .unwrap();

        let res = restore_backup(&broot, &b.id, &reg).unwrap();
        assert!(res.iter().all(|r| r.ok), "{res:?}");
        assert_eq!(
            std::fs::read_to_string(work.join("sub").join("a.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            reg.read_key("HKCU\\Software\\X\\Y")
                .unwrap()
                .get_string("v")
                .unwrap(),
            "1"
        );
        let z = reg.read_key("HKCU\\Software\\Z").unwrap();
        assert_eq!(z.get_u32("n"), Some(5));
        assert!(z.get("new").is_none());
        assert!(list_backups(&broot)[0].restored);
    }
}
