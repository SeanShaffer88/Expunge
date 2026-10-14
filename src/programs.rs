//! Installed programs: the classic uninstaller list (Win32 + Store apps).

use crate::model::KeyData;
use crate::platform::Registry;
use serde::{Deserialize, Serialize};

pub const UNINSTALL_ROOTS: [(&str, Source); 3] = [
    (
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        Source::Machine64,
    ),
    (
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        Source::Machine32,
    ),
    (
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        Source::User,
    ),
];

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Source {
    #[default]
    Machine64,
    Machine32,
    User,
    Store,
}

impl Source {
    pub fn label(&self) -> &'static str {
        match self {
            Source::Machine64 => "64-bit",
            Source::Machine32 => "32-bit",
            Source::User => "Current user",
            Source::Store => "Store app",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct InstalledProgram {
    /// Uninstall registry key (or package full name for Store apps).
    pub key: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// `YYYY-MM-DD` when known.
    pub install_date: String,
    pub install_location: String,
    pub uninstall_string: String,
    pub quiet_uninstall_string: String,
    pub modify_path: String,
    pub display_icon: String,
    pub url: String,
    pub comments: String,
    pub size_kb: u64,
    pub source: Source,
    pub system_component: bool,
    pub is_msi: bool,
    pub product_code: String,
    pub no_remove: bool,
}

impl InstalledProgram {
    pub fn from_key(k: &KeyData, source: Source) -> Option<Self> {
        let name = k.get_string("DisplayName")?;
        let key_leaf = crate::platform::leaf_name(&k.path).to_string();
        let uninstall = k.get_string("UninstallString").unwrap_or_default();
        let is_msi = k.get_u32("WindowsInstaller") == Some(1)
            || uninstall.to_lowercase().contains("msiexec");
        let product_code = if key_leaf.starts_with('{') && key_leaf.ends_with('}') {
            key_leaf.clone()
        } else {
            extract_guid(&uninstall).unwrap_or_default()
        };
        let raw_date = k.get_string("InstallDate").unwrap_or_default();
        Some(InstalledProgram {
            key: k.path.clone(),
            name: name.trim().to_string(),
            publisher: k
                .get_string("Publisher")
                .unwrap_or_default()
                .trim()
                .to_string(),
            version: k.get_string("DisplayVersion").unwrap_or_default(),
            install_date: format_install_date(&raw_date),
            install_location: k
                .get_string("InstallLocation")
                .unwrap_or_default()
                .trim()
                .trim_matches('"')
                .trim_end_matches('\\')
                .to_string(),
            uninstall_string: uninstall,
            quiet_uninstall_string: k.get_string("QuietUninstallString").unwrap_or_default(),
            modify_path: k.get_string("ModifyPath").unwrap_or_default(),
            display_icon: k.get_string("DisplayIcon").unwrap_or_default(),
            url: k
                .get_string("URLInfoAbout")
                .or_else(|| k.get_string("HelpLink"))
                .unwrap_or_default(),
            comments: k.get_string("Comments").unwrap_or_default(),
            size_kb: k.get_u32("EstimatedSize").unwrap_or(0) as u64,
            source,
            system_component: k.get_u32("SystemComponent") == Some(1),
            is_msi,
            product_code,
            no_remove: k.get_u32("NoRemove") == Some(1),
        })
    }

    /// Hidden entries: components, updates, patches.
    pub fn is_hidden(&self, k: &KeyData) -> bool {
        self.system_component
            || k.get_string("ParentKeyName").is_some()
            || k.get_string("ReleaseType").is_some_and(|r| {
                matches!(
                    r.to_lowercase().as_str(),
                    "update" | "hotfix" | "security update"
                )
            })
    }

    /// Command line that removes the program.
    pub fn uninstall_command(&self, quiet: bool) -> Option<String> {
        if self.source == Source::Store {
            return None;
        }
        if quiet && !self.quiet_uninstall_string.is_empty() {
            return Some(self.quiet_uninstall_string.clone());
        }
        if self.is_msi && !self.product_code.is_empty() {
            let q = if quiet { " /passive" } else { "" };
            return Some(format!("MsiExec.exe /X{}{q}", self.product_code));
        }
        if self.uninstall_string.is_empty() {
            None
        } else {
            Some(self.uninstall_string.clone())
        }
    }

    /// The entry points at an uninstaller or folder that no longer exists.
    pub fn is_broken(&self) -> bool {
        if self.source == Source::Store {
            return false;
        }
        if !self.install_location.is_empty()
            && !std::path::Path::new(&self.install_location).exists()
        {
            return true;
        }
        if self.is_msi {
            return false;
        }
        let cmd = crate::platform::expand_env(&self.uninstall_string);
        if cmd.trim().is_empty() {
            return !self.no_remove;
        }
        let (prog, _) = crate::util::split_command_line(&cmd);
        let p = std::path::Path::new(&prog);
        let lower = prog.to_lowercase();
        let system_tool = ["rundll32", "msiexec", "cmd.exe", "powershell"]
            .iter()
            .any(|t| lower.contains(t));
        p.is_absolute() && !system_tool && !p.exists()
    }

    pub fn has_quiet_uninstall(&self) -> bool {
        !self.quiet_uninstall_string.is_empty() || (self.is_msi && !self.product_code.is_empty())
    }
}

/// Total size of the files under a folder, in bytes.
pub fn dir_size(path: &str) -> u64 {
    walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

pub fn extract_guid(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s[start..].find('}')? + start;
    let g = &s[start..=end];
    (g.len() == 38).then(|| g.to_string())
}

fn format_install_date(raw: &str) -> String {
    let d: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if d.len() == 8 {
        format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8])
    } else {
        raw.to_string()
    }
}

/// Enumerate desktop programs from the three Uninstall locations.
pub fn list_programs(reg: &dyn Registry, include_hidden: bool) -> Vec<InstalledProgram> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (root, source) in UNINSTALL_ROOTS {
        for sub in reg.subkeys(root) {
            let path = format!("{root}\\{sub}");
            let Some(k) = reg.read_key(&path) else {
                continue;
            };
            let Some(p) = InstalledProgram::from_key(&k, source) else {
                continue;
            };
            if !include_hidden && p.is_hidden(&k) {
                continue;
            }
            // MSI products sometimes appear in both views.
            let dedup = format!("{}|{}|{}", p.name.to_lowercase(), p.version, p.product_code);
            if !seen.insert(dedup) {
                continue;
            }
            out.push(p);
        }
    }
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

/// Microsoft Store / MSIX packages the user can remove.
pub fn list_store_apps() -> Vec<InstalledProgram> {
    #[cfg(windows)]
    {
        let script = "Get-AppxPackage | Where-Object { -not $_.IsFramework -and -not $_.NonRemovable -and \
                      $_.SignatureKind -ne 'System' } | Select-Object Name,PackageFullName,Publisher,Version,InstallLocation \
                      | ConvertTo-Json -Compress";
        let Ok((_, out)) = crate::platform::run_hidden(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ],
        ) else {
            return Vec::new();
        };
        parse_store_json(&out)
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

pub fn parse_store_json(json: &str) -> Vec<InstalledProgram> {
    let v: serde_json::Value = match serde_json::from_str(json.trim()) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let items = match v {
        serde_json::Value::Array(a) => a,
        o @ serde_json::Value::Object(_) => vec![o],
        _ => return Vec::new(),
    };
    let s = |o: &serde_json::Value, k: &str| {
        o.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
    };
    let mut out: Vec<InstalledProgram> = items
        .iter()
        .map(|o| {
            let publisher = s(o, "Publisher");
            let publisher = publisher
                .split(',')
                .find_map(|p| p.trim().strip_prefix("CN="))
                .unwrap_or(&publisher)
                .to_string();
            InstalledProgram {
                key: s(o, "PackageFullName"),
                name: s(o, "Name"),
                publisher,
                version: s(o, "Version"),
                install_location: s(o, "InstallLocation"),
                source: Source::Store,
                ..Default::default()
            }
        })
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

pub fn remove_store_app(full_name: &str) -> anyhow::Result<String> {
    let safe: String = full_name
        .chars()
        .filter(|c| c.is_alphanumeric() || "._-~".contains(*c))
        .collect();
    let (code, out) = crate::platform::run_hidden(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("Remove-AppxPackage -Package '{safe}'"),
        ],
    )?;
    if code == 0 {
        Ok("removed".into())
    } else {
        anyhow::bail!("Remove-AppxPackage failed: {out}")
    }
}

/// Export the program list as CSV.
pub fn to_csv(programs: &[InstalledProgram]) -> String {
    let esc = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut out =
        String::from("Name,Publisher,Version,Installed,Size KB,Type,Location,Uninstall\r\n");
    for p in programs {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{}\r\n",
            esc(&p.name),
            esc(&p.publisher),
            esc(&p.version),
            esc(&p.install_date),
            p.size_kb,
            esc(p.source.label()),
            esc(&p.install_location),
            esc(&p.uninstall_string)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RegValue;
    use crate::platform::MemRegistry;

    #[test]
    fn lists_and_filters_programs() {
        let reg = MemRegistry::new();
        let root = UNINSTALL_ROOTS[0].0;
        reg.put(
            KeyData::new(&format!("{root}\\Foo"))
                .with(RegValue::string("DisplayName", "Foo"))
                .with(RegValue::string(
                    "UninstallString",
                    "\"C:\\Foo\\unins000.exe\"",
                ))
                .with(RegValue::string("InstallDate", "20260102"))
                .with(RegValue::dword("EstimatedSize", 2048)),
        );
        reg.put(
            KeyData::new(&format!("{root}\\{{12345678-1234-1234-1234-123456789012}}"))
                .with(RegValue::string("DisplayName", "Bar MSI"))
                .with(RegValue::dword("WindowsInstaller", 1))
                .with(RegValue::string(
                    "UninstallString",
                    "MsiExec.exe /I{12345678-1234-1234-1234-123456789012}",
                )),
        );
        reg.put(
            KeyData::new(&format!("{root}\\Hidden"))
                .with(RegValue::string("DisplayName", "Runtime"))
                .with(RegValue::dword("SystemComponent", 1)),
        );
        reg.put(KeyData::new(&format!("{root}\\NoName")));

        let list = list_programs(&reg, false);
        assert_eq!(list.len(), 2);
        let msi = list.iter().find(|p| p.name == "Bar MSI").unwrap();
        assert_eq!(
            msi.uninstall_command(false).unwrap(),
            "MsiExec.exe /X{12345678-1234-1234-1234-123456789012}"
        );
        assert!(msi.has_quiet_uninstall());
        let foo = list.iter().find(|p| p.name == "Foo").unwrap();
        assert_eq!(foo.install_date, "2026-01-02");
        assert_eq!(foo.size_kb, 2048);
        assert_eq!(list_programs(&reg, true).len(), 3);
        assert!(to_csv(&list).contains("\"Bar MSI\""));
    }

    #[test]
    fn store_json() {
        let one = r#"{"Name":"Contoso.App","PackageFullName":"Contoso.App_1.0_x64__abc","Publisher":"CN=Contoso, O=Contoso","Version":"1.0"}"#;
        let l = parse_store_json(one);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].publisher, "Contoso");
        assert_eq!(parse_store_json(&format!("[{one},{one}]")).len(), 2);
    }
}
