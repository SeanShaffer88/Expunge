//! On-disk cache of slow-to-compute program facts, so the program list opens
//! fast on later launches: folder sizes (walking a large folder takes
//! seconds) and digital-signature results (verifying hashes the whole file).
//!
//! Entries check themselves: a size is reused only for the same folder and
//! version and for a week; a signature only while the file's size and
//! modification time are unchanged.

use crate::trust::Trust;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// How long a measured folder size is trusted.
const SIZE_MAX_AGE_SECS: i64 = 7 * 24 * 3600;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SizeEntry {
    pub location: String,
    pub version: String,
    pub bytes: u64,
    /// Unix time it was measured.
    pub measured: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TrustEntry {
    pub path: String,
    pub len: u64,
    pub mtime: i64,
    pub trust: Trust,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Cache {
    #[serde(default)]
    pub sizes: HashMap<String, SizeEntry>,
    #[serde(default)]
    pub trust: HashMap<String, TrustEntry>,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Size and modification time of a file, if it exists.
pub fn file_stamp(path: &str) -> Option<(u64, i64)> {
    let m = std::fs::metadata(crate::platform::expand_env(path)).ok()?;
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Some((m.len(), mtime))
}

impl Cache {
    /// Load the cache; a missing or damaged file gives an empty cache.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn size(&self, key: &str, location: &str, version: &str, now: i64) -> Option<u64> {
        let e = self.sizes.get(key)?;
        (e.location == location && e.version == version && now - e.measured < SIZE_MAX_AGE_SECS)
            .then_some(e.bytes)
    }

    pub fn put_size(&mut self, key: &str, location: &str, version: &str, bytes: u64, now: i64) {
        self.sizes.insert(
            key.to_string(),
            SizeEntry {
                location: location.into(),
                version: version.into(),
                bytes,
                measured: now,
            },
        );
    }

    /// The cached signature result, if the file is unchanged since.
    pub fn trust(&self, key: &str, path: &str, stamp: Option<(u64, i64)>) -> Option<Trust> {
        let e = self.trust.get(key)?;
        let (len, mtime) = stamp?;
        (e.path == path && e.len == len && e.mtime == mtime).then(|| e.trust.clone())
    }

    pub fn put_trust(&mut self, key: &str, path: &str, stamp: Option<(u64, i64)>, trust: &Trust) {
        // Only definite answers about existing files are worth keeping.
        let Some((len, mtime)) = stamp else { return };
        if matches!(trust, Trust::Unknown | Trust::Missing) {
            return;
        }
        self.trust.insert(
            key.to_string(),
            TrustEntry {
                path: path.into(),
                len,
                mtime,
                trust: trust.clone(),
            },
        );
    }

    /// Drop entries for programs that are no longer installed.
    pub fn retain_keys(&mut self, keys: &HashSet<String>) {
        self.sizes.retain(|k, _| keys.contains(k));
        self.trust.retain(|k, _| keys.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_expire_and_follow_location_and_version() {
        let mut c = Cache::default();
        c.put_size("k", "C:\\App", "1.0", 500, 1000);
        assert_eq!(c.size("k", "C:\\App", "1.0", 2000), Some(500));
        assert_eq!(c.size("k", "C:\\App", "1.1", 2000), None);
        assert_eq!(c.size("k", "D:\\App", "1.0", 2000), None);
        assert_eq!(
            c.size("k", "C:\\App", "1.0", 1000 + SIZE_MAX_AGE_SECS),
            None
        );
    }

    #[test]
    fn trust_reused_only_for_unchanged_files_and_round_trips() {
        let mut c = Cache::default();
        let t = Trust::Signed {
            signer: "Contoso".into(),
        };
        c.put_trust("k", "C:\\a.exe", Some((10, 5)), &t);
        c.put_trust("gone", "C:\\b.exe", None, &t);
        c.put_trust("unknown", "C:\\c.exe", Some((1, 1)), &Trust::Unknown);
        assert_eq!(c.trust("k", "C:\\a.exe", Some((10, 5))), Some(t.clone()));
        assert_eq!(c.trust("k", "C:\\a.exe", Some((11, 5))), None);
        assert_eq!(c.trust.len(), 1);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        c.put_size("k", "C:\\App", "1", 7, 3);
        c.save(&path).unwrap();
        let back = Cache::load(&path);
        assert_eq!(back.trust("k", "C:\\a.exe", Some((10, 5))), Some(t));
        let mut keys = HashSet::new();
        keys.insert("other".to_string());
        let mut back = back;
        back.retain_keys(&keys);
        assert!(back.sizes.is_empty() && back.trust.is_empty());
        assert!(Cache::load(&dir.path().join("missing.json"))
            .sizes
            .is_empty());
    }
}
