//! Command-line interface for scripting and IT use.

use expunge_core::backup::{list_backups, restore_backup, BackupWriter};
use expunge_core::model::{human_size, InstallLog};
use expunge_core::platform::{self, Registry};
use expunge_core::revert::{build_plan, execute_plan, ExecOptions};
use expunge_core::snapshot::{
    detect_program, diff, filter_changes, load_snapshot, save_snapshot, take_snapshot, Progress,
};
use expunge_core::store::Store;
use expunge_core::{programs, report};
use std::path::PathBuf;

const HELP: &str = "\
Expunge — install monitor and complete uninstaller for Windows
Copyright (C) 2026 Sean Shaffer. GPLv3: free software, no warranty.

USAGE:
  expunge                                   Start the graphical interface
  expunge monitor [--name NAME] -- <installer> [args...]
                                            Snapshot, run the installer, wait,
                                            snapshot again, save an install log
  expunge snapshot <file.snap>              Save a snapshot of the system
  expunge diff <before.snap> <after.snap> [--name NAME]
                                            Compare two snapshots, save a log
  expunge logs                              List saved install logs
  expunge programs                          List installed programs
  expunge revert <log-id> [--dry-run] [--remove-unlogged] [--no-backup]
                                            Undo an installation precisely
  expunge export <log-id> <html|json|reg|ps1> <output-file>
  expunge backups                           List backups
  expunge restore <backup-id>               Undo a removal from its backup
  expunge --version | --help

Exit codes: 0 success, 1 error, 2 finished with some failures.";

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
    println!(
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
            save_new_log(&store()?, &name, "", diff(&before, &after))?;
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
            save_new_log(&st, &name, &installer[0], diff(&before, &after))?;
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
