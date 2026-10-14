//! winget (Windows Package Manager) integration: which installed programs
//! have updates, and upgrading them silently.
//!
//! winget has no stable machine-readable output for upgrades, so its table
//! is parsed by column position, taken from the header line above the row
//! of dashes. Column titles are localised, so only their positions are
//! used, never their text.

use crate::model::fold;
use crate::programs::InstalledProgram;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Upgrade {
    pub name: String,
    pub id: String,
    pub version: String,
    pub available: String,
    pub source: String,
}

/// Display width of a character in a console (East Asian wide characters
/// take two cells), so CJK names don't shift the columns.
fn char_width(c: char) -> usize {
    let u = c as u32;
    let wide = (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE4F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x1F300..=0x1F64F).contains(&u)
        || (0x20000..=0x3FFFD).contains(&u);
    if wide {
        2
    } else {
        1
    }
}

/// Cut a line into fields at the given display-column positions.
fn split_at_columns(line: &str, starts: &[usize]) -> Vec<String> {
    let mut fields = vec![String::new(); starts.len()];
    let mut col = 0usize;
    for c in line.chars() {
        let idx = starts.iter().rposition(|&s| col >= s).unwrap_or(0);
        fields[idx].push(c);
        col += char_width(c);
    }
    fields.into_iter().map(|f| f.trim().to_string()).collect()
}

/// Start columns of the words in a header line.
fn header_columns(header: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut col = 0usize;
    let mut prev_space = true;
    for c in header.chars() {
        let space = c == ' ';
        if !space && prev_space {
            starts.push(col);
        }
        prev_space = space;
        col += char_width(c);
    }
    starts
}

/// Parse the tables printed by `winget upgrade`.
pub fn parse_upgrades(output: &str) -> Vec<Upgrade> {
    // winget draws a spinner with carriage returns before the table.
    let lines: Vec<&str> = output
        .lines()
        .map(|l| l.rsplit('\r').next().unwrap_or(l))
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let is_rule = |l: &str| l.trim().len() >= 10 && l.trim().chars().all(|c| c == '-');
        if i > 0 && is_rule(lines[i]) {
            let starts = header_columns(lines[i - 1]);
            i += 1;
            if starts.len() < 4 {
                continue;
            }
            while i < lines.len() && !lines[i].trim().is_empty() && !is_rule(lines[i]) {
                let f = split_at_columns(lines[i], &starts);
                // Name, Id, Version, Available, [Source]
                if f.len() >= 4 && !f[1].is_empty() && !f[3].is_empty() && !f[1].contains(' ') {
                    out.push(Upgrade {
                        name: f[0].clone(),
                        id: f[1].clone(),
                        version: f[2].clone(),
                        available: f[3].clone(),
                        source: f.get(4).cloned().unwrap_or_default(),
                    });
                }
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    out
}

fn norm(s: &str) -> String {
    fold(s).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Match a winget entry to an installed program: same name (winget may cut
/// long names with "…") and same installed version when both are known.
pub fn matches(u: &Upgrade, p: &InstalledProgram) -> bool {
    let (truncated, name) = match u.name.strip_suffix('…') {
        Some(n) => (true, n),
        None => (false, u.name.as_str()),
    };
    let (a, b) = (norm(name), norm(&p.name));
    if a.is_empty() {
        return false;
    }
    let name_ok = if truncated { b.starts_with(&a) } else { a == b };
    let version_ok = u.version.is_empty()
        || p.version.is_empty()
        || u.version.starts_with('<')
        || norm(&u.version) == norm(&p.version)
        || norm(&p.version).starts_with(&norm(&u.version));
    name_ok && version_ok
}

/// Demo data for screenshots and tests on non-Windows builds.
fn demo() -> bool {
    !cfg!(windows) && std::env::var_os("EXPUNGE_DEMO").is_some()
}

const DEMO_UPGRADES: &str =
    "Name                         Id                          Version   Available Source\n\
------------------------------------------------------------------------------------\n\
Mozilla Firefox (x64 en-US)  Mozilla.Firefox             131.0.3   132.0     winget\n\
Visual Studio Code           Microsoft.VisualStudioCode  1.94.2    1.95.1    winget\n\
VLC media player             VideoLAN.VLC                3.0.21    3.0.22    winget\n";

/// Whether winget is installed.
pub fn available() -> bool {
    if demo() {
        return true;
    }
    crate::platform::run_hidden("winget", &["--version"]).is_ok_and(|(code, _)| code == 0)
}

/// Programs with updates, according to winget.
pub fn list_upgrades() -> anyhow::Result<Vec<Upgrade>> {
    if demo() {
        return Ok(parse_upgrades(DEMO_UPGRADES));
    }
    let (_, out) = crate::platform::run_hidden(
        "winget",
        &[
            "upgrade",
            "--include-unknown",
            "--accept-source-agreements",
            "--disable-interactivity",
        ],
    )?;
    Ok(parse_upgrades(&out))
}

/// Upgrade one package silently. Returns winget's output on failure.
pub fn upgrade(id: &str) -> anyhow::Result<()> {
    let (code, out) = crate::platform::run_hidden(
        "winget",
        &[
            "upgrade",
            "--id",
            id,
            "--exact",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ],
    )?;
    if code == 0 {
        Ok(())
    } else {
        let last = out
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string();
        anyhow::bail!("winget exit code {code}: {last}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "   - \r   \\ \r\
Name                               Id                          Version      Available    Source\n\
----------------------------------------------------------------------------------------------\n\
Mozilla Firefox (x64 en-US)        Mozilla.Firefox             131.0.3      132.0        winget\n\
Microsoft Visual Studio Code (Use… Microsoft.VisualStudioCode  1.94.2       1.95.1       winget\n\
7-Zip 24.08 (x64)                  7zip.7zip                   24.08        24.09        winget\n\
3 upgrades available.\n\
\n\
The following packages have an upgrade available, but require explicit targeting for upgrade:\n\
Name      Id              Version Available Source\n\
--------------------------------------------------\n\
Discord   Discord.Discord 1.0.9172 1.0.9175 winget\n";

    #[test]
    fn parses_upgrade_tables() {
        let u = parse_upgrades(SAMPLE);
        assert_eq!(u.len(), 4, "{u:#?}");
        assert_eq!(u[0].id, "Mozilla.Firefox");
        assert_eq!(u[0].available, "132.0");
        assert_eq!(u[1].name, "Microsoft Visual Studio Code (Use…");
        assert_eq!(u[3].id, "Discord.Discord");
        assert_eq!(u[3].available, "1.0.9175");
    }

    #[test]
    fn parses_wide_characters() {
        let text = "名前         Id              Version Available Source\n\
-----------------------------------------------------\n\
秀丸エディタ Hidemaru.Editor 9.1     9.2       winget\n";
        let u = parse_upgrades(text);
        assert_eq!(u.len(), 1, "{u:#?}");
        assert_eq!(u[0].id, "Hidemaru.Editor");
    }

    #[test]
    fn matches_programs() {
        let u = parse_upgrades(SAMPLE);
        let vscode = InstalledProgram {
            name: "Microsoft Visual Studio Code (User)".into(),
            version: "1.94.2".into(),
            ..Default::default()
        };
        let firefox = InstalledProgram {
            name: "Mozilla Firefox (x64 en-US)".into(),
            version: "131.0.3".into(),
            ..Default::default()
        };
        assert!(matches(&u[1], &vscode));
        assert!(matches(&u[0], &firefox));
        assert!(!matches(&u[0], &vscode));
        let old_firefox = InstalledProgram {
            version: "120.0".into(),
            ..firefox
        };
        assert!(!matches(&u[0], &old_firefox));
    }
}
