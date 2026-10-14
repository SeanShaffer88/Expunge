//! Command-line interface for scripting and IT use.

use expunge_core::backup::{list_backups, restore_backup, BackupWriter};
use expunge_core::lint;
use expunge_core::model::{human_size, InstallLog};
use expunge_core::platform::{self, Registry};
use expunge_core::revert::{build_plan, execute_plan, ExecOptions};
use expunge_core::revert::{residue, residue_markdown};
use expunge_core::snapshot::{
    detect_program, diff, filter_changes, load_snapshot, save_snapshot, take_snapshot, Progress,
};
use expunge_core::store::Store;
use expunge_core::{programs, report};
use std::path::PathBuf;

const HELP: &str = "\
Expunge — Open Source Install Monitor & Uninstaller
Copyright (C) 2026 Sean Shaffer. GPLv3: free software, no warranty.

USAGE:
  expunge                                   Start the graphical interface
  expunge monitor [--name NAME] -- <installer> [args...]
                                            Snapshot, run the installer, wait,
                                            snapshot again, save an install log
  expunge snapshot <file.snap>              Save a snapshot of the system
  expunge diff <before.snap> <after.snap> [--name NAME]
                                            Compare two snapshots, save a log
  expunge logs [--json]                     List saved install logs
  expunge programs [--all] [--json]         List installed programs
  expunge lint <log-id> [--json] [--strict] Check an installation for packaging
                                            problems (exit 2 on errors, or on
                                            warnings with --strict)
  expunge test-uninstall <log-id> [--quiet] [--no-run] [--timeout SECS] [--json]
                                            Run the program's own uninstaller and
                                            report what it left behind (exit 2
                                            if anything remains)
  expunge revert <log-id> [--dry-run] [--remove-unlogged] [--no-backup]
                                            Undo an installation precisely
  expunge export <log-id> <html|json|reg|ps1> <output-file>
  expunge backups                           List backups
  expunge restore <backup-id>               Undo a removal from its backup
  expunge --version | --help

  monitor and diff also accept --json to print the saved log's summary.

Exit codes: 0 success, 1 error, 2 finished with failures / findings.

Typical CI use (installer QA):
  expunge monitor --name MyApp --no-prompt --json -- dist\\MyApp-Setup.exe /S
  expunge lint <log-id> --strict
  expunge test-uninstall <log-id> --quiet";

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

fn has(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn store() -> anyhow::Result<Store> {
    Store::open()
}

fn save_new_log(
    store: &Store,
    name: &str,
    installer: &str,
    mut changes: expunge_core::model::ChangeSet,
) -> anyhow::Result<InstallLog> {
    let settings = store.load_settings();
    filter_changes(
        &mut changes,
        &settings.registry_exclusions,
        &settings.file_exclusions,
    );
    let mut log = InstallLog {
        id: expunge_core::util::new_id(),
        name: name.to_string(),
        created: expunge_core::util::now_string(),
        installer: installer.to_string(),
        ..Default::default()
    };
    if let Some(d) = detect_program(&changes) {
        if name.is_empty() {
            log.name = d.name;
        }
        log.publisher = d.publisher;
        log.version = d.version;
        log.uninstall_key = d.key;
    }
    if log.name.is_empty() {
        log.name = "Unnamed program".into();
    }
    log.changes = changes;
    store.save_log(&log)?;
    let st = log.changes.stats();
    eprintln!(
        "Saved install log {} for \"{}\": {} files, {} folders, {} keys, {} values added, {} changed ({})",
        log.id,
        log.name,
        st.files_added,
        st.dirs_added,
        st.keys_added,
        st.values_added,
        st.values_modified + st.files_modified,
        human_size(st.bytes_added)
    );
    Ok(log)
}

fn snapshot_now(
    reg: &dyn Registry,
    store: &Store,
) -> anyhow::Result<expunge_core::model::Snapshot> {
    let settings = store.load_settings();
    let p = Progress::default();
    eprintln!("Taking snapshot…");
    let s = take_snapshot(reg, &settings, &p)?;
    eprintln!(
        "  {} registry keys, {} files and folders",
        s.registry.len(),
        s.files.len()
    );
    Ok(s)
}

fn log_summary_json(l: &InstallLog) -> serde_json::Value {
    serde_json::json!({
        "id": l.id,
        "name": l.name,
        "publisher": l.publisher,
        "version": l.version,
        "created": l.created,
        "uninstall_key": l.uninstall_key,
        "stats": l.changes.stats(),
    })
}

pub fn run(args: Vec<String>) -> i32 {
    match run_inner(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

fn run_inner(args: Vec<String>) -> anyhow::Result<i32> {
    let cmd = args.first().map(String::as_str).unwrap_or("--help");
    if !matches!(cmd, "--help" | "-h" | "help" | "--version" | "-V") && !platform::is_elevated() {
        eprintln!("warning: not running as administrator; system-wide changes may fail.");
    }
    let reg = platform::system_registry();
    match cmd {
        "--help" | "-h" | "help" => println!("{HELP}"),
        "--version" | "-V" => println!("Expunge {}", expunge_core::APP_VERSION),
        "snapshot" => {
            let out = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("missing output file"))?;
            let st = store()?;
            let s = snapshot_now(reg.as_ref(), &st)?;
            save_snapshot(&PathBuf::from(out), &s)?;
            println!("Saved {out}");
        }
        "diff" => {
            let (a, b) = (args.get(1), args.get(2));
            let (Some(a), Some(b)) = (a, b) else {
                anyhow::bail!("usage: expunge diff <before> <after>")
            };
            let before = load_snapshot(&PathBuf::from(a))?;
            let after = load_snapshot(&PathBuf::from(b))?;
            let name = arg_value(&args, "--name").unwrap_or_default();
            let log = save_new_log(&store()?, &name, "", diff(&before, &after))?;
            if has(&args, "--json") {
                println!("{}", log_summary_json(&log));
            }
        }
        "monitor" => {
            let sep = args.iter().position(|a| a == "--").ok_or_else(|| {
                anyhow::anyhow!("usage: expunge monitor [--name N] -- <installer> [args]")
            })?;
            let installer: Vec<String> = args[sep + 1..].to_vec();
            if installer.is_empty() {
                anyhow::bail!("no installer given");
            }
            let name = arg_value(&args[..sep], "--name").unwrap_or_default();
            let st = store()?;
            let before = snapshot_now(reg.as_ref(), &st)?;
            let cmdline = installer
                .iter()
                .map(|a| {
                    if a.contains(' ') {
                        format!("\"{a}\"")
                    } else {
                        a.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            eprintln!("Running {cmdline}");
            let child = platform::spawn_command_line(&cmdline)?;
            let code = child.wait();
            eprintln!("Installer exited with code {code}.");
            if !has(&args, "--no-prompt") {
                eprintln!("If the installer is still working in another window, finish it, then press Enter.");
                let mut s = String::new();
                let _ = std::io::stdin().read_line(&mut s);
            }
            let after = snapshot_now(reg.as_ref(), &st)?;
            let log = save_new_log(&st, &name, &installer[0], diff(&before, &after))?;
            if has(&args, "--json") {
                println!("{}", log_summary_json(&log));
            }
        }
        "logs" if has(&args, "--json") => {
            let v: Vec<serde_json::Value> =
                store()?.list_logs().iter().map(log_summary_json).collect();
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        "logs" => {
            for l in store()?.list_logs() {
                let st = l.changes.stats();
                println!(
                    "{}  {}  {}  ({} files, {})",
                    l.id,
                    l.created,
                    l.name,
                    st.files_added,
                    human_size(st.bytes_added)
                );
            }
        }
        "programs" if has(&args, "--json") => {
            let list = programs::list_programs(reg.as_ref(), has(&args, "--all"));
            println!("{}", serde_json::to_string_pretty(&list)?);
        }
        "lint" => {
            let id = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("missing log id"))?;
            let log = store()?.load_log(id)?;
            let findings = lint::lint(&log);
            let (e, w, i) = lint::counts(&findings);
            if has(&args, "--json") {
                println!("{}", serde_json::to_string_pretty(&findings)?);
            } else {
                for f in &findings {
                    println!(
                        "{} {} {}",
                        f.code,
                        f.severity.label().to_uppercase(),
                        f.title
                    );
                    for it in f.items.iter().take(20) {
                        println!("    {it}");
                    }
                }
                println!("{e} errors, {w} warnings, {i} notes");
            }
            let fail = e > 0 || (has(&args, "--strict") && w > 0);
            return Ok(if fail { 2 } else { 0 });
        }
        "test-uninstall" => {
            let id = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("missing log id"))?;
            let log = store()?.load_log(id)?;
            if !has(&args, "--no-run") {
                let key = reg.read_key(&log.uninstall_key).ok_or_else(|| {
                    anyhow::anyhow!("no uninstall entry registered for this program (use --no-run to only check)")
                })?;
                let quiet = key.get_string("QuietUninstallString");
                let cmd = if has(&args, "--quiet") && quiet.is_some() {
                    quiet
                } else {
                    key.get_string("UninstallString")
                }
                .ok_or_else(|| anyhow::anyhow!("the uninstall entry has no UninstallString"))?;
                eprintln!("Running {cmd}");
                let child = platform::spawn_command_line(&platform::expand_env(&cmd))?;
                let code = child.wait();
                eprintln!("Uninstaller exited with code {code}; waiting for it to finish…");
                let timeout: u64 = arg_value(&args, "--timeout")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(600);
                let start = std::time::Instant::now();
                while reg.exists(&log.uninstall_key) && start.elapsed().as_secs() < timeout {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
            let left = residue(&log.changes, reg.as_ref());
            if has(&args, "--json") {
                let items: Vec<serde_json::Value> = left
                    .items
                    .iter()
                    .map(|i| serde_json::json!({ "category": i.action.category(), "item": i.action.describe(), "action": i.action }))
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({ "program": log.name, "clean": left.items.is_empty(), "left_behind": items })
                    )?
                );
            } else {
                print!("{}", residue_markdown(&log.name, &left));
            }
            return Ok(if left.items.is_empty() { 0 } else { 2 });
        }
        "programs" => {
            for p in programs::list_programs(reg.as_ref(), has(&args, "--all")) {
                println!("{}\t{}\t{}\t{}", p.name, p.version, p.publisher, p.key);
            }
        }
        "revert" => {
            let id = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("missing log id"))?;
            let st = store()?;
            let log = st.load_log(id)?;
            let plan = build_plan(&log.changes, reg.as_ref());
            let opts = ExecOptions {
                remove_unlogged: has(&args, "--remove-unlogged"),
                dry_run: has(&args, "--dry-run"),
            };
            let mut writer = if has(&args, "--no-backup") || opts.dry_run {
                None
            } else {
                Some(BackupWriter::new(
                    &st.backups_dir(),
                    &format!("Uninstall {}", log.name),
                )?)
            };
            let results = execute_plan(&plan, reg.as_ref(), writer.as_mut(), &opts);
            for r in &results {
                println!(
                    "[{}] {} — {}",
                    if r.ok { " OK " } else { "FAIL" },
                    r.description,
                    r.message
                );
            }
            for i in &plan.info {
                println!("[note] {i}");
            }
            if let Some(w) = writer {
                let b = w.finish()?;
                println!("Backup: {} (restore with: expunge restore {})", b.id, b.id);
            }
            let failed = results.iter().filter(|r| !r.ok).count();
            if !opts.dry_run && failed == 0 {
                st.delete_log(id)?;
            }
            return Ok(if failed > 0 { 2 } else { 0 });
        }
        "export" => {
            let (Some(id), Some(fmt), Some(out)) = (args.get(1), args.get(2), args.get(3)) else {
                anyhow::bail!("usage: expunge export <log-id> <html|json|reg|ps1> <output-file>")
            };
            let log = store()?.load_log(id)?;
            let content = match fmt.as_str() {
                "html" => report::to_html(&log),
                "reg" => report::to_reg_undo(&log),
                "ps1" => report::to_ps1(&log),
                "json" => serde_json::to_string_pretty(&log)?,
                f => anyhow::bail!("unknown format {f}"),
            };
            if fmt == "reg" {
                let mut bytes = vec![0xFF, 0xFE];
                content
                    .encode_utf16()
                    .for_each(|u| bytes.extend_from_slice(&u.to_le_bytes()));
                std::fs::write(out, bytes)?;
            } else {
                std::fs::write(out, content)?;
            }
            println!("Saved {out}");
        }
        "backups" => {
            for b in list_backups(&store()?.backups_dir()) {
                println!(
                    "{}  {}  {}{}  ({})",
                    b.id,
                    b.created,
                    b.title,
                    if b.restored { " [restored]" } else { "" },
                    b.summary()
                );
            }
        }
        "restore" => {
            let id = args
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("missing backup id"))?;
            let results = restore_backup(&store()?.backups_dir(), id, reg.as_ref())?;
            let failed = results.iter().filter(|r| !r.ok).count();
            for r in &results {
                println!(
                    "[{}] {} — {}",
                    if r.ok { " OK " } else { "FAIL" },
                    r.description,
                    r.message
                );
            }
            return Ok(if failed > 0 { 2 } else { 0 });
        }
        other => {
            eprintln!("unknown command: {other}\n\n{HELP}");
            return Ok(1);
        }
    }
    Ok(0)
}
