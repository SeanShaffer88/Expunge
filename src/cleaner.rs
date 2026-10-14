//! Junk cleaner: temporary files, caches, crash dumps, Recycle Bin.
//!
//! Only well-known, regenerable locations are touched, and only their
//! *contents*: the folders themselves stay. Files in use are skipped.

use crate::platform::expand_env;
use crate::util::glob_match;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Category {
    /// Stable id, also the translation key (`clean.<id>`).
    pub id: &'static str,
    pub patterns: Vec<&'static str>,
    pub needs_admin: bool,
    pub default_on: bool,
}

pub fn categories() -> Vec<Category> {
    let c = |id, patterns: Vec<&'static str>, needs_admin, default_on| Category {
        id,
        patterns,
        needs_admin,
        default_on,
    };
    vec![
        c("user_temp", vec!["%TEMP%"], false, true),
        c("windows_temp", vec!["%SystemRoot%\\Temp"], true, true),
        c(
            "crash_dumps",
            vec![
                "%LOCALAPPDATA%\\CrashDumps",
                "%ProgramData%\\Microsoft\\Windows\\WER\\ReportArchive",
                "%ProgramData%\\Microsoft\\Windows\\WER\\ReportQueue",
                "%LOCALAPPDATA%\\Microsoft\\Windows\\WER\\ReportArchive",
            ],
            false,
            true,
        ),
        c(
            "thumbnails",
            vec!["%LOCALAPPDATA%\\Microsoft\\Windows\\Explorer\\thumbcache_*.db"],
            false,
            false,
        ),
        c(
            "shader_cache",
            vec![
                "%LOCALAPPDATA%\\D3DSCache",
                "%LOCALAPPDATA%\\NVIDIA\\DXCache",
                "%LOCALAPPDATA%\\AMD\\DxCache",
            ],
            false,
            true,
        ),
        c(
            "update_cache",
            vec!["%SystemRoot%\\SoftwareDistribution\\Download"],
            true,
            false,
        ),
        c(
            "chrome_cache",
            vec![
                "%LOCALAPPDATA%\\Google\\Chrome\\User Data\\*\\Cache",
                "%LOCALAPPDATA%\\Google\\Chrome\\User Data\\*\\Code Cache",
                "%LOCALAPPDATA%\\Google\\Chrome\\User Data\\*\\GPUCache",
            ],
            false,
            false,
        ),
        c(
            "edge_cache",
            vec![
                "%LOCALAPPDATA%\\Microsoft\\Edge\\User Data\\*\\Cache",
                "%LOCALAPPDATA%\\Microsoft\\Edge\\User Data\\*\\Code Cache",
                "%LOCALAPPDATA%\\Microsoft\\Edge\\User Data\\*\\GPUCache",
            ],
            false,
            false,
        ),
        c(
            "firefox_cache",
            vec!["%LOCALAPPDATA%\\Mozilla\\Firefox\\Profiles\\*\\cache2"],
            false,
            false,
        ),
        c("recycle_bin", vec![], false, false),
    ]
}

/// Expand env vars and `*` wildcards (one or more path segments may contain them).
pub fn resolve_pattern(pattern: &str) -> Vec<PathBuf> {
    let expanded = expand_env(pattern);
    if expanded.contains('%') {
        return Vec::new();
    }
    // Accept both separators (patterns may mix them, e.g. %TEMP%/x on Windows).
    let mut parts = expanded
        .split(['\\', '/'])
        .filter(|s| !s.is_empty())
        .peekable();
    let mut current: Vec<PathBuf> = vec![];
    let first = parts.next().unwrap_or_default();
    current.push(if expanded.starts_with('/') {
        PathBuf::from(format!("/{first}"))
    } else if expanded.starts_with('\\') {
        PathBuf::from(format!("\\{first}"))
    } else {
        // Drive letter like "C:" needs its root separator.
        PathBuf::from(format!("{first}{}", std::path::MAIN_SEPARATOR))
    });
    for part in parts {
        let mut next = Vec::new();
        for base in &current {
            if part.contains('*') || part.contains('?') {
                if let Ok(rd) = std::fs::read_dir(base) {
                    for e in rd.flatten() {
                        if glob_match(part, &e.file_name().to_string_lossy()) {
                            next.push(e.path());
                        }
                    }
                }
            } else {
                next.push(base.join(part));
            }
        }
        current = next;
    }
    current.into_iter().filter(|p| p.exists()).collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanResult {
    pub files: u64,
    pub bytes: u64,
}

fn walk_files(p: &Path) -> impl Iterator<Item = walkdir::DirEntry> {
    walkdir::WalkDir::new(p)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
}

pub fn scan(cat: &Category) -> ScanResult {
    if cat.id == "recycle_bin" {
        return recycle_bin_size();
    }
    let mut r = ScanResult::default();
    for pat in &cat.patterns {
        for target in resolve_pattern(pat) {
            for e in walk_files(&target) {
                r.files += 1;
                r.bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    r
}

/// Delete the category's contents. Returns what was actually freed.
pub fn clean(cat: &Category) -> ScanResult {
    if cat.id == "recycle_bin" {
        let before = recycle_bin_size();
        return if empty_recycle_bin() {
            before
        } else {
            ScanResult::default()
        };
    }
    let mut freed = ScanResult::default();
    for pat in &cat.patterns {
        for target in resolve_pattern(pat) {
            if target.is_file() {
                let len = target.metadata().map(|m| m.len()).unwrap_or(0);
                if crate::platform::delete_file(&target).is_ok() {
                    freed.files += 1;
                    freed.bytes += len;
                }
                continue;
            }
            for e in walkdir::WalkDir::new(&target)
                .min_depth(1)
                .contents_first(true)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                if e.file_type().is_dir() {
                    let _ = std::fs::remove_dir(e.path());
                } else {
                    let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                    if crate::platform::delete_file(e.path()).is_ok() {
                        freed.files += 1;
                        freed.bytes += len;
                    }
                }
            }
        }
    }
    freed
}

fn recycle_bin_size() -> ScanResult {
    #[cfg(windows)]
    {
        let (files, bytes) = crate::platform::windows::recycle_bin_info();
        ScanResult { files, bytes }
    }
    #[cfg(not(windows))]
    {
        ScanResult::default()
    }
}

fn empty_recycle_bin() -> bool {
    #[cfg(windows)]
    {
        crate::platform::windows::empty_recycle_bin()
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
    fn wildcard_resolution_and_clean() {
        let tmp = tempfile::tempdir().unwrap();
        for p in ["Profile 1/Cache/a", "Default/Cache", "Default/Keep"] {
            std::fs::create_dir_all(tmp.path().join(p)).unwrap();
        }
        std::fs::write(tmp.path().join("Profile 1/Cache/a/f1"), "1234").unwrap();
        std::fs::write(tmp.path().join("Default/Cache/f2"), "12").unwrap();
        std::fs::write(tmp.path().join("Default/Keep/f3"), "1").unwrap();
        std::env::set_var("EXPUNGE_CLEAN_TEST", tmp.path());
        let pat: &'static str = "%EXPUNGE_CLEAN_TEST%/*/Cache";
        let found = resolve_pattern(pat);
        assert_eq!(found.len(), 2, "{found:?}");
        let cat = Category {
            id: "t",
            patterns: vec![pat],
            needs_admin: false,
            default_on: true,
        };
        assert_eq!(scan(&cat), ScanResult { files: 2, bytes: 6 });
        assert_eq!(clean(&cat).bytes, 6);
        assert!(tmp.path().join("Default/Cache").exists());
        assert!(!tmp.path().join("Profile 1/Cache/a").exists());
        assert!(tmp.path().join("Default/Keep/f3").exists());
    }
}
