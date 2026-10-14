//! Program trust: is a program's executable digitally signed, by whom, and
//! is the signature intact? Uses Windows' own Authenticode verification
//! (offline: no revocation lookups, so checks are fast and private).

use crate::platform::{expand_env, Registry};
use crate::programs::{InstalledProgram, Source};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Trust {
    /// Valid signature from a trusted publisher.
    Signed { signer: String },
    /// No signature at all.
    Unsigned,
    /// Signed, but the signature fails: tampered file, untrusted or revoked
    /// certificate, etc. `signer` is shown when it can still be read.
    Invalid { signer: String, reason: String },
    /// The file to check doesn't exist.
    Missing,
    /// Not checked (non-Windows build, Store app, nothing to check).
    Unknown,
}

impl Trust {
    pub fn signer(&self) -> Option<&str> {
        match self {
            Trust::Signed { signer } => Some(signer),
            Trust::Invalid { signer, .. } if !signer.is_empty() => Some(signer),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Trust::Signed { .. } => "Verified signature",
            Trust::Unsigned => "Not signed",
            Trust::Invalid { .. } => "Signature problem",
            Trust::Missing => "File not found",
            Trust::Unknown => "Not checked",
        }
    }
}

/// Map a WinVerifyTrust result to a human-readable reason.
pub fn describe_error(code: i32) -> &'static str {
    match code as u32 {
        0x800B_0100 => "No signature",
        0x8009_6010 => "The file was modified after it was signed",
        0x800B_0101 => "The signing certificate has expired",
        0x800B_010C => "The signing certificate was revoked",
        0x800B_0109 => "The certificate is not trusted by Windows",
        0x800B_010A => "The certificate chain is incomplete",
        0x800B_0111 => "The publisher is explicitly distrusted",
        0x800B_0004 => "The publisher is not trusted",
        0x8009_6019 => "The certificate is not valid for code signing",
        0x8009_2003 => "The file could not be read",
        _ => "The signature could not be verified",
    }
}

/// Classify the result of an Authenticode check.
pub fn classify(code: i32, signer: Option<String>) -> Trust {
    let signer_name = signer.clone().unwrap_or_default();
    match code as u32 {
        0 => Trust::Signed {
            signer: if signer_name.is_empty() {
                "Unknown publisher".into()
            } else {
                signer_name
            },
        },
        0x800B_0100 => Trust::Unsigned,
        0x8009_2003 if signer.is_none() => Trust::Unsigned,
        _ => Trust::Invalid {
            signer: signer_name,
            reason: describe_error(code).into(),
        },
    }
}

/// Convert a product code GUID to Windows Installer's "packed" form used in
/// its registry keys: {12345678-ABCD-EF01-2345-6789ABCDEF01} →
/// 87654321DCBA10FE32547698BADCFE10.
pub fn packed_guid(guid: &str) -> Option<String> {
    let g: String = guid
        .trim()
        .trim_matches(|c| c == '{' || c == '}')
        .chars()
        .filter(|c| *c != '-')
        .collect();
    if g.len() != 32 || !g.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let g = g.to_uppercase();
    let rev = |s: &str| s.chars().rev().collect::<String>();
    let mut out = String::new();
    out.push_str(&rev(&g[0..8]));
    out.push_str(&rev(&g[8..12]));
    out.push_str(&rev(&g[12..16]));
    // The last 16 hex digits are reversed pairwise (byte order kept, nibbles swapped).
    for i in (16..32).step_by(2) {
        out.push_str(&rev(&g[i..i + 2]));
    }
    Some(out)
}

/// The file whose signature best represents the program: its main executable
/// (from DisplayIcon), the cached MSI package, or its uninstaller.
pub fn trust_target(p: &InstalledProgram, reg: &dyn Registry) -> Option<String> {
    if p.source == Source::Store {
        return None;
    }
    if let Some((path, _)) = crate::programs::parse_icon_location(&p.display_icon) {
        if path.to_lowercase().ends_with(".exe") {
            return Some(path);
        }
    }
    if p.is_msi {
        let packed = packed_guid(&p.product_code)?;
        let key = format!(
            "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Installer\\UserData\\S-1-5-18\\Products\\{packed}\\InstallProperties"
        );
        return reg
            .read_key(&key)
            .and_then(|k| k.get_string("LocalPackage"));
    }
    p.icon_source()
        .map(|(path, _)| path)
        .filter(|s| !s.to_lowercase().ends_with(".ico"))
}

/// Check a file's signature.
pub fn check_file(path: &str) -> Trust {
    let path = expand_env(path);
    if !std::path::Path::new(&path).is_file() {
        return Trust::Missing;
    }
    #[cfg(windows)]
    {
        let (code, signer) = crate::platform::windows::authenticode(&path);
        classify(code, signer)
    }
    #[cfg(not(windows))]
    {
        Trust::Unknown
    }
}

pub fn check_program(p: &InstalledProgram, reg: &dyn Registry) -> Trust {
    match trust_target(p, reg) {
        Some(path) => check_file(&path),
        None => Trust::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KeyData, RegValue};
    use crate::platform::MemRegistry;

    #[test]
    fn packs_guids_like_windows_installer() {
        // Known pair from the Windows Installer documentation format.
        assert_eq!(
            packed_guid("{90150000-008C-0000-0000-0000000FF1CE}").unwrap(),
            "00005109C80000000000000000F01FEC"
        );
        assert!(packed_guid("not-a-guid").is_none());
    }

    #[test]
    fn classifies_results() {
        assert_eq!(
            classify(0, Some("Contoso Ltd".into())),
            Trust::Signed {
                signer: "Contoso Ltd".into()
            }
        );
        assert_eq!(classify(0x800B0100u32 as i32, None), Trust::Unsigned);
        match classify(0x80096010u32 as i32, Some("Contoso".into())) {
            Trust::Invalid { signer, reason } => {
                assert_eq!(signer, "Contoso");
                assert!(reason.contains("modified"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(Trust::Unsigned.signer(), None);
    }

    #[test]
    fn picks_the_right_file() {
        let reg = MemRegistry::new();
        let exe = InstalledProgram {
            display_icon: "\"C:\\App\\app.exe\",0".into(),
            ..Default::default()
        };
        assert_eq!(trust_target(&exe, &reg).unwrap(), "C:\\App\\app.exe");
        let code = "{90150000-008C-0000-0000-0000000FF1CE}";
        reg.put(
            KeyData::new(
                "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Installer\\UserData\\S-1-5-18\\Products\\00005109C80000000000000000F01FEC\\InstallProperties",
            )
            .with(RegValue::string("LocalPackage", "C:\\Windows\\Installer\\1a2b.msi")),
        );
        let msi = InstalledProgram {
            is_msi: true,
            product_code: code.into(),
            ..Default::default()
        };
        assert_eq!(
            trust_target(&msi, &reg).unwrap(),
            "C:\\Windows\\Installer\\1a2b.msi"
        );
        let store = InstalledProgram {
            source: Source::Store,
            ..Default::default()
        };
        assert!(trust_target(&store, &reg).is_none());
        assert_eq!(check_file("/definitely/not/here.exe"), Trust::Missing);
    }
}
