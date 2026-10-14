//! Small helpers: command-line splitting, glob matching, IDs, and the safety
//! rails that stop Expunge from ever touching critical system locations.

use crate::model::fold;
use crate::platform::{expand_env, normalize_reg_path};

/// Current local time as a sortable, readable string.
pub fn now_string() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// A unique, file-name-safe identifier based on the current time.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{:03}", chrono::Local::now().format("%Y%m%d-%H%M%S"), n % 1000)
}

/// Make a string safe to use as a file name.
pub fn safe_file_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' })
        .collect();
    let t = cleaned.trim().trim_matches('.').to_string();
    if t.is_empty() {
        "unnamed".into()
    } else {
        t.chars().take(80).collect()
    }
}

/// Split a Windows command line into (program, arguments). Handles quoted
/// paths and the common unquoted `C:\Program Files\X\unins000.exe /x` form.
pub fn split_command_line(cmd: &str) -> (String, String) {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (rest[..end].to_string(), rest[end + 1..].trim().to_string());
        }
        return (rest.to_string(), String::new());
    }
    let lower = cmd.to_lowercase();
    for ext in [".exe", ".com", ".bat", ".cmd", ".msi"] {
        let mut search_from = 0;
        while let Some(pos) = lower[search_from..].find(ext) {
            let end = search_from + pos + ext.len();
            if end == cmd.len() || cmd.as_bytes()[end] == b' ' {
                return (cmd[..end].to_string(), cmd[end..].trim().to_string());
            }
            search_from = end;
        }
    }
    match cmd.split_once(' ') {
        Some((p, a)) => (p.to_string(), a.trim().to_string()),
        None => (cmd.to_string(), String::new()),
    }
}

/// Case-insensitive glob with `*` (any run of characters, including `\`)
/// and `?` (any single character).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// True if `path` is excluded by any pattern. A pattern also excludes
/// everything beneath the location it names.
pub fn is_excluded(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| {
        let pat = pat.trim();
        if pat.is_empty() || pat.starts_with('#') {
            return false;
        }
        let pat = expand_env(pat);
        glob_match(&pat, path) || glob_match(&format!("{}\\*", pat.trim_end_matches('\\')), path)
    })
}

fn norm_file(p: &str) -> String {
    let mut s = fold(&p.replace('/', "\\"));
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    s
}

/// Folders Expunge must never delete as a whole, whatever a log or heuristic
/// says. Files *inside* them may still be removed individually.
pub fn protected_dirs() -> Vec<String> {
    let mut v: Vec<String> = [
        "%SystemDrive%\\",
        "%SystemRoot%",
        "%SystemRoot%\\System32",
        "%SystemRoot%\\SysWOW64",
        "%SystemRoot%\\System32\\drivers",
        "%SystemRoot%\\System32\\Tasks",
        "%SystemRoot%\\Fonts",
        "%SystemRoot%\\Temp",
        "%ProgramFiles%",
        "%ProgramFiles(x86)%",
        "%ProgramW6432%",
        "%CommonProgramFiles%",
        "%CommonProgramFiles(x86)%",
        "%CommonProgramW6432%",
        "%ProgramData%",
        "%ProgramData%\\Microsoft",
        "%ProgramData%\\Microsoft\\Windows",
        "%ProgramData%\\Microsoft\\Windows\\Start Menu",
        "%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs",
        "%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup",
        "%PUBLIC%",
        "%PUBLIC%\\Desktop",
        "%PUBLIC%\\Documents",
        "%USERPROFILE%",
        "%USERPROFILE%\\..",
        "%USERPROFILE%\\Desktop",
        "%USERPROFILE%\\Documents",
        "%USERPROFILE%\\Downloads",
        "%USERPROFILE%\\Pictures",
        "%USERPROFILE%\\Music",
        "%USERPROFILE%\\Videos",
        "%USERPROFILE%\\AppData",
        "%USERPROFILE%\\AppData\\LocalLow",
        "%APPDATA%",
        "%APPDATA%\\Microsoft",
        "%APPDATA%\\Microsoft\\Windows",
        "%APPDATA%\\Microsoft\\Windows\\Start Menu",
        "%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs",
        "%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup",
        "%LOCALAPPDATA%",
        "%LOCALAPPDATA%\\Microsoft",
        "%LOCALAPPDATA%\\Programs",
        "%LOCALAPPDATA%\\Temp",
        "%TEMP%",
        "%OneDrive%",
    ]
    .iter()
    .map(|s| expand_env(s))
    .filter(|s| !s.contains('%'))
    .map(|s| {
        // Resolve a trailing `\..` lexically (the Users folder).
        match s.strip_suffix("\\..") {
            Some(base) => base.rsplit_once('\\').map(|(p, _)| p.to_string()).unwrap_or_default(),
            None => s,
        }
    })
    .map(|s| norm_file(&s))
    .collect();
    v.sort();
    v.dedup();
    v
}

/// Whether deleting this folder (recursively) would be unsafe.
pub fn is_protected_dir(path: &str, protected: &[String]) -> bool {
    let p = norm_file(path);
    // Drive roots and anything with no real depth.
    let depth = p.trim_end_matches('\\').split('\\').filter(|s| !s.is_empty()).count();
    if depth <= 1 {
        return true;
    }
    protected.iter().any(|d| d == &p)
}

/// Registry keys that must never be deleted as a whole.
const PROTECTED_KEY_SUFFIXES: &[&str] = &[
    "software",
    "software\\classes",
    "software\\classes\\*",
    "software\\classes\\clsid",
    "software\\classes\\interface",
    "software\\classes\\typelib",
    "software\\classes\\appid",
    "software\\classes\\directory",
    "software\\classes\\directory\\shell",
    "software\\classes\\directory\\background\\shell",
    "software\\classes\\folder",
    "software\\classes\\drive",
    "software\\classes\\installer",
    "software\\classes\\local settings",
    "software\\microsoft",
    "software\\microsoft\\windows",
    "software\\microsoft\\windows\\currentversion",
    "software\\microsoft\\windows\\currentversion\\run",
    "software\\microsoft\\windows\\currentversion\\runonce",
    "software\\microsoft\\windows\\currentversion\\uninstall",
    "software\\microsoft\\windows\\currentversion\\app paths",
    "software\\microsoft\\windows\\currentversion\\explorer",
    "software\\microsoft\\windows\\currentversion\\shell extensions",
    "software\\microsoft\\windows\\currentversion\\shell extensions\\approved",
    "software\\microsoft\\windows nt",
    "software\\microsoft\\windows nt\\currentversion",
    "software\\policies",
    "software\\registeredapplications",
    "software\\clients",
    "software\\wow6432node",
    "system",
    "system\\currentcontrolset",
    "system\\currentcontrolset\\services",
    "system\\currentcontrolset\\control",
    "system\\currentcontrolset\\control\\session manager",
    "system\\currentcontrolset\\control\\session manager\\environment",
    "environment",
];

pub fn is_protected_key(path: &str) -> bool {
    let n = fold(&normalize_reg_path(path));
    let Some((hive, rest)) = n.split_once('\\') else {
        return true; // hive root
    };
    // HKU\<sid> and HKU\<sid>\Software are profile roots.
    let rest = if hive == "hku" {
        match rest.split_once('\\') {
            Some((_, r)) => r.to_string(),
            None => return true,
        }
    } else {
        rest.to_string()
    };
    // Treat the 32-bit view the same as the 64-bit one.
    let rest = rest.replace("software\\wow6432node\\", "software\\");
    // HKCR is a merged view of Software\Classes.
    let rest = if hive == "hkcr" { format!("software\\classes\\{rest}") } else { rest };
    PROTECTED_KEY_SUFFIXES.contains(&rest.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_commands() {
        assert_eq!(
            split_command_line("\"C:\\Program Files\\A\\unins000.exe\" /SILENT"),
            ("C:\\Program Files\\A\\unins000.exe".into(), "/SILENT".into())
        );
        assert_eq!(
            split_command_line("C:\\Program Files\\A B\\uninst.exe /x"),
            ("C:\\Program Files\\A B\\uninst.exe".into(), "/x".into())
        );
        assert_eq!(
            split_command_line("MsiExec.exe /X{1234}"),
            ("MsiExec.exe".into(), "/X{1234}".into())
        );
        assert_eq!(split_command_line("C:\\a\\b.exe"), ("C:\\a\\b.exe".into(), "".into()));
    }

    #[test]
    fn globs() {
        assert!(glob_match("HKLM\\SOFTWARE\\Microsoft\\*", "hklm\\software\\microsoft\\x\\y"));
        assert!(glob_match("*.etl", "C:\\a\\b.ETL"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "abbc"));
        assert!(is_excluded("HKCU\\Software\\X\\Bags\\1", &["HKCU\\Software\\X\\Bags".into()]));
        assert!(!is_excluded("HKCU\\Software\\X\\BagsMore", &["HKCU\\Software\\X\\Bags".into()]));
    }

    #[test]
    fn protected_keys() {
        assert!(is_protected_key("HKLM"));
        assert!(is_protected_key("HKLM\\SOFTWARE"));
        assert!(is_protected_key("HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run"));
        assert!(is_protected_key("HKCR\\CLSID"));
        assert!(is_protected_key("HKU\\S-1-5-21-1\\Software"));
        assert!(!is_protected_key("HKLM\\SOFTWARE\\Vendor\\App"));
        assert!(!is_protected_key("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\App"));
    }

    #[test]
    fn protected_dirs_rules() {
        std::env::set_var("ProgramFiles", "C:\\Program Files");
        let p = protected_dirs();
        assert!(is_protected_dir("C:\\", &p));
        assert!(is_protected_dir("C:\\Program Files\\", &p));
        assert!(is_protected_dir("c:\\program files", &p));
        assert!(!is_protected_dir("C:\\Program Files\\Vendor", &p));
        assert!(is_protected_dir("D:", &p));
    }
}
