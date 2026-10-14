//! Exporting install logs: HTML report, JSON, `.reg` undo script, and a
//! standalone PowerShell removal script.

use crate::model::*;
use crate::platform::{split_hive, HIVES};

pub fn long_path(path: &str) -> String {
    match split_hive(path) {
        Some((h, rest)) => {
            let long = HIVES
                .iter()
                .find(|(s, _)| *s == h)
                .map(|(_, l)| *l)
                .unwrap_or(h);
            if rest.is_empty() {
                long.to_string()
            } else {
                format!("{long}\\{rest}")
            }
        }
        None => path.to_string(),
    }
}

fn reg_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn reg_name(name: &str) -> String {
    if name.is_empty() {
        "@".into()
    } else {
        format!("\"{}\"", reg_escape(name))
    }
}

/// Format a value as a `.reg` file data string.
pub fn reg_data(v: &RegValue) -> String {
    let hex = |d: &[u8]| {
        d.iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    match v.kind {
        reg_type::SZ => {
            let s = decode_utf16z(&v.data);
            // Strings with embedded NULs or odd data can't round-trip as text.
            if encode_utf16z(&s) == v.data {
                format!("\"{}\"", reg_escape(&s))
            } else {
                format!("hex(1):{}", hex(&v.data))
            }
        }
        reg_type::DWORD if v.data.len() == 4 => format!("dword:{:08x}", v.as_u32().unwrap_or(0)),
        reg_type::BINARY => format!("hex:{}", hex(&v.data)),
        k => format!("hex({k:x}):{}", hex(&v.data)),
    }
}

/// A `.reg` file that undoes the installation's registry changes.
pub fn to_reg_undo(log: &InstallLog) -> String {
    let cs = &log.changes;
    let mut out = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    out.push_str(&format!(
        "; Expunge undo script for: {}\r\n; Created {}\r\n\r\n",
        log.name, log.created
    ));

    let added: std::collections::BTreeSet<String> =
        cs.keys_added.iter().map(|k| fold(&k.path)).collect();
    for k in &cs.keys_added {
        let has_parent = crate::platform::parent_path(&k.path).is_some_and(|p| {
            let mut cur = Some(p);
            while let Some(c) = cur {
                if added.contains(&fold(c)) && !crate::util::is_protected_key(c) {
                    return true;
                }
                cur = crate::platform::parent_path(c);
            }
            false
        });
        if !has_parent && !crate::util::is_protected_key(&k.path) {
            out.push_str(&format!("[-{}]\r\n\r\n", long_path(&k.path)));
        }
    }

    // Group value edits by key.
    let mut edits: std::collections::BTreeMap<String, (String, Vec<String>)> = Default::default();
    let mut edit = |key: &str, line: String| {
        edits
            .entry(fold(key))
            .or_insert_with(|| (key.to_string(), Vec::new()))
            .1
            .push(line);
    };
    for v in &cs.values_added {
        edit(&v.key, format!("{}=-", reg_name(&v.value.name)));
    }
    for v in &cs.values_modified {
        edit(
            &v.key,
            format!("{}={}", reg_name(&v.before.name), reg_data(&v.before)),
        );
    }
    for v in &cs.values_removed {
        edit(
            &v.key,
            format!("{}={}", reg_name(&v.value.name), reg_data(&v.value)),
        );
    }
    for k in &cs.keys_removed {
        let e = edits
            .entry(fold(&k.path))
            .or_insert_with(|| (k.path.clone(), Vec::new()));
        for v in k.values.values() {
            e.1.push(format!("{}={}", reg_name(&v.name), reg_data(v)));
        }
    }
    for (_, (key, lines)) in edits {
        out.push_str(&format!("[{}]\r\n", long_path(&key)));
        for l in lines {
            out.push_str(&l);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
    }
    out
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A PowerShell script that removes what the installation added.
pub fn to_ps1(log: &InstallLog) -> String {
    let cs = &log.changes;
    let mut out = String::new();
    out.push_str(&format!(
        "# Expunge removal script for: {}\r\n# Created {}\r\n",
        log.name, log.created
    ));
    out.push_str(
        "# Run in an elevated PowerShell. Registry values that were modified or removed\r\n",
    );
    out.push_str("# are restored by the companion .reg undo file (export it from Expunge).\r\n");
    out.push_str("#Requires -RunAsAdministrator\r\n$ErrorActionPreference = 'Continue'\r\n\r\n");

    let services: Vec<&str> = cs
        .keys_added
        .iter()
        .filter_map(|k| {
            let f = fold(&k.path);
            f.strip_prefix("hklm\\system\\currentcontrolset\\services\\")
                .filter(|r| !r.contains('\\'))
                .map(|_| crate::platform::leaf_name(&k.path))
        })
        .collect();
    if !services.is_empty() {
        out.push_str("# Services\r\n");
        for s in services {
            out.push_str(&format!(
                "sc.exe stop {0}; sc.exe delete {0}\r\n",
                ps_quote(s)
            ));
        }
        out.push_str("\r\n");
    }

    out.push_str("# Files\r\n");
    for f in cs.files_added.iter().filter(|f| !f.is_dir) {
        out.push_str(&format!(
            "Remove-Item -LiteralPath {} -Force -ErrorAction SilentlyContinue\r\n",
            ps_quote(&f.path)
        ));
    }
    let mut dirs: Vec<&FileEntry> = cs.files_added.iter().filter(|f| f.is_dir).collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.path.len()));
    if !dirs.is_empty() {
        out.push_str("\r\n# Folders (only removed when empty)\r\n");
        for d in dirs {
            out.push_str(&format!(
                "if ((Test-Path -LiteralPath {0}) -and -not (Get-ChildItem -LiteralPath {0} -Force)) {{ Remove-Item -LiteralPath {0} -Force }}\r\n",
                ps_quote(&d.path)
            ));
        }
    }
    out.push_str("\r\n# Registry\r\n");
    for k in &cs.keys_added {
        if crate::util::is_protected_key(&k.path) {
            continue;
        }
        out.push_str(&format!(
            "Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue\r\n",
            ps_quote(&format!("Registry::{}", long_path(&k.path)))
        ));
    }
    for v in &cs.values_added {
        out.push_str(&format!(
            "Remove-ItemProperty -LiteralPath {} -Name {} -Force -ErrorAction SilentlyContinue\r\n",
            ps_quote(&format!("Registry::{}", long_path(&v.key))),
            ps_quote(&v.value.name)
        ));
    }
    out
}

fn h(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Self-contained HTML report (dark, matching the app).
pub fn to_html(log: &InstallLog) -> String {
    let cs = &log.changes;
    let st = cs.stats();
    let mut rows = String::new();
    let mut row = |kind: &str, class: &str, what: &str, detail: &str| {
        rows.push_str(&format!(
            "<tr><td><span class=\"tag {class}\">{}</span></td><td class=\"p\">{}</td><td>{}</td></tr>\n",
            h(kind),
            h(what),
            h(detail)
        ));
    };
    for k in &cs.keys_added {
        row(
            "Key added",
            "add",
            &k.path,
            &format!("{} values", k.values.len()),
        );
    }
    for v in &cs.values_added {
        row(
            "Value added",
            "add",
            &format!("{}\\{}", v.key, v.value.display_name()),
            &v.value.display_data(),
        );
    }
    for v in &cs.values_modified {
        row(
            "Value changed",
            "mod",
            &format!("{}\\{}", v.key, v.after.display_name()),
            &format!("{} → {}", v.before.display_data(), v.after.display_data()),
        );
    }
    for v in &cs.values_removed {
        row(
            "Value removed",
            "del",
            &format!("{}\\{}", v.key, v.value.display_name()),
            &v.value.display_data(),
        );
    }
    for k in &cs.keys_removed {
        row("Key removed", "del", &k.path, "");
    }
    for f in &cs.files_added {
        let what = if f.is_dir {
            "Folder added"
        } else {
            "File added"
        };
        row(
            what,
            "add",
            &f.path,
            &if f.is_dir {
                String::new()
            } else {
                human_size(f.size)
            },
        );
    }
    for f in &cs.files_modified {
        row(
            "File changed",
            "mod",
            &f.after.path,
            &format!(
                "{} → {}",
                human_size(f.before.size),
                human_size(f.after.size)
            ),
        );
    }
    for f in &cs.files_removed {
        row("File removed", "del", &f.path, "");
    }
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Expunge – {name}</title>
<style>
:root{{color-scheme:dark;--bg:#111113;--fg:#ededef;--mut:#a0a0ab;--card:#19191c;--line:#2e2e33;--acc:#6B0700;--add:#3dd68c;--mod:#ffa057;--del:#e05a45}}
body{{border-top:3px solid var(--acc)}}
body{{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 "Segoe UI",system-ui,sans-serif}}
main{{max-width:1100px;margin:0 auto;padding:24px 16px}}
h1{{margin:0 0 4px;font-size:22px}} .meta{{color:var(--mut);margin-bottom:20px}}
.stats{{display:flex;flex-wrap:wrap;gap:8px;margin-bottom:20px}}
.stat{{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:8px 12px}}
.stat b{{display:block;font-size:18px}}
table{{width:100%;border-collapse:collapse;background:var(--card);border:1px solid var(--line);border-radius:8px;overflow:hidden}}
td{{padding:6px 10px;border-top:1px solid var(--line);vertical-align:top}} td.p{{word-break:break-all}}
.tag{{font-size:12px;font-weight:600;white-space:nowrap}} .add{{color:var(--add)}} .mod{{color:var(--mod)}} .del{{color:var(--del)}}
footer{{color:var(--mut);margin-top:16px;font-size:12px}}
</style></head><body><main>
<h1>{name}</h1>
<div class="meta">{publisher} {version} · monitored {created}{installer}</div>
<div class="stats">
<div class="stat"><b>{fa}</b>files added</div><div class="stat"><b>{da}</b>folders added</div>
<div class="stat"><b>{ka}</b>keys added</div><div class="stat"><b>{va}</b>values added</div>
<div class="stat"><b>{vm}</b>values changed</div><div class="stat"><b>{size}</b>disk space</div>
</div>
<table>{rows}</table>
<footer>Generated by Expunge {ver} · GPLv3 · © Sean Shaffer</footer>
</main></body></html>"#,
        name = h(&log.name),
        publisher = h(&log.publisher),
        version = h(&log.version),
        created = h(&log.created),
        installer = if log.installer.is_empty() {
            String::new()
        } else {
            format!(" · {}", h(&log.installer))
        },
        fa = st.files_added,
        da = st.dirs_added,
        ka = st.keys_added,
        va = st.values_added,
        vm = st.values_modified,
        size = human_size(st.bytes_added),
        ver = crate::APP_VERSION,
    )
}

/// Find logs whose changes mention `needle` (case-insensitive substring).
pub fn search_logs<'a>(logs: &'a [InstallLog], needle: &str) -> Vec<(&'a InstallLog, Vec<String>)> {
    let n = needle.to_lowercase();
    if n.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for log in logs {
        let cs = &log.changes;
        let mut hits: Vec<String> = Vec::new();
        let mut check = |s: &str| {
            if hits.len() < 50 && s.to_lowercase().contains(&n) {
                hits.push(s.to_string());
            }
        };
        cs.keys_added.iter().for_each(|k| check(&k.path));
        cs.values_added
            .iter()
            .for_each(|v| check(&format!("{}\\{}", v.key, v.value.name)));
        cs.values_modified
            .iter()
            .for_each(|v| check(&format!("{}\\{}", v.key, v.after.name)));
        cs.files_added.iter().for_each(|f| check(&f.path));
        cs.files_modified.iter().for_each(|f| check(&f.after.path));
        if !hits.is_empty() {
            out.push((log, hits));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> InstallLog {
        InstallLog {
            id: "x".into(),
            name: "Acme <Widget>".into(),
            created: "2026-10-14".into(),
            changes: ChangeSet {
                keys_added: vec![
                    KeyData::new("HKLM\\SOFTWARE\\Acme"),
                    KeyData::new("HKLM\\SOFTWARE\\Acme\\Sub"),
                ],
                values_added: vec![ValueRef {
                    key: "HKCU\\Software\\Shared".into(),
                    value: RegValue::dword("acme", 1),
                }],
                values_modified: vec![ValueChange {
                    key: "HKCU\\Software\\Shared".into(),
                    before: RegValue::string("Path", "C:\\a \"q\""),
                    after: RegValue::string("Path", "C:\\b"),
                }],
                files_added: vec![
                    FileEntry {
                        path: "C:\\Program Files\\Acme".into(),
                        is_dir: true,
                        size: 0,
                        mtime: 0,
                    },
                    FileEntry {
                        path: "C:\\Program Files\\Acme\\it's.exe".into(),
                        is_dir: false,
                        size: 10,
                        mtime: 0,
                    },
                ],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn reg_undo_format() {
        let r = to_reg_undo(&sample());
        assert!(r.starts_with("Windows Registry Editor Version 5.00"));
        assert!(r.contains("[-HKEY_LOCAL_MACHINE\\SOFTWARE\\Acme]"));
        assert!(!r.contains("Acme\\Sub]"));
        assert!(r.contains(
            "[HKEY_CURRENT_USER\\Software\\Shared]\r\n\"acme\"=-\r\n\"Path\"=\"C:\\\\a \\\"q\\\"\""
        ));
        assert_eq!(reg_data(&RegValue::dword("d", 255)), "dword:000000ff");
        assert_eq!(
            reg_data(&RegValue {
                name: "b".into(),
                kind: reg_type::BINARY,
                data: vec![1, 0xab]
            }),
            "hex:01,ab"
        );
    }

    #[test]
    fn ps1_and_html() {
        let ps = to_ps1(&sample());
        assert!(ps.contains("'C:\\Program Files\\Acme\\it''s.exe'"));
        assert!(ps.contains("Registry::HKEY_LOCAL_MACHINE\\SOFTWARE\\Acme"));
        let html = to_html(&sample());
        assert!(html.contains("Acme &lt;Widget&gt;"));
        assert!(html.contains("Value changed"));
        let logs = vec![sample()];
        assert_eq!(search_logs(&logs, "it's.EXE").len(), 1);
        assert!(search_logs(&logs, "nothing").is_empty());
    }
}
