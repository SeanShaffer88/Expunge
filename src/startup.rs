//! Startup manager: Run keys, Startup folders, services and scheduled tasks.
//!
//! Enabling/disabling Run and Startup-folder entries uses the same
//! `StartupApproved` flags as Task Manager, so both tools always agree.

use crate::backup::BackupWriter;
use crate::model::{fold, RegValue};
use crate::platform::{self, expand_env, Registry};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

const APPROVED: &str = "Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum StartupSource {
    /// A value in a Run/RunOnce key.
    Registry { key: String },
    /// A file in a Startup folder.
    Folder { dir: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StartupEntry {
    pub name: String,
    pub command: String,
    pub location: String,
    pub source: StartupSource,
    pub enabled: bool,
    /// `StartupApproved` key and value name, if this entry can be toggled.
    pub approved: Option<(String, String)>,
    pub all_users: bool,
    /// The target program could not be found (a broken entry).
    pub missing: bool,
}

struct RunLoc {
    key: &'static str,
    approved: Option<&'static str>,
    all_users: bool,
}

const RUN_LOCATIONS: &[RunLoc] = &[
    RunLoc {
        key: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
        approved: Some("HKCU\\Software\\{A}\\Run"),
        all_users: false,
    },
    RunLoc {
        key: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
        approved: None,
        all_users: false,
    },
    RunLoc {
        key: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run",
        approved: Some("HKLM\\SOFTWARE\\{A}\\Run"),
        all_users: true,
    },
    RunLoc {
        key: "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run",
        approved: Some("HKLM\\SOFTWARE\\{A}\\Run32"),
        all_users: true,
    },
    RunLoc {
        key: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
        approved: None,
        all_users: true,
    },
];

fn approved_enabled(reg: &dyn Registry, key: &str, name: &str) -> bool {
    match reg.read_key(key).and_then(|k| k.get(name).cloned()) {
        Some(v) => v.data.first().map(|b| b & 1 == 0).unwrap_or(true),
        None => true,
    }
}

/// Best-effort check that the program a command line starts exists.
fn target_missing(command: &str) -> bool {
    let (prog, _) = crate::util::split_command_line(&expand_env(command));
    let p = Path::new(&prog);
    if !p.is_absolute() {
        return false; // relies on PATH; don't guess
    }
    !p.exists()
}

/// Startup entries that keep Windows, security software or hardware working
/// (Windows Security, audio, touchpad, storage drivers, anti-virus).
/// Simple mode does not let these be disabled or deleted.
pub fn is_essential(e: &StartupEntry) -> bool {
    let cmd = fold(&expand_env(&e.command));
    let sysroot = fold(&expand_env("%SystemRoot%"));
    if !sysroot.contains('%') && cmd.trim_start_matches('"').starts_with(&sysroot) {
        return true;
    }
    let hay = format!("{} {}", fold(&e.name), cmd);
    const KNOWN: &[&str] = &[
        "securityhealth",
        "windows defender",
        "windowsdefender",
        "msmpeng",
        "rtkaud",
        "rthdvcpl",
        "realtek",
        "maxxaudio",
        "nahimic",
        "syntp",
        "synaptics",
        "etdctrl",
        "igfx",
        "iastor",
        "intel rst",
        "bthtray",
        "bluetooth",
        "\\avast",
        "\\avg\\",
        "norton",
        "mcafee",
        "kaspersky",
        "bitdefender",
        "\\eset\\",
        "malwarebytes",
    ];
    KNOWN.iter().any(|k| hay.contains(k))
}

pub fn startup_folders() -> Vec<(String, String, bool)> {
    vec![
        (
            expand_env("%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup"),
            format!("HKCU\\Software\\{APPROVED}\\StartupFolder"),
            false,
        ),
        (
            expand_env("%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup"),
            format!("HKLM\\SOFTWARE\\{APPROVED}\\StartupFolder"),
            true,
        ),
    ]
}

pub fn list_startup(reg: &dyn Registry) -> Vec<StartupEntry> {
    list_startup_with(reg, &startup_folders())
}

pub fn list_startup_with(
    reg: &dyn Registry,
    folders: &[(String, String, bool)],
) -> Vec<StartupEntry> {
    let mut out = Vec::new();
    for loc in RUN_LOCATIONS {
        let Some(k) = reg.read_key(loc.key) else {
            continue;
        };
        let approved_key = loc.approved.map(|a| a.replace("{A}", APPROVED));
        for v in k.values.values() {
            let command = v.as_string().unwrap_or_default();
            let enabled = approved_key
                .as_ref()
                .map(|ak| approved_enabled(reg, ak, &v.name))
                .unwrap_or(true);
            out.push(StartupEntry {
                name: v.name.clone(),
                missing: target_missing(&command),
                command,
                location: loc.key.to_string(),
                source: StartupSource::Registry {
                    key: loc.key.to_string(),
                },
                enabled,
                approved: approved_key.clone().map(|ak| (ak, v.name.clone())),
                all_users: loc.all_users,
            });
        }
    }
    for (dir, approved_key, all_users) in folders {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if fold(&name) == "desktop.ini" || !e.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            out.push(StartupEntry {
                command: e.path().to_string_lossy().to_string(),
                location: dir.clone(),
                source: StartupSource::Folder { dir: dir.clone() },
                enabled: approved_enabled(reg, approved_key, &name),
                approved: Some((approved_key.clone(), name.clone())),
                all_users: *all_users,
                missing: false,
                name,
            });
        }
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// Enable or disable an entry via its `StartupApproved` flag.
pub fn set_enabled(
    reg: &dyn Registry,
    entry: &StartupEntry,
    enabled: bool,
    backup: Option<&mut BackupWriter>,
) -> Result<()> {
    let Some((key, name)) = &entry.approved else {
        anyhow::bail!("this entry can only be deleted, not disabled");
    };
    if let Some(b) = backup {
        if !reg.exists(key) {
            b.add_key_absent(key)?;
        }
        b.add_value(reg, key, name)?;
    }
    let mut data = vec![0u8; 12];
    data[0] = if enabled { 2 } else { 3 };
    if !enabled {
        // FILETIME of when it was disabled (shown by Task Manager).
        let secs = crate::time::unix_now() + 11_644_473_600;
        let ft = (secs as u64) * 10_000_000;
        data[4..12].copy_from_slice(&ft.to_le_bytes());
    }
    reg.set_value(
        key,
        &RegValue {
            name: name.clone(),
            kind: crate::model::reg_type::BINARY,
            data,
        },
    )
}

/// Delete an entry entirely (backed up first).
pub fn delete_entry(
    reg: &dyn Registry,
    entry: &StartupEntry,
    mut backup: Option<&mut BackupWriter>,
) -> Result<()> {
    match &entry.source {
        StartupSource::Registry { key } => {
            if let Some(b) = backup.as_deref_mut() {
                b.add_value(reg, key, &entry.name)?;
            }
            reg.delete_value(key, &entry.name)?;
        }
        StartupSource::Folder { .. } => {
            let p = Path::new(&entry.command);
            if let Some(b) = backup.as_deref_mut() {
                b.add_file(p)?;
            }
            platform::delete_file(p)?;
        }
    }
    if let Some((key, name)) = &entry.approved {
        if reg.read_key(key).is_some_and(|k| k.get(name).is_some()) {
            if let Some(b) = backup {
                b.add_value(reg, key, name)?;
            }
            reg.delete_value(key, name)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ServiceEntry {
    pub name: String,
    pub display_name: String,
    pub image_path: String,
    /// 0 boot, 1 system, 2 automatic, 3 manual, 4 disabled.
    pub start: u32,
    pub delayed: bool,
    /// Ships with Windows (lives under the Windows folder).
    pub windows: bool,
    pub is_driver: bool,
}

impl ServiceEntry {
    pub fn start_label(&self) -> &'static str {
        match (self.start, self.delayed) {
            (0, _) => "boot",
            (1, _) => "system",
            (2, true) => "auto_delayed",
            (2, false) => "automatic",
            (3, _) => "manual",
            (4, _) => "disabled",
            _ => "unknown",
        }
    }
}

const SERVICES: &str = "HKLM\\SYSTEM\\CurrentControlSet\\Services";

pub fn list_services(reg: &dyn Registry) -> Vec<ServiceEntry> {
    let sysroot = fold(&expand_env("%SystemRoot%"));
    let mut out = Vec::new();
    for name in reg.subkeys(SERVICES) {
        let Some(k) = reg.read_key(&format!("{SERVICES}\\{name}")) else {
            continue;
        };
        let Some(ty) = k.get_u32("Type") else {
            continue;
        };
        let Some(start) = k.get_u32("Start") else {
            continue;
        };
        let image = k.get_string("ImagePath").unwrap_or_default();
        let fi = fold(&expand_env(&image));
        let windows = fi.is_empty()
            || fi.contains(&sysroot)
            || fi.starts_with("system32\\")
            || fi.starts_with("\\systemroot\\")
            || fi.contains("\\windows\\");
        let mut display = k.get_string("DisplayName").unwrap_or_else(|| name.clone());
        if display.starts_with('@') {
            display = name.clone(); // indirect resource string
        }
        out.push(ServiceEntry {
            display_name: display,
            image_path: image,
            start,
            delayed: k.get_u32("DelayedAutostart") == Some(1),
            windows,
            is_driver: ty & 0x3 != 0 && ty & 0x30 == 0,
            name,
        });
    }
    out.sort_by_key(|s| s.display_name.to_lowercase());
    out
}

/// Change a service's start type (takes effect at next boot / next start).
pub fn set_service_start(
    reg: &dyn Registry,
    name: &str,
    start: u32,
    delayed: bool,
    backup: Option<&mut BackupWriter>,
) -> Result<()> {
    let key = format!("{SERVICES}\\{name}");
    if let Some(b) = backup {
        b.add_value(reg, &key, "Start")?;
        b.add_value(reg, &key, "DelayedAutostart")?;
    }
    reg.set_value(&key, &RegValue::dword("Start", start))?;
    if start == 2 {
        reg.set_value(&key, &RegValue::dword("DelayedAutostart", delayed as u32))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scheduled tasks
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TaskEntry {
    /// Task path like `\Vendor\Updater`.
    pub name: String,
    pub file: String,
    pub command: String,
    pub author: String,
    pub enabled: bool,
    pub microsoft: bool,
}

fn decode_task_xml(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        crate::model::decode_utf16z(&bytes[2..])
    } else {
        String::from_utf8_lossy(bytes)
            .trim_start_matches('\u{feff}')
            .to_string()
    }
}

fn xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(xml_unescape(xml[start..end].trim()))
}

fn xml_unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub fn parse_task(name: &str, file: &str, bytes: &[u8]) -> TaskEntry {
    let xml = decode_task_xml(bytes);
    let settings = xml.split("<Settings>").nth(1).unwrap_or("");
    let enabled = xml_tag(settings, "Enabled")
        .map(|v| v != "false")
        .unwrap_or(true);
    let mut command = xml_tag(&xml, "Command").unwrap_or_default();
    if let Some(a) = xml_tag(&xml, "Arguments") {
        command = format!("{command} {a}");
    }
    if command.is_empty() && xml.contains("<ComHandler>") {
        command = "(COM handler)".into();
    }
    TaskEntry {
        name: name.to_string(),
        file: file.to_string(),
        command,
        author: xml_tag(&xml, "Author").unwrap_or_default(),
        enabled,
        microsoft: fold(name).starts_with("\\microsoft\\"),
    }
}

pub fn list_tasks() -> Vec<TaskEntry> {
    list_tasks_in(&expand_env("%SystemRoot%\\System32\\Tasks"))
}

pub fn list_tasks_in(root: &str) -> Vec<TaskEntry> {
    let mut out = Vec::new();
    for e in crate::fswalk::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .flatten()
    {
        if !e.file_type().is_file() {
            continue;
        }
        let file = e.path().to_string_lossy().to_string();
        let rel = file[root.len()..].replace('/', "\\");
        let Ok(bytes) = std::fs::read(e.path()) else {
            continue;
        };
        out.push(parse_task(&rel, &file, &bytes));
    }
    out.sort_by_key(|t| t.name.to_lowercase());
    out
}

pub fn set_task_enabled(name: &str, enabled: bool) -> Result<()> {
    let flag = if enabled { "/ENABLE" } else { "/DISABLE" };
    let (code, out) = platform::run_hidden("schtasks.exe", &["/Change", "/TN", name, flag])?;
    if code != 0 {
        anyhow::bail!("schtasks failed: {out}");
    }
    Ok(())
}

pub fn delete_task(task: &TaskEntry, backup: Option<&mut BackupWriter>) -> Result<()> {
    if let Some(b) = backup {
        b.add_file(Path::new(&task.file))?;
    }
    let (code, out) = platform::run_hidden("schtasks.exe", &["/Delete", "/TN", &task.name, "/F"])?;
    if code != 0 {
        anyhow::bail!("schtasks failed: {out}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn essential_entries() {
        let e = |name: &str, command: &str| StartupEntry {
            name: name.into(),
            command: command.into(),
            location: String::new(),
            source: StartupSource::Registry { key: String::new() },
            enabled: true,
            approved: None,
            all_users: true,
            missing: false,
        };
        assert!(is_essential(&e(
            "SecurityHealth",
            "%windir%\\system32\\SecurityHealthSystray.exe"
        )));
        assert!(is_essential(&e(
            "RtkAudUService",
            "\"C:\\Program Files\\Realtek\\Audio\\RtkAudUService64.exe\""
        )));
        assert!(is_essential(&e(
            "ESET",
            "\"C:\\Program Files\\ESET\\ESET Security\\ecmds.exe\""
        )));
        assert!(!is_essential(&e(
            "Spotify",
            "\"C:\\Users\\a\\AppData\\Roaming\\Spotify\\Spotify.exe\" /background"
        )));
        assert!(!is_essential(&e("Reset helper", "C:\\Tools\\reset.exe")));
    }
    use crate::model::KeyData;
    use crate::platform::MemRegistry;

    #[test]
    fn run_entries_toggle_like_task_manager() {
        let tmp = crate::util::tempdir().unwrap();
        let folder = tmp.path().join("Startup");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("App.lnk"), "x").unwrap();
        let reg = MemRegistry::new();
        reg.put(KeyData::new(RUN_LOCATIONS[0].key).with(RegValue::string("Tray", "tray.exe")));
        let fk = format!("HKCU\\Software\\{APPROVED}\\StartupFolder");
        let folders = vec![(folder.to_string_lossy().to_string(), fk.clone(), false)];

        let list = list_startup_with(&reg, &folders);
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|e| e.enabled));
        let tray = list.iter().find(|e| e.name == "Tray").unwrap();
        set_enabled(&reg, tray, false, None).unwrap();
        let list = list_startup_with(&reg, &folders);
        assert!(!list.iter().find(|e| e.name == "Tray").unwrap().enabled);

        let app = list.iter().find(|e| e.name == "App.lnk").unwrap().clone();
        set_enabled(&reg, &app, false, None).unwrap();
        assert!(
            !list_startup_with(&reg, &folders)
                .iter()
                .find(|e| e.name == "App.lnk")
                .unwrap()
                .enabled
        );
        delete_entry(&reg, &app, None).unwrap();
        assert!(!folder.join("App.lnk").exists());
        assert!(reg.read_key(&fk).unwrap().get("App.lnk").is_none());
    }

    #[test]
    fn parses_task_xml() {
        let xml = "<?xml version=\"1.0\"?><Task><RegistrationInfo><Author>Acme</Author></RegistrationInfo>\
                   <Settings><Enabled>false</Enabled></Settings><Actions><Exec><Command>C:\\a.exe</Command>\
                   <Arguments>/x &amp; y</Arguments></Exec></Actions></Task>";
        let t = parse_task("\\Acme\\Up", "f", xml.as_bytes());
        assert!(!t.enabled);
        assert_eq!(t.command, "C:\\a.exe /x & y");
        assert_eq!(t.author, "Acme");
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(crate::model::encode_utf16z(xml));
        assert_eq!(parse_task("\\Microsoft\\X", "f", &utf16).command, t.command);
    }

    #[test]
    fn services() {
        let reg = MemRegistry::new();
        reg.put(
            KeyData::new(&format!("{SERVICES}\\AcmeSvc"))
                .with(RegValue::dword("Type", 16))
                .with(RegValue::dword("Start", 2))
                .with(RegValue::string(
                    "ImagePath",
                    "\"C:\\Program Files\\Acme\\svc.exe\"",
                ))
                .with(RegValue::string("DisplayName", "Acme Service")),
        );
        let l = list_services(&reg);
        assert_eq!(l.len(), 1);
        assert!(!l[0].windows);
        set_service_start(&reg, "AcmeSvc", 4, false, None).unwrap();
        assert_eq!(list_services(&reg)[0].start_label(), "disabled");
    }
}
