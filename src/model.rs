//! Core data types shared by every part of Expunge.
//!
//! Registry and file paths are compared case-insensitively (like Windows does),
//! so every map is keyed by a *folded* (lower-cased) path while the entry itself
//! keeps the original spelling for display and for writing back.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Raw registry value types (winnt.h).
pub mod reg_type {
    pub const NONE: u32 = 0;
    pub const SZ: u32 = 1;
    pub const EXPAND_SZ: u32 = 2;
    pub const BINARY: u32 = 3;
    pub const DWORD: u32 = 4;
    pub const DWORD_BIG_ENDIAN: u32 = 5;
    pub const LINK: u32 = 6;
    pub const MULTI_SZ: u32 = 7;
    pub const QWORD: u32 = 11;
}

/// Case-fold a registry or file path / value name for use as a map key.
pub fn fold(s: &str) -> String {
    s.to_lowercase()
}

/// Serialize byte vectors as compact hex strings instead of JSON number arrays.
pub mod hexbytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&to_hex(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        from_hex(&s).map_err(serde::de::Error::custom)
    }

    pub fn to_hex(data: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(data.len() * 2);
        for b in data {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        }
        out
    }

    pub fn from_hex(s: &str) -> Result<Vec<u8>, String> {
        if s.len() % 2 != 0 {
            return Err("odd-length hex string".into());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect()
    }
}

/// A single registry value with its raw type and bytes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RegValue {
    /// Value name; the empty string is the key's "(Default)" value.
    pub name: String,
    pub kind: u32,
    #[serde(with = "hexbytes")]
    pub data: Vec<u8>,
}

impl RegValue {
    pub fn string(name: &str, s: &str) -> Self {
        RegValue { name: name.into(), kind: reg_type::SZ, data: encode_utf16z(s) }
    }

    pub fn dword(name: &str, v: u32) -> Self {
        RegValue { name: name.into(), kind: reg_type::DWORD, data: v.to_le_bytes().to_vec() }
    }

    /// The value interpreted as text, when it is a string type.
    pub fn as_string(&self) -> Option<String> {
        match self.kind {
            reg_type::SZ | reg_type::EXPAND_SZ | reg_type::LINK => Some(decode_utf16z(&self.data)),
            reg_type::MULTI_SZ => Some(decode_multi_sz(&self.data).join("; ")),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self.kind {
            reg_type::DWORD if self.data.len() >= 4 => {
                Some(u32::from_le_bytes([self.data[0], self.data[1], self.data[2], self.data[3]]))
            }
            reg_type::DWORD_BIG_ENDIAN if self.data.len() >= 4 => {
                Some(u32::from_be_bytes([self.data[0], self.data[1], self.data[2], self.data[3]]))
            }
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self.kind {
            reg_type::NONE => "REG_NONE",
            reg_type::SZ => "REG_SZ",
            reg_type::EXPAND_SZ => "REG_EXPAND_SZ",
            reg_type::BINARY => "REG_BINARY",
            reg_type::DWORD => "REG_DWORD",
            reg_type::DWORD_BIG_ENDIAN => "REG_DWORD_BIG_ENDIAN",
            reg_type::LINK => "REG_LINK",
            reg_type::MULTI_SZ => "REG_MULTI_SZ",
            reg_type::QWORD => "REG_QWORD",
            _ => "REG_UNKNOWN",
        }
    }

    /// Short human-readable rendering of the data.
    pub fn display_data(&self) -> String {
        if let Some(s) = self.as_string() {
            return s;
        }
        if let Some(v) = self.as_u32() {
            return format!("0x{v:08x} ({v})");
        }
        if self.kind == reg_type::QWORD && self.data.len() >= 8 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&self.data[..8]);
            let v = u64::from_le_bytes(b);
            return format!("0x{v:016x} ({v})");
        }
        let mut s = String::new();
        for (i, b) in self.data.iter().take(48).enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(&format!("{b:02x}"));
        }
        if self.data.len() > 48 {
            s.push_str(&format!(" … ({} bytes)", self.data.len()));
        }
        s
    }

    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            "(Default)"
        } else {
            &self.name
        }
    }
}

pub fn encode_utf16z(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2 + 2);
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

pub fn decode_utf16z(data: &[u8]) -> String {
    let units: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

pub fn decode_multi_sz(data: &[u8]) -> Vec<String> {
    let units: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    units
        .split(|&u| u == 0)
        .filter(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

/// One registry key and its values (not its subkeys).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct KeyData {
    /// Full path in original case, e.g. `HKLM\SOFTWARE\Vendor\App`.
    pub path: String,
    /// Values keyed by folded name.
    pub values: BTreeMap<String, RegValue>,
}

impl KeyData {
    pub fn new(path: &str) -> Self {
        KeyData { path: path.to_string(), values: BTreeMap::new() }
    }

    pub fn with(mut self, v: RegValue) -> Self {
        self.values.insert(fold(&v.name), v);
        self
    }

    pub fn get(&self, name: &str) -> Option<&RegValue> {
        self.values.get(&fold(name))
    }

    pub fn get_string(&self, name: &str) -> Option<String> {
        self.get(name).and_then(|v| v.as_string()).filter(|s| !s.trim().is_empty())
    }

    pub fn get_u32(&self, name: &str) -> Option<u32> {
        self.get(name).and_then(|v| v.as_u32())
    }
}

/// Metadata for a file or directory captured in a snapshot.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// Last-write time, seconds since the Unix epoch.
    pub mtime: i64,
}

/// A point-in-time capture of the parts of the system Expunge watches.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Snapshot {
    pub created: String,
    pub computer: String,
    pub user: String,
    /// Registry keys keyed by folded path.
    pub registry: BTreeMap<String, KeyData>,
    /// Files and directories keyed by folded path.
    pub files: BTreeMap<String, FileEntry>,
    /// Things that could not be read (access denied etc.).
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl Snapshot {
    pub fn insert_key(&mut self, k: KeyData) {
        self.registry.insert(fold(&k.path), k);
    }

    pub fn insert_file(&mut self, f: FileEntry) {
        self.files.insert(fold(&f.path), f);
    }

    pub fn value_count(&self) -> usize {
        self.registry.values().map(|k| k.values.len()).sum()
    }
}

/// A registry value located in a specific key.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ValueRef {
    pub key: String,
    pub value: RegValue,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ValueChange {
    pub key: String,
    pub before: RegValue,
    pub after: RegValue,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub before: FileEntry,
    pub after: FileEntry,
}

/// Everything that differs between two snapshots.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    /// New keys, with the values they contain.
    pub keys_added: Vec<KeyData>,
    /// Keys that disappeared, with the values they had.
    pub keys_removed: Vec<KeyData>,
    /// New values in keys that already existed.
    pub values_added: Vec<ValueRef>,
    /// Values removed from keys that still exist.
    pub values_removed: Vec<ValueRef>,
    pub values_modified: Vec<ValueChange>,
    pub files_added: Vec<FileEntry>,
    pub files_removed: Vec<FileEntry>,
    pub files_modified: Vec<FileChange>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    pub fn total(&self) -> usize {
        self.keys_added.len()
            + self.keys_removed.len()
            + self.values_added.len()
            + self.values_removed.len()
            + self.values_modified.len()
            + self.files_added.len()
            + self.files_removed.len()
            + self.files_modified.len()
    }

    pub fn stats(&self) -> ChangeStats {
        ChangeStats {
            keys_added: self.keys_added.len(),
            keys_removed: self.keys_removed.len(),
            values_added: self.values_added.len()
                + self.keys_added.iter().map(|k| k.values.len()).sum::<usize>(),
            values_removed: self.values_removed.len(),
            values_modified: self.values_modified.len(),
            files_added: self.files_added.iter().filter(|f| !f.is_dir).count(),
            dirs_added: self.files_added.iter().filter(|f| f.is_dir).count(),
            files_removed: self.files_removed.len(),
            files_modified: self.files_modified.len(),
            bytes_added: self.files_added.iter().map(|f| f.size).sum(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeStats {
    pub keys_added: usize,
    pub keys_removed: usize,
    pub values_added: usize,
    pub values_removed: usize,
    pub values_modified: usize,
    pub files_added: usize,
    pub dirs_added: usize,
    pub files_removed: usize,
    pub files_modified: usize,
    pub bytes_added: u64,
}

/// A saved record of everything one installation changed: the heart of a
/// monitored program. Reverting it is how Expunge uninstalls precisely.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct InstallLog {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub version: String,
    pub created: String,
    /// Installer that was run under monitoring, if any.
    #[serde(default)]
    pub installer: String,
    /// The program's own Uninstall registry key, if one was created.
    #[serde(default)]
    pub uninstall_key: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub group: String,
    pub changes: ChangeSet,
}

/// Human-friendly byte size.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let d = vec![0u8, 1, 0xab, 0xff];
        assert_eq!(hexbytes::from_hex(&hexbytes::to_hex(&d)).unwrap(), d);
    }

    #[test]
    fn string_values() {
        let v = RegValue::string("DisplayName", "Foo Bar");
        assert_eq!(v.as_string().unwrap(), "Foo Bar");
        assert_eq!(RegValue::dword("x", 7).as_u32(), Some(7));
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
    }
}
