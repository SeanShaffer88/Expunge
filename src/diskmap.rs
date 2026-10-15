//! Disk space map: a size tree of a folder or drive, and the squarified
//! treemap layout used to draw it.
//!
//! The tree stays small even for a whole drive: inside every folder only the
//! largest files are kept as separate entries (the rest are summed into one
//! "smaller files" entry), and folders under [`DETAIL_MIN`] keep their total
//! but drop their contents.

use crate::snapshot::Progress;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Files kept individually per folder.
const FILES_PER_DIR: usize = 40;
/// Smallest file shown as its own entry.
const FILE_MIN: u64 = 256 * 1024;
/// Folders smaller than this keep only their total.
pub const DETAIL_MIN: u64 = 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub name: String,
    /// Bytes, including everything below.
    pub size: u64,
    /// Number of files, including everything below.
    pub files: u64,
    pub kind: Kind,
    /// Largest first.
    pub children: Vec<Node>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Dir,
    File,
    /// Several small files summed together.
    Others,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    /// Follow child indices down from this node.
    pub fn at(&self, path: &[usize]) -> Option<&Node> {
        let mut n = self;
        for &i in path {
            n = n.children.get(i)?;
        }
        Some(n)
    }

    /// File extension, lowercase, for colouring.
    pub fn ext(&self) -> &str {
        if self.kind != Kind::File {
            return "";
        }
        match self.name.rfind('.') {
            Some(i) if i > 0 => &self.name[i + 1..],
            _ => "",
        }
    }
}

/// Result of a scan.
#[derive(Debug, Default)]
pub struct DiskMap {
    pub root: Node,
    pub root_path: PathBuf,
    /// Folders that could not be read (access denied and similar).
    pub unreadable: u64,
    pub cancelled: bool,
}

impl DiskMap {
    /// Full path of the node at `path`.
    pub fn path_of(&self, path: &[usize]) -> PathBuf {
        let mut p = self.root_path.clone();
        let mut n = &self.root;
        for &i in path {
            let Some(c) = n.children.get(i) else { break };
            if c.kind == Kind::Others {
                break;
            }
            p.push(&c.name);
            n = c;
        }
        p
    }
}

/// Scan `root`. Top-level folders are scanned in parallel; links and
/// junctions are not followed.
pub fn scan(root: &Path, progress: &Progress) -> DiskMap {
    let unreadable = AtomicUsize::new(0);
    let mut top = Node {
        name: root.to_string_lossy().to_string(),
        ..Default::default()
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    match std::fs::read_dir(root) {
        Ok(rd) => {
            for e in rd.flatten() {
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    dirs.push(e.path());
                } else if let Ok(m) = e.metadata() {
                    progress.tick();
                    files.push((e.file_name().to_string_lossy().to_string(), m.len()));
                }
            }
        }
        Err(_) => {
            unreadable.fetch_add(1, Ordering::Relaxed);
        }
    }
    let results = Mutex::new(Vec::with_capacity(dirs.len()));
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8)
        .min(dirs.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(d) = dirs.get(i) else { break };
                let n = scan_dir(d, progress, &unreadable);
                results.lock().unwrap().push(n);
            });
        }
    });
    let sub = results.into_inner().unwrap();
    finish(&mut top, sub, files);
    DiskMap {
        root: top,
        root_path: root.to_path_buf(),
        unreadable: unreadable.load(Ordering::Relaxed) as u64,
        cancelled: progress.cancelled(),
    }
}

fn scan_dir(dir: &Path, progress: &Progress, unreadable: &AtomicUsize) -> Node {
    let mut node = Node {
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        ..Default::default()
    };
    if progress.cancelled() {
        return node;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => {
            unreadable.fetch_add(1, Ordering::Relaxed);
            return node;
        }
    };
    let mut sub = Vec::new();
    let mut files = Vec::new();
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            sub.push(scan_dir(&e.path(), progress, unreadable));
        } else if let Ok(m) = e.metadata() {
            progress.tick();
            files.push((e.file_name().to_string_lossy().to_string(), m.len()));
        }
    }
    finish(&mut node, sub, files);
    node
}

/// Combine a folder's sub-folders and files into `node`.
fn finish(node: &mut Node, dirs: Vec<Node>, mut files: Vec<(String, u64)>) {
    node.size = dirs.iter().map(|d| d.size).sum::<u64>() + files.iter().map(|f| f.1).sum::<u64>();
    node.files = dirs.iter().map(|d| d.files).sum::<u64>() + files.len() as u64;
    if node.size < DETAIL_MIN {
        return;
    }
    files.sort_by(|a, b| b.1.cmp(&a.1));
    let keep = files
        .iter()
        .take(FILES_PER_DIR)
        .take_while(|f| f.1 >= FILE_MIN)
        .count();
    let mut children: Vec<Node> = dirs.into_iter().filter(|d| d.size > 0).collect();
    let rest = files.split_off(keep);
    for (name, size) in files {
        children.push(Node {
            name,
            size,
            files: 1,
            kind: Kind::File,
            children: Vec::new(),
        });
    }
    let rest_size: u64 = rest.iter().map(|f| f.1).sum();
    if rest_size > 0 {
        children.push(Node {
            name: String::new(),
            size: rest_size,
            files: rest.len() as u64,
            kind: Kind::Others,
            children: Vec::new(),
        });
    }
    children.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    node.children = children;
}

/// The largest folders and files anywhere in the tree, as (path, bytes,
/// is folder). A folder is listed only when no single entry inside it
/// accounts for most of its size, so the list points at where space really
/// goes instead of repeating every parent.
pub fn largest(map: &DiskMap, n: usize) -> Vec<(String, u64, bool)> {
    let mut out = Vec::new();
    fn walk(node: &Node, path: &Path, out: &mut Vec<(String, u64, bool)>) {
        for c in &node.children {
            if c.kind == Kind::Others {
                continue;
            }
            let p = path.join(&c.name);
            let dominated = c
                .children
                .first()
                .is_some_and(|g| g.kind != Kind::Others && g.size * 10 >= c.size * 8);
            if !c.is_dir() || !dominated {
                out.push((p.to_string_lossy().to_string(), c.size, c.is_dir()));
            }
            if c.is_dir() {
                walk(c, &p, out);
            }
        }
    }
    walk(&map.root, &map.root_path, &mut out);
    out.sort_by(|a, b| b.1.cmp(&a.1));
    // Drop folders whose size is mostly an already-listed entry inside them.
    let mut kept: Vec<(String, u64, bool)> = Vec::new();
    for e in out {
        if kept.len() >= n {
            break;
        }
        let inside_bigger = kept
            .iter()
            .any(|k| e.2 && k.0.starts_with(&e.0) && k.1 * 10 >= e.1 * 8);
        if !inside_bigger {
            kept.push(e);
        }
    }
    kept
}

/// A rectangle: x, y, width, height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Squarified treemap (Bruls, Huizing & van Wijk). `sizes` must be sorted
/// largest first; returns one rectangle per size, in the same order. Zero
/// sizes get empty rectangles.
pub fn layout(sizes: &[u64], area: Rect) -> Vec<Rect> {
    let mut out = vec![
        Rect {
            x: area.x,
            y: area.y,
            w: 0.0,
            h: 0.0
        };
        sizes.len()
    ];
    let total: f64 = sizes.iter().map(|&s| s as f64).sum();
    if total <= 0.0 || area.w <= 0.0 || area.h <= 0.0 {
        return out;
    }
    let scale = (area.w as f64 * area.h as f64) / total;
    let areas: Vec<f64> = sizes.iter().map(|&s| s as f64 * scale).collect();
    let mut free = area;
    let mut start = 0;
    while start < areas.len() {
        if areas[start] <= 0.0 {
            start += 1;
            continue;
        }
        let short = free.w.min(free.h) as f64;
        // Grow the row while the worst aspect ratio improves.
        let mut end = start + 1;
        let mut worst = worst_ratio(&areas[start..end], short);
        while end < areas.len() && areas[end] > 0.0 {
            let w = worst_ratio(&areas[start..end + 1], short);
            if w > worst {
                break;
            }
            worst = w;
            end += 1;
        }
        let row_area: f64 = areas[start..end].iter().sum();
        if free.w >= free.h {
            // Column along the left edge.
            let col_w = (row_area / free.h as f64) as f32;
            let mut y = free.y;
            for i in start..end {
                let h = (areas[i] / col_w as f64) as f32;
                out[i] = Rect {
                    x: free.x,
                    y,
                    w: col_w,
                    h,
                };
                y += h;
            }
            free.x += col_w;
            free.w = (free.w - col_w).max(0.0);
        } else {
            // Row along the top edge.
            let row_h = (row_area / free.w as f64) as f32;
            let mut x = free.x;
            for i in start..end {
                let w = (areas[i] / row_h as f64) as f32;
                out[i] = Rect {
                    x,
                    y: free.y,
                    w,
                    h: row_h,
                };
                x += w;
            }
            free.y += row_h;
            free.h = (free.h - row_h).max(0.0);
        }
        start = end;
    }
    out
}

fn worst_ratio(row: &[f64], short: f64) -> f64 {
    let s: f64 = row.iter().sum();
    let (mut lo, mut hi) = (f64::MAX, 0.0f64);
    for &a in row {
        lo = lo.min(a);
        hi = hi.max(a);
    }
    let s2 = s * s;
    let w2 = short * short;
    (w2 * hi / s2).max(s2 / (w2 * lo))
}

/// Drives to offer: fixed and removable drive roots that exist.
pub fn drives() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        // Fixed drives, and removable ones with media in them. Asking the
        // drive type first avoids touching empty card readers and
        // disconnected network drives, which can stall.
        use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
        const DRIVE_REMOVABLE: u32 = 2;
        const DRIVE_FIXED: u32 = 3;
        let mask = unsafe { GetLogicalDrives() };
        (0..26u8)
            .filter(|i| mask & (1 << i) != 0)
            .map(|i| format!("{}:\\", (b'A' + i) as char))
            .filter(|root| {
                let w = crate::platform::windows::wide(root);
                unsafe {
                    match GetDriveTypeW(w.as_ptr()) {
                        DRIVE_FIXED => true,
                        DRIVE_REMOVABLE => {
                            let (mut a, mut t, mut f) = (0u64, 0u64, 0u64);
                            GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut t, &mut f) != 0
                        }
                        _ => false,
                    }
                }
            })
            .map(PathBuf::from)
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut v = vec![PathBuf::from("/")];
        if let Some(h) = std::env::var_os("HOME") {
            v.push(PathBuf::from(h));
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_fills_the_area_proportionally() {
        let sizes = [600, 300, 200, 100, 50, 50, 0];
        let area = Rect {
            x: 10.0,
            y: 20.0,
            w: 400.0,
            h: 260.0,
        };
        let rs = layout(&sizes, area);
        let total: u64 = sizes.iter().sum();
        let mut sum = 0.0;
        for (r, &s) in rs.iter().zip(&sizes) {
            let a = r.w * r.h;
            sum += a;
            let want = area.w * area.h * s as f32 / total as f32;
            assert!((a - want).abs() < 1.0, "{a} vs {want}");
            if s > 0 {
                assert!(r.x >= area.x - 0.01 && r.y >= area.y - 0.01);
                assert!(r.x + r.w <= area.x + area.w + 0.5);
                assert!(r.y + r.h <= area.y + area.h + 0.5);
            }
        }
        assert!((sum - area.w * area.h).abs() < 2.0);
        // Squarified: the biggest block is not a sliver.
        let r = rs[0];
        assert!(r.w.max(r.h) / r.w.min(r.h) < 3.0);
    }

    #[test]
    fn layout_handles_empty_input() {
        let area = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        assert!(layout(&[], area).is_empty());
        assert_eq!(layout(&[0, 0], area)[1].w, 0.0);
    }

    #[test]
    fn scan_sums_sizes_and_groups_small_files() {
        let dir = std::env::temp_dir().join(format!("expunge-diskmap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("big/inner")).unwrap();
        std::fs::create_dir_all(dir.join("tiny")).unwrap();
        std::fs::write(dir.join("big/inner/a.bin"), vec![0u8; 1_500_000]).unwrap();
        std::fs::write(dir.join("big/b.bin"), vec![0u8; 400_000]).unwrap();
        for i in 0..5 {
            std::fs::write(dir.join(format!("big/s{i}.txt")), vec![0u8; 1000]).unwrap();
        }
        std::fs::write(dir.join("tiny/x.txt"), b"hello").unwrap();
        let p = Progress::default();
        let m = scan(&dir, &p);
        assert_eq!(m.root.size, 1_500_000 + 400_000 + 5000 + 5);
        assert_eq!(m.root.files, 8);
        let big = &m.root.children[0];
        assert_eq!(big.name, "big");
        // inner, b.bin, and the five small files summed together.
        assert_eq!(big.children.len(), 3);
        assert_eq!(big.children[0].name, "inner");
        assert_eq!(big.children[1].name, "b.bin");
        assert_eq!(big.children[2].kind, Kind::Others);
        assert_eq!(big.children[2].files, 5);
        // Small folders keep their total but not their contents.
        let tiny = m.root.children.iter().find(|c| c.name == "tiny").unwrap();
        assert_eq!(tiny.size, 5);
        assert!(tiny.children.is_empty());
        assert_eq!(m.path_of(&[0, 0]), dir.join("big").join("inner"));
        assert_eq!(m.root.at(&[0, 1]).unwrap().ext(), "bin");
        let top = largest(&m, 3);
        let a = dir.join("big").join("inner").join("a.bin");
        assert!(top.iter().any(|e| e.0 == a.to_string_lossy()), "{top:?}");
        assert!(top.iter().all(|e| !e.0.ends_with("inner")), "{top:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
