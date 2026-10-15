//! Pre- and post-uninstall scripts: commands the user attaches to a program,
//! run before its uninstaller starts and after it has finished (for example
//! to export settings or stop a service first). Advanced mode only.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ProgramScripts {
    #[serde(default)]
    pub pre: String,
    #[serde(default)]
    pub post: String,
}

impl ProgramScripts {
    pub fn is_empty(&self) -> bool {
        self.pre.trim().is_empty() && self.post.trim().is_empty()
    }
}

/// Scripts by program name (case-insensitive).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Scripts {
    #[serde(default)]
    pub programs: BTreeMap<String, ProgramScripts>,
}

fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

impl Scripts {
    pub fn path(data_root: &Path) -> PathBuf {
        data_root.join("scripts.json")
    }

    pub fn load(data_root: &Path) -> Self {
        std::fs::read_to_string(Self::path(data_root))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data_root: &Path) -> anyhow::Result<()> {
        std::fs::write(Self::path(data_root), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn get(&self, program: &str) -> ProgramScripts {
        self.programs
            .get(&key(program))
            .cloned()
            .unwrap_or_default()
    }

    pub fn set(&mut self, program: &str, s: ProgramScripts) {
        if s.is_empty() {
            self.programs.remove(&key(program));
        } else {
            self.programs.insert(key(program), s);
        }
    }
}

/// Run one script with `cmd /C`, giving it the program's name and folder in
/// EXPUNGE_PROGRAM and EXPUNGE_INSTALL_DIR. Returns a line for the report.
pub fn run(script: &str, program: &str, install_dir: &str) -> Result<String, String> {
    let script = script.trim();
    if script.is_empty() {
        return Ok(String::new());
    }
    let mut c = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C").arg(script);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(script);
        c
    };
    c.env("EXPUNGE_PROGRAM", program)
        .env("EXPUNGE_INSTALL_DIR", install_dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = c
        .output()
        .map_err(|e| format!("could not start the script: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !err.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&err);
    }
    match out.status.code() {
        Some(0) => Ok(text),
        Some(code) => Err(format!("exit code {code}: {text}")),
        None => Err("the script was stopped".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_run() {
        let d = std::env::temp_dir().join(format!("expunge-scripts-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut s = Scripts::default();
        s.set(
            "Photo Editor",
            ProgramScripts {
                pre: "echo before".into(),
                post: String::new(),
            },
        );
        s.save(&d).unwrap();
        let back = Scripts::load(&d);
        assert_eq!(back.get("photo editor").pre, "echo before");
        let mut back = back;
        back.set("PHOTO EDITOR", ProgramScripts::default());
        assert!(back.programs.is_empty());
        let _ = std::fs::remove_dir_all(&d);

        #[cfg(not(windows))]
        {
            assert_eq!(
                run("echo $EXPUNGE_PROGRAM", "Photo Editor", "/x").unwrap(),
                "Photo Editor"
            );
            assert!(run("exit 3", "a", "b")
                .unwrap_err()
                .starts_with("exit code 3"));
            assert_eq!(run("  ", "a", "b").unwrap(), "");
        }
    }
}
