//! Installer lint: check a recorded installation for common packaging
//! problems. Useful for developers testing their own installers, and for
//! anyone curious how tidy a program's installer is.

use crate::model::{fold, InstallLog, KeyData};
use crate::platform::expand_env;
use crate::snapshot::is_uninstall_entry;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Info => "Info",
            Severity::Warning => "Warning",
            Severity::Error => "Error",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    /// Short English title (also the translation key).
    pub title: &'static str,
    /// Longer English explanation (also the translation key).
    pub advice: &'static str,
    /// Concrete paths or values involved.
    pub items: Vec<String>,
}

fn under(path: &str, var: &str) -> bool {
    let base = expand_env(var);
    if base.contains('%') {
        return false;
    }
    let base = fold(base.trim_end_matches('\\'));
    let p = fold(path);
    p == base || p.starts_with(&format!("{base}\\"))
}

fn find(
    findings: &mut Vec<Finding>,
    code: &'static str,
    severity: Severity,
    title: &'static str,
    advice: &'static str,
    items: Vec<String>,
) {
    if !items.is_empty() {
        findings.push(Finding {
            code,
            severity,
            title,
            advice,
            items,
        });
    }
}

/// Run every check. Results are sorted with the most serious first.
pub fn lint(log: &InstallLog) -> Vec<Finding> {
    let cs = &log.changes;
    let mut out = Vec::new();

    // Uninstall entry.
    let entries: Vec<&KeyData> = cs
        .keys_added
        .iter()
        .filter(|k| is_uninstall_entry(&k.path))
        .collect();
    let updated_entry = cs
        .values_added
        .iter()
        .chain(cs.values_removed.iter())
        .any(|v| is_uninstall_entry(&v.key))
        || cs
            .values_modified
            .iter()
            .any(|v| is_uninstall_entry(&v.key));
    if entries.is_empty() && !updated_entry && !cs.files_added.is_empty() {
        out.push(Finding {
            code: "EXP001",
            severity: Severity::Error,
            title: "No uninstall entry",
            advice: "The installer did not register the program under Apps & features, so users cannot uninstall it the normal way.",
            items: vec![],
        });
    }
    for e in &entries {
        let mut missing = Vec::new();
        for v in [
            "DisplayName",
            "Publisher",
            "DisplayVersion",
            "DisplayIcon",
            "InstallLocation",
            "EstimatedSize",
            "UninstallString",
        ] {
            if e.get(v).is_none() {
                missing.push(v.to_string());
            }
        }
        find(
            &mut out,
            "EXP002",
            if missing.iter().any(|m| m == "UninstallString" || m == "DisplayName") { Severity::Error } else { Severity::Warning },
            "Uninstall entry is incomplete",
            "Apps & features shows less information, or cannot remove the program. Add the missing values to the Uninstall key.",
            missing.into_iter().map(|m| format!("{}  ←  {m}", e.path)).collect(),
        );
        if e.get("QuietUninstallString").is_none() && e.get_u32("WindowsInstaller") != Some(1) {
            find(
                &mut out,
                "EXP003",
                Severity::Info,
                "No silent uninstall command",
                "Add QuietUninstallString so administrators and tools can remove the program without prompts.",
                vec![e.path.clone()],
            );
        }
        // The uninstaller should be something the installer actually put on disk.
        if let Some(cmd) = e.get_string("UninstallString") {
            let (prog, _) = crate::util::split_command_line(&expand_env(&cmd));
            let lower = fold(&prog);
            let is_system = ["msiexec", "rundll32", "cmd.exe", "powershell"]
                .iter()
                .any(|t| lower.contains(t));
            let added = cs.files_added.iter().any(|f| fold(&f.path) == lower);
            if !is_system
                && std::path::Path::new(&prog).is_absolute()
                && !added
                && !std::path::Path::new(&prog).exists()
            {
                find(
                    &mut out,
                    "EXP004",
                    Severity::Error,
                    "Uninstaller file is missing",
                    "UninstallString points to a file that does not exist, so uninstalling will fail.",
                    vec![prog],
                );
            }
        }
    }

    // Files in Windows system folders.
    let sys: Vec<String> = cs
        .files_added
        .iter()
        .filter(|f| {
            under(&f.path, "%SystemRoot%")
                && !under(&f.path, "%SystemRoot%\\System32\\Tasks")
                && !under(&f.path, "%SystemRoot%\\Fonts")
        })
        .map(|f| f.path.clone())
        .collect();
    find(
        &mut out,
        "EXP010",
        Severity::Warning,
        "Files written to the Windows folder",
        "Programs should keep their files in their own folder. Files in System32 or the Windows folder can conflict with other software and are easily left behind.",
        sys,
    );

    // Existing files changed or removed.
    let touched: Vec<String> = cs
        .files_modified
        .iter()
        .map(|f| f.after.path.clone())
        .chain(cs.files_removed.iter().map(|f| f.path.clone()))
        .collect();
    find(
        &mut out,
        "EXP011",
        Severity::Warning,
        "Existing files were changed or deleted",
        "The installer modified or removed files it did not create. These changes cannot be undone automatically.",
        touched,
    );

    // Per-machine and per-user locations mixed.
    let machine = cs.files_added.iter().any(|f| {
        under(&f.path, "%ProgramFiles%")
            || under(&f.path, "%ProgramFiles(x86)%")
            || under(&f.path, "%ProgramData%")
    }) || cs
        .keys_added
        .iter()
        .any(|k| fold(&k.path).starts_with("hklm\\"));
    let user_paths: Vec<String> = cs
        .files_added
        .iter()
        .filter(|f| under(&f.path, "%APPDATA%") || under(&f.path, "%LOCALAPPDATA%"))
        .map(|f| f.path.clone())
        .chain(
            cs.keys_added
                .iter()
                .filter(|k| fold(&k.path).starts_with("hkcu\\"))
                .map(|k| k.path.clone()),
        )
        .take(25)
        .collect();
    if machine {
        find(
            &mut out,
            "EXP012",
            Severity::Info,
            "Per-machine install also wrote per-user data",
            "Data written to the installing user's profile is not available to other users and is often left behind when another user uninstalls.",
            user_paths,
        );
    }

    // PATH and other environment changes.
    let env: Vec<String> = cs
        .values_modified
        .iter()
        .filter(|v| fold(&v.key).ends_with("environment"))
        .map(|v| format!("{} = {}", v.after.name, v.after.display_data()))
        .chain(
            cs.values_added
                .iter()
                .filter(|v| fold(&v.key).ends_with("environment"))
                .map(|v| format!("{} = {}", v.value.name, v.value.display_data())),
        )
        .collect();
    find(
        &mut out,
        "EXP020",
        Severity::Info,
        "Environment variables changed",
        "Changes to PATH or other variables affect every program. Make sure the uninstaller reverts exactly these entries.",
        env,
    );

    // Autostart.
    let run: Vec<String> = cs
        .values_added
        .iter()
        .filter(|v| {
            let k = fold(&v.key);
            k.ends_with("\\currentversion\\run") || k.ends_with("\\currentversion\\runonce")
        })
        .map(|v| format!("{}\\{} = {}", v.key, v.value.name, v.value.display_data()))
        .chain(
            cs.files_added
                .iter()
                .filter(|f| fold(&f.path).contains("\\start menu\\programs\\startup\\"))
                .map(|f| f.path.clone()),
        )
        .collect();
    find(
        &mut out,
        "EXP021",
        Severity::Warning,
        "Starts automatically with Windows",
        "The program added itself to startup. Ask users first, and remove the entry on uninstall.",
        run,
    );

    // Services.
    let services: Vec<String> = cs
        .keys_added
        .iter()
        .filter(|k| {
            let f = fold(&k.path);
            f.strip_prefix("hklm\\system\\currentcontrolset\\services\\")
                .is_some_and(|r| !r.contains('\\'))
        })
        .map(|k| {
            let start = match k.get_u32("Start") {
                Some(2) => "automatic",
                Some(3) => "manual",
                Some(4) => "disabled",
                Some(0) | Some(1) => "boot/system driver",
                _ => "?",
            };
            format!("{} ({start})", crate::platform::leaf_name(&k.path))
        })
        .collect();
    find(
        &mut out,
        "EXP022",
        Severity::Info,
        "Services installed",
        "Services run in the background. Prefer manual or delayed start, and make sure the uninstaller stops and deletes them.",
        services,
    );

    // Scheduled tasks.
    let tasks: Vec<String> = cs
        .files_added
        .iter()
        .filter(|f| !f.is_dir && under(&f.path, "%SystemRoot%\\System32\\Tasks"))
        .map(|f| f.path.clone())
        .collect();
    find(
        &mut out,
        "EXP023",
        Severity::Info,
        "Scheduled tasks created",
        "Often used for updaters. Make sure the uninstaller removes them.",
        tasks,
    );

    // Shell integration.
    let shell: Vec<String> = cs
        .keys_added
        .iter()
        .map(|k| k.path.clone())
        .filter(|p| {
            let f = fold(p);
            f.contains("\\shellex\\")
                || f.contains("\\shell\\")
                || f.contains("\\contextmenuhandlers\\")
                || f.contains("\\shell extensions\\approved")
        })
        .collect();
    find(
        &mut out,
        "EXP024",
        Severity::Info,
        "Explorer integration added",
        "Context-menu entries and shell extensions run inside Explorer; buggy ones can slow it down or crash it.",
        shell,
    );

    // File associations.
    let assoc: Vec<String> = cs
        .keys_added
        .iter()
        .filter(|k| {
            let f = fold(&k.path);
            let rest = f
                .strip_prefix("hklm\\software\\classes\\")
                .or_else(|| f.strip_prefix("hkcu\\software\\classes\\"));
            rest.is_some_and(|r| r.starts_with('.') && !r.contains('\\'))
        })
        .map(|k| crate::platform::leaf_name(&k.path).to_string())
        .collect();
    find(
        &mut out,
        "EXP025",
        Severity::Info,
        "File types registered",
        "The program registered file extensions. Use OpenWithProgids instead of taking over the default handler without asking.",
        assoc,
    );

    // Firewall.
    let fw: Vec<String> = cs
        .values_added
        .iter()
        .filter(|v| fold(&v.key).contains("firewallpolicy\\firewallrules"))
        .map(|v| v.value.display_data())
        .collect();
    find(
        &mut out,
        "EXP026",
        Severity::Info,
        "Firewall rules added",
        "Check that every rule is needed and is removed on uninstall.",
        fw,
    );

    // Shared registry values changed.
    let shared: Vec<String> = cs
        .values_modified
        .iter()
        .filter(|v| !fold(&v.key).ends_with("environment") && !is_uninstall_entry(&v.key))
        .map(|v| format!("{}\\{}", v.key, v.after.display_name()))
        .collect();
    find(
        &mut out,
        "EXP027",
        Severity::Info,
        "Existing registry values changed",
        "These values existed before installation. Expunge restores them on uninstall, but most uninstallers do not.",
        shared,
    );

    // Executables the installer added that aren't properly signed
    // (only checked while the files still exist).
    let mut unsigned = Vec::new();
    let mut broken = Vec::new();
    for f in cs.files_added.iter().filter(|f| !f.is_dir).take(2000) {
        let lower = f.path.to_lowercase();
        if !(lower.ends_with(".exe") || lower.ends_with(".msi") || lower.ends_with(".sys")) {
            continue;
        }
        match crate::trust::check_file(&f.path) {
            crate::trust::Trust::Unsigned => unsigned.push(f.path.clone()),
            crate::trust::Trust::Invalid { reason, .. } => {
                broken.push(format!("{}  ({reason})", f.path))
            }
            _ => {}
        }
    }
    find(
        &mut out,
        "EXP030",
        Severity::Warning,
        "Executables are not digitally signed",
        "Unsigned programs trigger SmartScreen warnings and can't be traced to their publisher. Sign them with a code-signing certificate.",
        unsigned,
    );
    find(
        &mut out,
        "EXP031",
        Severity::Error,
        "Executables have a broken signature",
        "These files are signed, but the signature does not verify. They may have been modified after signing.",
        broken,
    );

    out.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.code.cmp(b.code)));
    out
}

pub fn counts(findings: &[Finding]) -> (usize, usize, usize) {
    let c = |s| findings.iter().filter(|f| f.severity == s).count();
    (c(Severity::Error), c(Severity::Warning), c(Severity::Info))
}

/// Markdown report, e.g. to attach to a bug or pull request.
pub fn to_markdown(log: &InstallLog, findings: &[Finding]) -> String {
    let (e, w, i) = counts(findings);
    let mut s = format!(
        "# Installer lint: {}\n\n{} {} · {}\n\n**{e} errors, {w} warnings, {i} notes**\n\n",
        log.name, log.publisher, log.version, log.created
    );
    for f in findings {
        s.push_str(&format!(
            "## {} {} — {}\n\n{}\n\n",
            f.severity.label(),
            f.code,
            f.title,
            f.advice
        ));
        for it in f.items.iter().take(50) {
            s.push_str(&format!("- `{it}`\n"));
        }
        if f.items.len() > 50 {
            s.push_str(&format!("- … and {} more\n", f.items.len() - 50));
        }
        s.push('\n');
    }
    s.push_str("_Generated by Expunge_\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn finds_common_problems() {
        std::env::set_var("SystemRoot", "C:\\Windows");
        let un = "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Acme";
        let log = InstallLog {
            name: "Acme".into(),
            changes: ChangeSet {
                keys_added: vec![
                    KeyData::new(un)
                        .with(RegValue::string("DisplayName", "Acme"))
                        .with(RegValue::string("UninstallString", "C:\\Acme\\gone.exe")),
                    KeyData::new("HKLM\\SYSTEM\\CurrentControlSet\\Services\\AcmeSvc")
                        .with(RegValue::dword("Start", 2)),
                    KeyData::new("HKLM\\SOFTWARE\\Classes\\.acme"),
                ],
                values_added: vec![ValueRef {
                    key: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
                    value: RegValue::string("Acme", "acme.exe"),
                }],
                files_added: vec![FileEntry {
                    path: "C:\\Windows\\System32\\acme.dll".into(),
                    is_dir: false,
                    size: 1,
                    mtime: 0,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let f = lint(&log);
        let codes: Vec<&str> = f.iter().map(|x| x.code).collect();
        let mut expect = vec!["EXP002", "EXP003", "EXP010", "EXP021", "EXP022", "EXP025"];
        if cfg!(windows) {
            expect.push("EXP004"); // needs Windows-style absolute paths
        }
        for c in expect {
            assert!(codes.contains(&c), "{c} missing from {codes:?}");
        }
        assert!(f[0].severity >= Severity::Warning);
        assert!(to_markdown(&log, &f).contains("EXP010"));

        let empty = InstallLog {
            changes: ChangeSet {
                files_added: vec![FileEntry {
                    path: "C:\\X\\a".into(),
                    is_dir: false,
                    size: 1,
                    mtime: 0,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(lint(&empty)[0].code, "EXP001");
    }
}
