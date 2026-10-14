//! Installed Windows updates, and removing individual ones.
//!
//! Listing combines DISM's package list (what can actually be removed on
//! Windows 11) with the hotfix list (which carries the KB numbers). Removal
//! uses DISM for packages, falling back to `wusa.exe` for plain hotfixes.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveMethod {
    Dism,
    Wusa,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UpdateEntry {
    /// `KB5031234`, when known.
    pub kb: String,
    pub description: String,
    /// DISM package identity (empty for hotfix-only entries).
    pub package: String,
    pub release_type: String,
    /// `YYYY-MM-DD`.
    pub installed: String,
    /// Microsoft marks these as non-removable (servicing stack).
    pub permanent: bool,
    pub method: RemoveMethod,
}

impl UpdateEntry {
    pub fn title(&self) -> String {
        if !self.kb.is_empty() {
            self.kb.clone()
        } else {
            self.package
                .split('~')
                .next()
                .unwrap_or(&self.package)
                .replace("Package_for_", "")
        }
    }
}

const LIST_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$out = @()
foreach ($p in (Get-WindowsPackage -Online)) {
  if ($p.PackageState -ne 'Installed') { continue }
  $rt = "$($p.ReleaseType)"
  if ($rt -notin @('Update','SecurityUpdate','Hotfix','CriticalUpdate','ServicePack')) { continue }
  $d = ''; if ($p.InstallTime) { $d = $p.InstallTime.ToString('yyyy-MM-dd') }
  $out += [pscustomobject]@{ src='pkg'; id=$p.PackageName; type=$rt; date=$d; desc='' }
}
foreach ($h in (Get-HotFix)) {
  $d = ''; if ($h.InstalledOn) { $d = ([datetime]$h.InstalledOn).ToString('yyyy-MM-dd') }
  $out += [pscustomobject]@{ src='hotfix'; id=$h.HotFixID; type=''; date=$d; desc=$h.Description }
}
$out | ConvertTo-Json -Compress
"#;

#[derive(Deserialize)]
struct RawItem {
    src: String,
    id: String,
    #[serde(default, rename = "type")]
    ty: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    desc: String,
}

fn kb_in(s: &str) -> Option<String> {
    let up = s.to_uppercase();
    let i = up.find("KB")?;
    let digits: String = up[i + 2..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (digits.len() >= 6).then(|| format!("KB{digits}"))
}

/// Merge the raw PowerShell output into one list (newest first).
pub fn merge(json: &str) -> Vec<UpdateEntry> {
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap_or(serde_json::Value::Null);
    let raw: Vec<RawItem> = match v {
        serde_json::Value::Array(a) => a
            .into_iter()
            .filter_map(|x| serde_json::from_value(x).ok())
            .collect(),
        o @ serde_json::Value::Object(_) => serde_json::from_value(o).ok().into_iter().collect(),
        _ => Vec::new(),
    };
    let hotfixes: Vec<&RawItem> = raw.iter().filter(|r| r.src == "hotfix").collect();
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in raw.iter().filter(|r| r.src == "pkg") {
        let lower = p.id.to_lowercase();
        let permanent = lower.contains("servicingstack") || lower.starts_with("package_for_ssu");
        let mut kb = kb_in(&p.id).unwrap_or_default();
        let mut desc = String::new();
        if kb.is_empty() && !permanent {
            // Cumulative updates ("RollupFix") don't carry the KB in their
            // name; pair them with the hotfix installed the same day.
            if let Some(h) = hotfixes
                .iter()
                .find(|h| !h.date.is_empty() && h.date == p.date && !used.contains(&h.id))
            {
                kb = h.id.clone();
                desc = h.desc.clone();
            }
        } else if let Some(h) = hotfixes.iter().find(|h| h.id.eq_ignore_ascii_case(&kb)) {
            desc = h.desc.clone();
        }
        if !kb.is_empty() {
            used.insert(kb.clone());
        }
        out.push(UpdateEntry {
            kb,
            description: desc,
            package: p.id.clone(),
            release_type: p.ty.clone(),
            installed: p.date.clone(),
            permanent,
            method: RemoveMethod::Dism,
        });
    }
    for h in hotfixes {
        if used.contains(&h.id) {
            continue;
        }
        out.push(UpdateEntry {
            kb: h.id.clone(),
            description: h.desc.clone(),
            package: String::new(),
            release_type: String::new(),
            installed: h.date.clone(),
            permanent: false,
            method: RemoveMethod::Wusa,
        });
    }
    out.sort_by(|a, b| b.installed.cmp(&a.installed).then(a.kb.cmp(&b.kb)));
    out
}

/// Query installed updates (slow: runs PowerShell; needs admin for the full list).
pub fn list_updates() -> anyhow::Result<Vec<UpdateEntry>> {
    let (_, out) = crate::platform::run_hidden(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            LIST_SCRIPT,
        ],
    )?;
    Ok(merge(&out))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveOutcome {
    Removed,
    RestartRequired,
}

/// Remove one update. Blocks until DISM/WUSA finishes (can take minutes).
pub fn remove_update(u: &UpdateEntry) -> anyhow::Result<RemoveOutcome> {
    if u.permanent {
        anyhow::bail!("Microsoft marks this update as permanent; it cannot be removed");
    }
    let (code, out) = match u.method {
        RemoveMethod::Dism => {
            let arg = format!("/PackageName:{}", u.package);
            crate::platform::run_hidden(
                "dism.exe",
                &["/Online", "/Remove-Package", &arg, "/Quiet", "/NoRestart"],
            )?
        }
        RemoveMethod::Wusa => {
            let kb: String = u.kb.chars().filter(|c| c.is_ascii_digit()).collect();
            let arg = format!("/kb:{kb}");
            crate::platform::run_hidden("wusa.exe", &["/uninstall", &arg, "/quiet", "/norestart"])?
        }
    };
    match code {
        0 => Ok(RemoveOutcome::Removed),
        3010 | 1641 => Ok(RemoveOutcome::RestartRequired),
        740 => anyhow::bail!("administrator rights are required"),
        _ => anyhow::bail!("removal failed (code {code}): {out}"),
    }
}

/// Create a System Restore point (admin; Windows may throttle to one per day).
pub fn create_restore_point(description: &str) -> anyhow::Result<()> {
    let safe: String = description
        .chars()
        .filter(|c| c.is_alphanumeric() || " -_.".contains(*c))
        .collect();
    let script = format!(
        "Checkpoint-Computer -Description '{safe}' -RestorePointType 'APPLICATION_UNINSTALL'"
    );
    let (code, out) = crate::platform::run_hidden(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )?;
    if code != 0 {
        anyhow::bail!("restore point failed: {out}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_packages_and_hotfixes() {
        let json = r#"[
          {"src":"pkg","id":"Package_for_RollupFix~31bf3856ad364e35~amd64~~26100.2033.1.10","type":"SecurityUpdate","date":"2026-09-10","desc":""},
          {"src":"pkg","id":"Package_for_KB5043080~31bf3856ad364e35~amd64~~26100.1.1.0","type":"Update","date":"2026-08-01","desc":""},
          {"src":"pkg","id":"Package_for_ServicingStack_2030~31bf3856ad364e35~amd64~~26100.2030.1.0","type":"Update","date":"2026-09-10","desc":""},
          {"src":"hotfix","id":"KB5044284","type":"","date":"2026-09-10","desc":"Security Update"},
          {"src":"hotfix","id":"KB5043080","type":"","date":"2026-08-01","desc":"Update"},
          {"src":"hotfix","id":"KB5000001","type":"","date":"2026-01-05","desc":"Update"}
        ]"#;
        let l = merge(json);
        assert_eq!(l.len(), 4, "{l:#?}");
        let rollup = l.iter().find(|u| u.package.contains("RollupFix")).unwrap();
        assert_eq!(rollup.kb, "KB5044284");
        assert_eq!(rollup.method, RemoveMethod::Dism);
        assert!(
            l.iter()
                .find(|u| u.package.contains("ServicingStack"))
                .unwrap()
                .permanent
        );
        let wusa = l.iter().find(|u| u.kb == "KB5000001").unwrap();
        assert_eq!(wusa.method, RemoveMethod::Wusa);
        assert_eq!(l[0].installed, "2026-09-10");
        assert!(merge("").is_empty());
    }
}
