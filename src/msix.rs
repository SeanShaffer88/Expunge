//! MSIX output for a repackaged installation.
//!
//! Turns the `files/<FOLDER>/…` layout written by [`crate::repackage`] into
//! an MSIX package layout (files under `VFS\…`, an `AppxManifest.xml` and
//! logos). When Microsoft's MakeAppx tool from the Windows SDK is installed,
//! the layout is packed into an `.msix` file. The package must still be
//! signed (with SignTool, or the free MSIX Packaging Tool) before Windows
//! installs it, with a certificate whose subject matches the Publisher in the
//! manifest.

use crate::model::InstallLog;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MsixResult {
    /// Folder with AppxManifest.xml and the VFS files.
    pub layout: PathBuf,
    /// The packed .msix, when MakeAppx was found.
    pub package: Option<PathBuf>,
    /// The program file the package starts.
    pub executable: String,
    /// Destination folders MSIX can't hold (left out).
    pub skipped: Vec<String>,
}

/// MSIX virtual folder for a repackage folder variable.
pub fn vfs_folder(var: &str) -> Option<&'static str> {
    Some(match var.to_ascii_lowercase().as_str() {
        "programfiles" | "programw6432" => "ProgramFilesX64",
        "programfiles(x86)" => "ProgramFilesX86",
        "commonprogramfiles" | "commonprogramw6432" => "ProgramFilesCommonX64",
        "commonprogramfiles(x86)" => "ProgramFilesCommonX86",
        "programdata" => "Common AppData",
        "appdata" => "AppData",
        "localappdata" => "LocalAppData",
        _ => return None,
    })
}

/// `1.2.3` → `1.2.3.0`; anything unusable → `1.0.0.0`.
pub fn msix_version(v: &str) -> String {
    let mut parts: Vec<u32> = v
        .split(['.', '-', ' ', '+'])
        .map_while(|p| p.parse::<u32>().ok())
        .take(4)
        .map(|n| n.min(65535))
        .collect();
    if parts.is_empty() {
        parts.push(1);
    }
    while parts.len() < 4 {
        parts.push(0);
    }
    parts.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(".")
}

/// Package identity name: letters, digits, dots and dashes, 3–50 characters.
pub fn identity_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches(['-', '.']).to_string();
    let s = if s.len() < 3 { format!("App-{s}") } else { s };
    s.chars().take(50).collect()
}

/// A publisher DN for the manifest: `CN=<publisher>` with characters that
/// need escaping removed.
pub fn publisher_dn(publisher: &str) -> String {
    let p: String = publisher
        .chars()
        .filter(|c| !",+\"\\<>;=#".contains(*c))
        .collect::<String>()
        .trim()
        .to_string();
    format!("CN={}", if p.is_empty() { "Unknown Publisher" } else { &p })
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn manifest(log: &InstallLog, executable: &str) -> String {
    let name = identity_name(&log.name);
    let display = if log.name.trim().is_empty() { "Application" } else { log.name.trim() };
    let publisher = if log.publisher.trim().is_empty() { "Unknown Publisher" } else { log.publisher.trim() };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
         IgnorableNamespaces="uap rescap">
  <Identity Name="{name}" Publisher="{dn}" Version="{ver}" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>{display}</DisplayName>
    <PublisherDisplayName>{publisher}</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Resources><Resource Language="en-us" /></Resources>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.22000.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
  <Applications>
    <Application Id="App" Executable="{exe}" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="{display}" Description="{display}" BackgroundColor="transparent"
                          Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png" />
    </Application>
  </Applications>
</Package>
"#,
        dn = xml(&publisher_dn(publisher)),
        ver = msix_version(&log.version),
        display = xml(display),
        publisher = xml(publisher),
        exe = xml(executable),
    )
}

/// The program file to start: an .exe that isn't an uninstaller, updater
/// or crash reporter, preferring one named like the program, else the
/// largest. Paths are relative to the layout, with backslashes.
pub fn pick_executable(candidates: &[(String, u64)], program: &str) -> Option<String> {
    let skip = ["unins", "uninst", "setup", "update", "crash", "helper", "elevat", "report"];
    let good: Vec<&(String, u64)> = candidates
        .iter()
        .filter(|(p, _)| p.to_lowercase().ends_with(".exe"))
        .filter(|(p, _)| {
            let f = p.rsplit('\\').next().unwrap_or(p).to_lowercase();
            !skip.iter().any(|s| f.contains(s))
        })
        .collect();
    let want = crate::leftovers::norm(&crate::leftovers::base_name(program));
    if !want.is_empty() {
        if let Some(hit) = good.iter().find(|(p, _)| {
            let f = p.rsplit('\\').next().unwrap_or(p);
            let stem = f.rsplit_once('.').map(|x| x.0).unwrap_or(f);
            let s = crate::leftovers::norm(stem);
            !s.is_empty() && (want.contains(&s) || s.contains(&want))
        }) {
            return Some(hit.0.clone());
        }
    }
    good.iter().max_by_key(|(_, size)| *size).map(|(p, _)| p.clone())
}

fn copy_tree(from: &Path, to: &Path, found: &mut Vec<(String, u64)>, root: &Path) -> Result<()> {
    for e in walkdir::WalkDir::new(from).follow_links(false).into_iter().flatten() {
        let rel = e.path().strip_prefix(from).unwrap_or(e.path());
        let dest = to.join(rel);
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else if e.file_type().is_file() {
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            let n = std::fs::copy(e.path(), &dest)?;
            let in_pkg = dest
                .strip_prefix(root)
                .unwrap_or(&dest)
                .to_string_lossy()
                .replace('/', "\\");
            found.push((in_pkg, n));
        }
    }
    Ok(())
}

fn logo(path: &Path, size: u32) -> Result<()> {
    let mut img = image::RgbaImage::from_pixel(size, size, image::Rgba([0x6B, 0x07, 0x00, 255]));
    // A light frame so the tile is recognizable.
    let b = (size / 12).max(1);
    for y in 0..size {
        for x in 0..size {
            if x < b || y < b || x >= size - b || y >= size - b {
                img.put_pixel(x, y, image::Rgba([0xE0, 0x5A, 0x45, 255]));
            }
        }
    }
    img.save(path)?;
    Ok(())
}

/// Windows SDK MakeAppx.exe, newest version first.
pub fn find_makeappx() -> Option<PathBuf> {
    let base = PathBuf::from(crate::platform::expand_env("%ProgramFiles(x86)%\\Windows Kits\\10\\bin"));
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("x64").join("makeappx.exe").is_file())
        .collect();
    versions.sort();
    versions.pop().map(|v| v.join("x64").join("makeappx.exe"))
}

/// Build the MSIX layout (and package, if possible) next to a repackaged
/// folder written by [`crate::repackage`].
pub fn build(repackaged: &Path, log: &InstallLog) -> Result<MsixResult> {
    let layout = repackaged.join("msix");
    let _ = std::fs::remove_dir_all(&layout);
    std::fs::create_dir_all(layout.join("Assets"))?;
    let mut res = MsixResult {
        layout: layout.clone(),
        ..Default::default()
    };
    let mut files = Vec::new();
    let src = repackaged.join("files");
    for e in std::fs::read_dir(&src).with_context(|| format!("read {}", src.display()))?.flatten() {
        let var = e.file_name().to_string_lossy().to_string();
        match vfs_folder(&var) {
            Some(vfs) => copy_tree(&e.path(), &layout.join("VFS").join(vfs), &mut files, &layout)?,
            None => res.skipped.push(var),
        }
    }
    res.executable = pick_executable(&files, &log.name)
        .ok_or_else(|| anyhow::anyhow!("the installation contains no program file (.exe) to start"))?;
    std::fs::write(layout.join("AppxManifest.xml"), manifest(log, &res.executable))?;
    logo(&layout.join("Assets").join("Square150x150Logo.png"), 150)?;
    logo(&layout.join("Assets").join("Square44x44Logo.png"), 44)?;
    logo(&layout.join("Assets").join("StoreLogo.png"), 50)?;
    if let Some(tool) = find_makeappx() {
        let out = repackaged.join(format!("{}.msix", identity_name(&log.name)));
        let (code, text) = crate::platform::run_hidden(
            &tool.to_string_lossy(),
            &["pack", "/d", &layout.to_string_lossy(), "/p", &out.to_string_lossy(), "/o"],
        )?;
        if code == 0 {
            res.package = Some(out);
        } else {
            anyhow::bail!("MakeAppx couldn't pack the package: {text}");
        }
    }
    std::fs::write(
        repackaged.join("MSIX-README.txt"),
        format!(
            "MSIX package for {name}\r\n\r\n\
msix\\            the package layout (AppxManifest.xml, VFS files, logos).\r\n\
{pkg}\
To install it, sign the package with a certificate whose subject is\r\n\
  {dn}\r\n\
(edit Publisher in AppxManifest.xml to match your certificate), for example:\r\n\
  SignTool sign /fd SHA256 /a /f MyCert.pfx /p <password> \"{file}\"\r\n\
or open the layout in Microsoft's free MSIX Packaging Tool.\r\n\r\n\
Registry entries are not part of this package; install.reg holds them if the\r\n\
program needs them. {skipped}\r\n",
            name = log.name,
            pkg = if res.package.is_some() {
                "*.msix           the packed package (unsigned).\r\n\r\n"
            } else {
                "To pack it, install the Windows SDK and run:\r\n  MakeAppx pack /d msix /p app.msix\r\n\r\n"
            },
            dn = publisher_dn(&log.publisher),
            file = res
                .package
                .as_ref()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                .unwrap_or_else(|| "app.msix".into()),
            skipped = if res.skipped.is_empty() {
                String::new()
            } else {
                format!("Files in these folders can't be packaged and were left out: {}.", res.skipped.join(", "))
            },
        ),
    )?;
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parts() {
        assert_eq!(msix_version("24.08"), "24.8.0.0");
        assert_eq!(msix_version("1.2.3.4.5"), "1.2.3.4");
        assert_eq!(msix_version("beta"), "1.0.0.0");
        assert_eq!(identity_name("Photo Editor (64-bit)"), "Photo-Editor-64-bit");
        assert_eq!(identity_name("Q"), "App-Q");
        assert_eq!(publisher_dn("Acme, Inc."), "CN=Acme Inc.");
        assert_eq!(vfs_folder("ProgramFiles"), Some("ProgramFilesX64"));
        assert_eq!(vfs_folder("USERPROFILE"), None);
        let exes = vec![
            ("VFS\\ProgramFilesX64\\Acme\\unins000.exe".to_string(), 900),
            ("VFS\\ProgramFilesX64\\Acme\\PhotoEditor.exe".to_string(), 500),
            ("VFS\\ProgramFilesX64\\Acme\\big.exe".to_string(), 800),
            ("VFS\\ProgramFilesX64\\Acme\\data.pak".to_string(), 9000),
        ];
        assert_eq!(pick_executable(&exes, "Photo Editor 2.1").unwrap(), "VFS\\ProgramFilesX64\\Acme\\PhotoEditor.exe");
        assert_eq!(pick_executable(&exes, "Something Else").unwrap(), "VFS\\ProgramFilesX64\\Acme\\big.exe");
    }

    #[test]
    fn builds_layout() {
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("files").join("ProgramFiles").join("Acme");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("acme.exe"), b"MZ").unwrap();
        let home = d.path().join("files").join("USERPROFILE");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("x.txt"), b"x").unwrap();
        let log = InstallLog {
            name: "Acme".into(),
            publisher: "Acme Corp".into(),
            version: "2.0".into(),
            ..Default::default()
        };
        let r = build(d.path(), &log).unwrap();
        assert_eq!(r.executable, "VFS\\ProgramFilesX64\\Acme\\acme.exe");
        assert_eq!(r.skipped, vec!["USERPROFILE"]);
        let m = std::fs::read_to_string(r.layout.join("AppxManifest.xml")).unwrap();
        assert!(m.contains("Version=\"2.0.0.0\"") && m.contains("CN=Acme Corp"));
        assert!(r.layout.join("Assets").join("Square44x44Logo.png").is_file());
        assert!(r.layout.join("VFS").join("ProgramFilesX64").join("Acme").join("acme.exe").is_file());
    }
}
