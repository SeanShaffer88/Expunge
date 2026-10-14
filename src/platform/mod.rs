//! Operating-system access, kept behind small traits so the core logic can be
//! unit-tested anywhere with an in-memory registry.

use crate::model::{fold, KeyData, RegValue};
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[cfg(windows)]
pub mod windows;

/// Minimal registry API used by snapshots, uninstall and repair code.
/// Paths look like `HKLM\SOFTWARE\Vendor` and are case-insensitive.
/// Implementations always use the 64-bit view; the 32-bit view is reached
/// explicitly through `WOW6432Node` paths.
pub trait Registry: Send + Sync {
    fn exists(&self, path: &str) -> bool;
    /// The key's own values, or `None` if it can't be opened.
    fn read_key(&self, path: &str) -> Option<KeyData>;
    /// Names (not full paths) of the key's direct subkeys.
    fn subkeys(&self, path: &str) -> Vec<String>;
    fn create_key(&self, path: &str) -> Result<()>;
    fn set_value(&self, key: &str, value: &RegValue) -> Result<()>;
    fn delete_value(&self, key: &str, name: &str) -> Result<()>;
    /// Delete a key and everything beneath it.
    fn delete_tree(&self, path: &str) -> Result<()>;
}

/// Canonical short hive names.
pub const HIVES: [(&str, &str); 5] = [
    ("HKLM", "HKEY_LOCAL_MACHINE"),
    ("HKCU", "HKEY_CURRENT_USER"),
    ("HKU", "HKEY_USERS"),
    ("HKCR", "HKEY_CLASSES_ROOT"),
    ("HKCC", "HKEY_CURRENT_CONFIG"),
];

/// Split `HKLM\SOFTWARE\X` into (`HKLM`, `SOFTWARE\X`). Long hive names are
/// normalised to their short form.
pub fn split_hive(path: &str) -> Option<(&'static str, &str)> {
    let (head, rest) = match path.split_once('\\') {
        Some((h, r)) => (h, r),
        None => (path, ""),
    };
    let h = head.to_ascii_uppercase();
    HIVES
        .iter()
        .find(|(short, long)| h == *short || h == *long)
        .map(|(short, _)| (*short, rest.trim_matches('\\')))
}

/// Normalise a registry path to short-hive form with single separators.
pub fn normalize_reg_path(path: &str) -> String {
    let p = path.trim().trim_matches('\\');
    match split_hive(p) {
        Some((h, "")) => h.to_string(),
        Some((h, rest)) => {
            let parts: Vec<&str> = rest.split('\\').filter(|s| !s.is_empty()).collect();
            format!("{h}\\{}", parts.join("\\"))
        }
        None => p.to_string(),
    }
}

pub fn parent_path(path: &str) -> Option<&str> {
    path.rsplit_once('\\').map(|(p, _)| p)
}

pub fn leaf_name(path: &str) -> &str {
    path.rsplit_once('\\').map(|(_, l)| l).unwrap_or(path)
}

/// In-memory registry used by tests and by non-Windows development builds.
#[derive(Default)]
pub struct MemRegistry {
    keys: Mutex<BTreeMap<String, KeyData>>,
}

impl MemRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a key, creating any missing parents.
    pub fn put(&self, key: KeyData) {
        let mut keys = self.keys.lock().unwrap();
        let mut p = key.path.as_str();
        while let Some(parent) = parent_path(p) {
            keys.entry(fold(parent))
                .or_insert_with(|| KeyData::new(parent));
            p = parent;
        }
        keys.insert(fold(&key.path), key);
    }

    pub fn len(&self) -> usize {
        self.keys.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Registry for MemRegistry {
    fn exists(&self, path: &str) -> bool {
        split_hive(path).map(|(_, r)| r.is_empty()).unwrap_or(false)
            || self.keys.lock().unwrap().contains_key(&fold(path))
    }

    fn read_key(&self, path: &str) -> Option<KeyData> {
        let keys = self.keys.lock().unwrap();
        keys.get(&fold(path)).cloned().or_else(|| {
            // Hive roots always exist.
            match split_hive(path) {
                Some((h, "")) => Some(KeyData::new(h)),
                _ => None,
            }
        })
    }

    fn subkeys(&self, path: &str) -> Vec<String> {
        let prefix = format!("{}\\", fold(path));
        let keys = self.keys.lock().unwrap();
        keys.range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .filter(|(k, _)| !k[prefix.len()..].contains('\\'))
            .map(|(_, v)| leaf_name(&v.path).to_string())
            .collect()
    }

    fn create_key(&self, path: &str) -> Result<()> {
        if !self.exists(path) {
            self.put(KeyData::new(path));
        }
        Ok(())
    }

    fn set_value(&self, key: &str, value: &RegValue) -> Result<()> {
        self.create_key(key)?;
        let mut keys = self.keys.lock().unwrap();
        let k = keys
            .get_mut(&fold(key))
            .ok_or_else(|| anyhow!("key missing"))?;
        k.values.insert(fold(&value.name), value.clone());
        Ok(())
    }

    fn delete_value(&self, key: &str, name: &str) -> Result<()> {
        let mut keys = self.keys.lock().unwrap();
        let k = keys
            .get_mut(&fold(key))
            .ok_or_else(|| anyhow!("key not found: {key}"))?;
        k.values
            .remove(&fold(name))
            .ok_or_else(|| anyhow!("value not found: {name}"))?;
        Ok(())
    }

    fn delete_tree(&self, path: &str) -> Result<()> {
        let f = fold(path);
        let prefix = format!("{f}\\");
        let mut keys = self.keys.lock().unwrap();
        if !keys.contains_key(&f) {
            return Err(anyhow!("key not found: {path}"));
        }
        keys.retain(|k, _| k != &f && !k.starts_with(&prefix));
        Ok(())
    }
}

/// Read a key and all of its descendants (depth-first, parents first).
pub fn read_tree(reg: &dyn Registry, path: &str) -> Vec<KeyData> {
    let mut out = Vec::new();
    let mut stack = vec![path.to_string()];
    while let Some(p) = stack.pop() {
        if let Some(k) = reg.read_key(&p) {
            let mut subs = reg.subkeys(&p);
            subs.sort();
            for s in subs.into_iter().rev() {
                stack.push(format!("{p}\\{s}"));
            }
            out.push(k);
        }
    }
    out
}

/// Recreate keys and values previously captured with [`read_tree`].
pub fn write_tree(reg: &dyn Registry, keys: &[KeyData]) -> Result<()> {
    for k in keys {
        reg.create_key(&k.path)?;
        for v in k.values.values() {
            reg.set_value(&k.path, v)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// File-system helpers
// ---------------------------------------------------------------------------

/// Delete a file, clearing the read-only attribute first if needed.
pub fn delete_file(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            if let Ok(meta) = std::fs::metadata(path) {
                let mut perm = meta.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                perm.set_readonly(false);
                let _ = std::fs::set_permissions(path, perm);
            }
            std::fs::remove_file(path)
        }
        Err(e) => Err(e),
    }
}

/// Move a file or folder to the Recycle Bin.
pub fn recycle(path: &std::path::Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows::recycle(path)
    }
    #[cfg(not(windows))]
    {
        Err(anyhow!(
            "the Recycle Bin is only available on Windows: {}",
            path.display()
        ))
    }
}

/// Ask Windows to delete a locked file at the next reboot.
pub fn delete_on_reboot(path: &std::path::Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows::delete_on_reboot(path)
    }
    #[cfg(not(windows))]
    {
        Err(anyhow!(
            "delete-on-reboot is only available on Windows: {}",
            path.display()
        ))
    }
}

/// Expand `%VAR%` references using the process environment. Unknown
/// variables are left untouched.
pub fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Run a console program without showing a window; returns exit code and
/// combined output.
pub fn run_hidden(program: &str, args: &[&str]) -> Result<(i32, String)> {
    let mut c = std::process::Command::new(program);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    let out = c
        .output()
        .map_err(|e| anyhow!("could not run {program}: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code().unwrap_or(-1), text.trim().to_string()))
}

pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

pub fn user_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".into())
}

/// The live system registry on Windows; an empty in-memory one elsewhere.
pub fn system_registry() -> Box<dyn Registry> {
    #[cfg(windows)]
    {
        Box::new(windows::WinRegistry)
    }
    #[cfg(not(windows))]
    {
        let r = MemRegistry::new();
        if std::env::var("EXPUNGE_DEMO").is_ok() {
            demo_programs(&r);
        }
        Box::new(r)
    }
}

/// Sample programs for UI development on non-Windows machines.
#[cfg(not(windows))]
fn demo_programs(r: &MemRegistry) {
    let root = "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    let apps = [
        (
            "7-Zip 24.08 (x64)",
            "Igor Pavlov",
            "24.08",
            "20260611",
            5_800,
        ),
        (
            "Audacity 3.7.1",
            "Audacity Team",
            "3.7.1",
            "20260702",
            98_000,
        ),
        ("Discord", "Discord Inc.", "1.0.9172", "20260915", 0),
        (
            "Git",
            "The Git Development Community",
            "2.47.0",
            "20260820",
            340_000,
        ),
        ("GIMP 3.0.2", "The GIMP Team", "3.0.2", "20260530", 410_000),
        (
            "Mozilla Firefox (x64 en-US)",
            "Mozilla",
            "131.0.3",
            "20261008",
            255_000,
        ),
        (
            "Notepad++ (64-bit x64)",
            "Notepad++ Team",
            "8.7",
            "20260901",
            15_000,
        ),
        ("OBS Studio", "OBS Project", "31.0.0", "20260722", 420_000),
        (
            "Old Toolbar Helper",
            "Unknown Corp",
            "1.2",
            "20190101",
            1_200,
        ),
        (
            "Python 3.13.0 (64-bit)",
            "Python Software Foundation",
            "3.13.0",
            "20261001",
            110_000,
        ),
        ("Steam", "Valve Corporation", "2.10.91.91", "20260415", 0),
        (
            "VLC media player",
            "VideoLAN",
            "3.0.21",
            "20260610",
            165_000,
        ),
        (
            "Visual Studio Code",
            "Microsoft Corporation",
            "1.94.2",
            "20261010",
            380_000,
        ),
    ];
    for (i, (name, publisher, ver, date, kb)) in apps.iter().enumerate() {
        let mut k = KeyData::new(&format!("{root}\\App{i}"))
            .with(RegValue::string("DisplayName", name))
            .with(RegValue::string("Publisher", publisher))
            .with(RegValue::string("DisplayVersion", ver))
            .with(RegValue::string("InstallDate", date))
            .with(RegValue::string(
                "UninstallString",
                &format!("C:\\Program Files\\App{i}\\uninstall.exe"),
            ));
        if name.starts_with("Old") {
            k = k.with(RegValue::string(
                "InstallLocation",
                "/nonexistent/OldToolbar",
            ));
        }
        if *kb > 0 {
            k = k.with(RegValue::dword("EstimatedSize", *kb));
        }
        r.put(k);
    }
}

#[cfg(windows)]
pub use windows::{spawn_command_line, ChildProcess};

/// A started process that can be polled for completion.
#[cfg(not(windows))]
pub struct ChildProcess(Mutex<std::process::Child>);

#[cfg(not(windows))]
impl ChildProcess {
    pub fn try_wait(&self) -> Option<u32> {
        self.0
            .lock()
            .unwrap()
            .try_wait()
            .ok()
            .flatten()
            .map(|s| s.code().unwrap_or(0) as u32)
    }
    pub fn wait(&self) -> u32 {
        self.0
            .lock()
            .unwrap()
            .wait()
            .map(|s| s.code().unwrap_or(0) as u32)
            .unwrap_or(0)
    }
}

/// Run a raw command line (development stand-in for the Windows version).
#[cfg(not(windows))]
pub fn spawn_command_line(cmdline: &str) -> Result<ChildProcess> {
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmdline)
        .spawn()?;
    Ok(ChildProcess(Mutex::new(child)))
}

/// Open a file, folder or URL with its default handler.
pub fn shell_open(target: &str) {
    #[cfg(windows)]
    windows::shell_open(target);
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

/// Show a file selected in Explorer (or open its folder).
pub fn reveal(path: &str) {
    #[cfg(windows)]
    {
        if std::path::Path::new(path).is_file() {
            let _ =
                windows::shell_execute_wait("explorer.exe", &format!("/select,\"{path}\""), false);
            return;
        }
    }
    shell_open(path);
}

pub fn open_in_regedit(path: &str) -> Result<()> {
    #[cfg(windows)]
    {
        windows::open_in_regedit(path)
    }
    #[cfg(not(windows))]
    {
        Err(anyhow!(
            "Registry Editor is only available on Windows ({path})"
        ))
    }
}

/// Restart elevated; true if the new instance started.
pub fn relaunch_elevated() -> bool {
    #[cfg(windows)]
    {
        windows::relaunch_elevated()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn apply_window_style(border_rgb: (u8, u8, u8), dark: bool) {
    #[cfg(windows)]
    windows::apply_window_style(border_rgb, dark);
    #[cfg(not(windows))]
    let _ = (border_rgb, dark);
}

pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        windows::is_elevated()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hive_parsing() {
        assert_eq!(
            split_hive("HKEY_LOCAL_MACHINE\\Software"),
            Some(("HKLM", "Software"))
        );
        assert_eq!(
            normalize_reg_path("hkcu\\\\Software\\X\\"),
            "HKCU\\Software\\X"
        );
        assert_eq!(split_hive("Nope\\x"), None);
    }

    #[test]
    fn mem_registry_tree() {
        let r = MemRegistry::new();
        r.put(KeyData::new("HKLM\\SOFTWARE\\A\\B").with(RegValue::dword("x", 1)));
        assert!(r.exists("hklm\\software\\a"));
        assert_eq!(r.subkeys("HKLM\\SOFTWARE"), vec!["A".to_string()]);
        let t = read_tree(&r, "HKLM\\SOFTWARE\\A");
        assert_eq!(t.len(), 2);
        r.delete_tree("HKLM\\SOFTWARE\\A").unwrap();
        assert!(!r.exists("HKLM\\SOFTWARE\\A\\B"));
        write_tree(&r, &t).unwrap();
        assert_eq!(
            r.read_key("HKLM\\SOFTWARE\\A\\B").unwrap().get_u32("x"),
            Some(1)
        );
    }

    #[test]
    fn env_expansion() {
        std::env::set_var("EXPUNGE_TEST_VAR", "C:\\X");
        assert_eq!(expand_env("%EXPUNGE_TEST_VAR%\\y"), "C:\\X\\y");
        assert_eq!(expand_env("%NOPE_NOT_SET%\\y"), "%NOPE_NOT_SET%\\y");
        assert_eq!(expand_env("100%"), "100%");
    }
}
