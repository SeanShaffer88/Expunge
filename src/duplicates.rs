//! Duplicate file finder.
//!
//! Files are grouped by size first; only files that share a size are read.
//! A quick hash of the first and last 64 KB rules out most candidates, and a
//! full SHA-256 confirms the rest, so two files are only reported as
//! duplicates when their contents are identical.
//!
//! Windows, program and app-data folders are never searched, and cloud-only
//! files (OneDrive placeholders) are skipped so they are not downloaded.

use crate::snapshot::Progress;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Bytes read from each end of a file for the quick comparison.
const EDGE: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct DupFile {
    pub path: PathBuf,
    /// Seconds since the Unix epoch.
    pub modified: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// Size of each copy in bytes.
    pub size: u64,
    pub hash: String,
    pub files: Vec<DupFile>,
}

impl Group {
    /// Space freed by keeping just one copy.
    pub fn wasted(&self) -> u64 {
        self.size * (self.files.len().saturating_sub(1)) as u64
    }
}

#[derive(Debug, Default)]
pub struct DupResult {
    /// Largest waste first.
    pub groups: Vec<Group>,
    pub files_scanned: u64,
    pub cancelled: bool,
}

impl DupResult {
    pub fn wasted(&self) -> u64 {
        self.groups.iter().map(|g| g.wasted()).sum()
    }
}

/// Which copy to keep when selecting automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Oldest,
    Newest,
    ShortestPath,
}

/// The index of the copy to keep in a group.
pub fn keeper(g: &Group, keep: Keep) -> usize {
    let idx = 0..g.files.len();
    match keep {
        Keep::Oldest => idx.min_by_key(|&i| (g.files[i].modified, g.files[i].path.clone())),
        Keep::Newest => idx.max_by_key(|&i| {
            (
                g.files[i].modified,
                std::cmp::Reverse(g.files[i].path.clone()),
            )
        }),
        Keep::ShortestPath => idx.min_by_key(|&i| {
            let p = g.files[i].path.to_string_lossy().to_string();
            (p.chars().count(), p)
        }),
    }
    .unwrap_or(0)
}

/// Folders never searched: Windows itself, installed programs, app data,
/// and system bookkeeping folders.
pub fn excluded_roots() -> Vec<String> {
    let mut v = Vec::new();
    for var in [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
    ] {
        if let Some(p) = std::env::var_os(var) {
            v.push(normalize(&PathBuf::from(p)));
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        v.push(normalize(&PathBuf::from(home).join("AppData")));
    }
    v
}

/// Folder names skipped wherever they appear.
const SKIP_NAMES: &[&str] = &[
    "$recycle.bin",
    "system volume information",
    "windows.old",
    "$windows.~bt",
    "$windows.~ws",
    "node_modules",
    ".git",
];

fn normalize(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    s.trim_end_matches('\\').to_string()
}

fn is_excluded(path: &Path, excluded: &[String]) -> bool {
    let n = normalize(path);
    excluded
        .iter()
        .any(|e| n == *e || (n.starts_with(e.as_str()) && n[e.len()..].starts_with('\\')))
}

/// Cloud-only placeholder or offline file: reading it would download it.
#[cfg(windows)]
fn is_cloud_only(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x40000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x400000;
    meta.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

#[cfg(not(windows))]
fn is_cloud_only(_meta: &std::fs::Metadata) -> bool {
    false
}

fn modified(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Find duplicate files of at least `min_size` bytes under `roots`.
pub fn find(roots: &[PathBuf], min_size: u64, progress: &Progress) -> DupResult {
    find_with(roots, min_size, &excluded_roots(), progress)
}

pub fn find_with(
    roots: &[PathBuf],
    min_size: u64,
    excluded: &[String],
    progress: &Progress,
) -> DupResult {
    progress.set_phase("list");
    let min_size = min_size.max(1);
    let mut by_size: HashMap<u64, Vec<DupFile>> = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    let mut scanned = 0u64;
    for root in roots {
        let walker = walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if !e.file_type().is_dir() {
                    return true;
                }
                let name = e.file_name().to_string_lossy().to_lowercase();
                !SKIP_NAMES.contains(&name.as_str()) && !is_excluded(e.path(), excluded)
            });
        for e in walker.flatten() {
            if progress.cancelled() {
                return DupResult {
                    cancelled: true,
                    ..Default::default()
                };
            }
            if !e.file_type().is_file() {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.len() < min_size || is_cloud_only(&meta) {
                continue;
            }
            // Overlapping locations must not report a file as its own copy.
            if !seen.insert(normalize(e.path())) {
                continue;
            }
            scanned += 1;
            progress.tick();
            by_size.entry(meta.len()).or_default().push(DupFile {
                path: e.into_path(),
                modified: modified(&meta),
            });
        }
    }

    // Only sizes shared by two or more files need reading.
    let candidates: Vec<(u64, Vec<DupFile>)> =
        by_size.into_iter().filter(|(_, v)| v.len() > 1).collect();
    progress.set_phase("compare");
    progress.items.store(0, Ordering::Relaxed);
    let next = AtomicUsize::new(0);
    let groups = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 6);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((size, files)) = candidates.get(i) else {
                    break;
                };
                if progress.cancelled() {
                    break;
                }
                let found = compare(*size, files, progress);
                groups.lock().unwrap().extend(found);
            });
        }
    });
    let mut groups = groups.into_inner().unwrap();
    for g in &mut groups {
        g.files.sort_by(|a, b| a.path.cmp(&b.path));
    }
    groups.sort_by(|a, b| {
        b.wasted()
            .cmp(&a.wasted())
            .then_with(|| a.files[0].path.cmp(&b.files[0].path))
    });
    DupResult {
        groups,
        files_scanned: scanned,
        cancelled: progress.cancelled(),
    }
}

/// Split same-size files into groups of identical content.
fn compare(size: u64, files: &[DupFile], progress: &Progress) -> Vec<Group> {
    let mut quick: HashMap<[u8; 32], Vec<&DupFile>> = HashMap::new();
    for f in files {
        if let Ok(h) = edge_hash(&f.path, size) {
            quick.entry(h).or_default().push(f);
        }
        progress.tick();
    }
    let mut out = Vec::new();
    for (qh, same) in quick {
        if same.len() < 2 {
            continue;
        }
        // Small files were read completely by the quick hash.
        let mut full: HashMap<[u8; 32], Vec<&DupFile>> = HashMap::new();
        if size <= 2 * EDGE {
            full.insert(qh, same);
        } else {
            for f in same {
                if progress.cancelled() {
                    return out;
                }
                if let Ok(h) = full_hash(&f.path) {
                    full.entry(h).or_default().push(f);
                }
            }
        }
        for (h, fs) in full {
            if fs.len() > 1 {
                out.push(Group {
                    size,
                    hash: hex(&h),
                    files: fs.into_iter().cloned().collect(),
                });
            }
        }
    }
    out
}

fn edge_hash(path: &Path, size: u64) -> std::io::Result<[u8; 32]> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    if size <= 2 * EDGE {
        let mut buf = Vec::with_capacity(size as usize);
        f.read_to_end(&mut buf)?;
        h.update(&buf);
    } else {
        let mut buf = vec![0u8; EDGE as usize];
        f.read_exact(&mut buf)?;
        h.update(&buf);
        f.seek(SeekFrom::End(-(EDGE as i64)))?;
        f.read_exact(&mut buf)?;
        h.update(&buf);
    }
    Ok(h.finalize().into())
}

fn full_hash(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Checks before removing `remove` from a group: at least one copy stays,
/// and every copy still has the size it had when it was found.
pub fn check_removal(g: &Group, remove: &[usize]) -> Result<(), String> {
    if remove.iter().any(|&i| i >= g.files.len()) {
        return Err("unknown file".into());
    }
    let kept: Vec<usize> = (0..g.files.len()).filter(|i| !remove.contains(i)).collect();
    if kept.is_empty() {
        return Err("at least one copy must be kept".into());
    }
    let kept_ok = kept.iter().any(|&i| {
        std::fs::metadata(&g.files[i].path)
            .map(|m| m.len() == g.size)
            .unwrap_or(false)
    });
    if !kept_ok {
        return Err(format!(
            "the copy being kept ({}) has changed or is gone",
            g.files[kept[0]].path.display()
        ));
    }
    for &i in remove {
        match std::fs::metadata(&g.files[i].path) {
            Ok(m) if m.len() == g.size => {}
            _ => {
                return Err(format!(
                    "{} has changed since the scan",
                    g.files[i].path.display()
                ))
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("expunge-dups-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_identical_files_only() {
        let d = tmp("find");
        std::fs::create_dir_all(d.join("a/sub")).unwrap();
        std::fs::create_dir_all(d.join("b")).unwrap();
        std::fs::create_dir_all(d.join("node_modules")).unwrap();
        std::fs::create_dir_all(d.join("skipme")).unwrap();
        let big: Vec<u8> = (0..400_000u32).map(|i| (i % 251) as u8).collect();
        let mut big2 = big.clone();
        big2[200_000] ^= 1; // same size and same ends, different middle
        std::fs::write(d.join("a/photo.jpg"), &big).unwrap();
        std::fs::write(d.join("b/photo copy.jpg"), &big).unwrap();
        std::fs::write(d.join("a/sub/photo (1).jpg"), &big).unwrap();
        std::fs::write(d.join("b/edited.jpg"), &big2).unwrap();
        std::fs::write(d.join("node_modules/photo.jpg"), &big).unwrap();
        std::fs::write(d.join("skipme/photo.jpg"), &big).unwrap();
        std::fs::write(d.join("a/note.txt"), b"same").unwrap();
        std::fs::write(d.join("b/note.txt"), b"same").unwrap();
        std::fs::write(d.join("b/tiny.txt"), b"x").unwrap();

        let excluded = vec![normalize(&d.join("skipme"))];
        let p = Progress::default();
        // Overlapping roots must not double-count.
        let r = find_with(&[d.clone(), d.join("a")], 1, &excluded, &p);
        assert_eq!(r.groups.len(), 2, "{:#?}", r.groups);
        let g = &r.groups[0];
        assert_eq!(g.size, 400_000);
        assert_eq!(g.files.len(), 3);
        assert_eq!(g.wasted(), 800_000);
        assert!(g
            .files
            .iter()
            .all(|f| !f.path.to_string_lossy().contains("edited")));
        assert_eq!(r.groups[1].files.len(), 2);
        assert_eq!(r.wasted(), 800_000 + 4);

        // Minimum size filters out the small pair.
        let r = find_with(&[d.clone()], 1000, &excluded, &p);
        assert_eq!(r.groups.len(), 1);

        // Keeping one copy is enforced.
        assert!(check_removal(g, &[0, 1, 2]).is_err());
        assert!(check_removal(g, &[1, 2]).is_ok());
        std::fs::write(&g.files[0].path, b"changed").unwrap();
        assert!(check_removal(g, &[1, 2]).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn keeper_rules() {
        let g = Group {
            size: 1,
            hash: String::new(),
            files: vec![
                DupFile {
                    path: "C:\\x\\long folder name\\a.jpg".into(),
                    modified: 50,
                },
                DupFile {
                    path: "C:\\x\\a.jpg".into(),
                    modified: 90,
                },
                DupFile {
                    path: "C:\\x\\old\\a.jpg".into(),
                    modified: 10,
                },
            ],
        };
        assert_eq!(keeper(&g, Keep::Oldest), 2);
        assert_eq!(keeper(&g, Keep::Newest), 1);
        assert_eq!(keeper(&g, Keep::ShortestPath), 1);
    }

    #[test]
    fn exclusion_matches_whole_folders() {
        let ex = vec!["c:\\windows".to_string()];
        assert!(is_excluded(Path::new("C:\\Windows"), &ex));
        assert!(is_excluded(Path::new("C:\\Windows\\System32"), &ex));
        assert!(!is_excluded(Path::new("C:\\WindowsApps-backup"), &ex));
    }
}
