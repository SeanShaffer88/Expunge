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
  expunge logs [--group G] [--tag T] [--json]
                                            List saved install logs
  expunge tag <log-id> [--group G] [--tags a,b]
                                            Set a log's group and tags
  expunge programs [--all] [--trust] [--json]
                                            List installed programs (--trust adds
                                            each program's digital signature)
  expunge verify <file>                     Check a file's digital signature
                                            (exit 2 if unsigned or invalid)
  expunge lint <log-id> [--json] [--strict] Check an installation for packaging
                                            problems (exit 2 on errors, or on
                                            warnings with --strict)
  expunge test-uninstall <log-id> [--quiet] [--no-run] [--timeout SECS] [--json]
                                            Run the program's own uninstaller and
                                            report what it left behind (exit 2
                                            if anything remains)
  expunge revert <log-id> [--dry-run] [--remove-unlogged] [--no-backup] [--recycle]
                                            Undo an installation precisely
  expunge export <log-id> <html|json|reg|ps1> <output-file>
  expunge learn-noise [--seconds N] [--apply] [--json]
                                            Take two snapshots N seconds apart
                                            (default 60) and list what changed
                                            on its own; --apply adds it to the
                                            exclusions
  expunge updates [--json]                  Programs with updates (needs winget)
  expunge update <winget-id> | --all        Install updates silently with winget
  expunge transfer export <out.expunge> <program>...
                                            Pack programs (by name) with their files
                                            and settings to move to another PC
  expunge transfer check <package>          Check a package against this PC
  expunge transfer import <package> [--overwrite]
                                            Restore a package (undo from backups)
  expunge repackage <log-id> <folder> [--msix]
                                            Turn a monitored installation into a
                                            folder with files, install.reg and
                                            silent install/uninstall scripts
  expunge orphans [--json]                  List registrations left by removed
                                            programs (pointing to missing files)
  expunge disk [folder] [--top N] [--json]  Largest folders and files under a
                                            folder or drive (default: C:\\)
  expunge duplicates <folder>... [--min-size MB] [--json]
                                            List identical files (compared by
                                            content); nothing is deleted
  expunge uninstall <name>... [--silent] [--leftovers safe|none] [--dry-run]
                                            Uninstall programs one after another,
                                            then remove safe leftovers (backed up)
  expunge clean [--categories a,b] [--list] [--dry-run]
                                            Clean junk (default: the safe kinds)
  expunge schedule-clean daily|weekly|monthly|off [--categories a,b]
                                            Clean junk automatically
  expunge startup [--json]                  List startup programs
  expunge startup enable|disable <name>     Turn a startup program on or off
  expunge reputation <file>... [--vt-key KEY]
                                            Look files up online by their hash
                                            (only the hash is sent)
  expunge watch                             Background auto-monitoring (started
                                            at sign-in when turned on in Settings)
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

/// Find one program by name: exact (ignoring case) first, then a unique
/// partial match.
fn pick_program<'a>(
    progs: &'a [expunge_core::programs::InstalledProgram],
    name: &str,
) -> anyhow::Result<&'a expunge_core::programs::InstalledProgram> {
    if let Some(p) = progs.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
        return Ok(p);
    }
    let low = name.to_lowercase();
    let hits: Vec<_> = progs.iter().filter(|p| p.name.to_lowercase().contains(&low)).collect();
    match hits.len() {
        0 => anyhow::bail!("no installed program matches"),
        1 => Ok(hits[0]),
        _ => anyhow::bail!(
            "several programs match: {}",
            hits.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// Where scheduled cleaning records what it freed.
fn st_log_path() -> anyhow::Result<PathBuf> {
    Ok(store()?.root.join("cleaning.log"))
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
        &settings.effective_registry_exclusions(),
        &settings.effective_file_exclusions(),
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
        eprintln!("warning: not running as Administrator; system-wide changes may fail.");
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
            let group = arg_value(&args, "--group");
            let tag = arg_value(&args, "--tag");
            let logs: Vec<_> = store()?
                .list_logs()
                .into_iter()
                .filter(|l| group.as_ref().is_none_or(|g| l.group.eq_ignore_ascii_case(g)))
                .filter(|l| tag.as_ref().is_none_or(|t| l.tags.iter().any(|x| x.eq_ignore_ascii_case(t))))
                .collect();
            if has(&args, "--json") {
                let items: Vec<_> = logs
                    .iter()
                    .map(|l| {
                        let st = l.changes.stats();
                        serde_json::json!({
                            "id": l.id, "name": l.name, "publisher": l.publisher,
                            "version": l.version, "created": l.created,
                            "group": l.group, "tags": l.tags,
                            "files_added": st.files_added, "bytes_added": st.bytes_added,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else {
                for l in &logs {
                    let st = l.changes.stats();
                    let mut extra = String::new();
                    if !l.group.is_empty() {
                        extra.push_str(&format!("  [{}]", l.group));
                    }
                    if !l.tags.is_empty() {
                        extra.push_str(&format!("  #{}", l.tags.join(" #")));
                    }
                    println!(
                        "{}  {}  {}  ({} files, {}){extra}",
                        l.id,
                        l.created,
                        l.name,
                        st.files_added,
                        human_size(st.bytes_added)
                    );
                }
            }
        }
        "tag" => {
            let id = args.get(1).ok_or_else(|| anyhow::anyhow!("usage: expunge tag <log-id> [--group G] [--tags a,b]"))?;
            let st = store()?;
            let mut log = st.load_log(id)?;
            if let Some(g) = arg_value(&args, "--group") {
                log.group = g.trim().to_string();
            }
            if let Some(t) = arg_value(&args, "--tags") {
                log.tags = expunge_core::model::parse_tags(&t);
            }
            st.save_log(&log)?;
            println!("{}: group \"{}\", tags [{}]", log.name, log.group, log.tags.join(", "));
        }
        "updates" => {
            let list = expunge_core::winget::list_upgrades()?;
            if has(&args, "--json") {
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else if list.is_empty() {
                println!("Everything is up to date.");
            } else {
                for u in &list {
                    println!("{}  {} → {}  ({})", u.name, u.version, u.available, u.id);
                }
            }
        }
        "update" => {
            let ids: Vec<String> = if has(&args, "--all") {
                expunge_core::winget::list_upgrades()?
                    .into_iter()
                    .map(|u| u.id)
                    .collect()
            } else {
                args.iter()
                    .skip(1)
                    .filter(|a| !a.starts_with("--"))
                    .cloned()
                    .collect()
            };
            if ids.is_empty() {
                anyhow::bail!("usage: expunge update <winget-id>... | --all");
            }
            let mut failed = 0;
            for id in &ids {
                eprint!("Updating {id}… ");
                match expunge_core::winget::upgrade(id) {
                    Ok(()) => eprintln!("done"),
                    Err(e) => {
                        failed += 1;
                        eprintln!("{e}");
                    }
                }
            }
            return Ok(if failed == 0 { 0 } else { 2 });
        }
        "repackage" => {
            let (Some(id), Some(dir)) = (args.get(1), args.get(2)) else {
                anyhow::bail!("usage: expunge repackage <log-id> <folder> [--msix]")
            };
            let st = store()?;
            let log = st.load_log(id)?;
            let s = expunge_core::repackage::repackage(
                &log,
                reg.as_ref(),
                &PathBuf::from(dir),
                &|_| {},
            )?;
            println!(
                "Saved {}: {} files ({}), {} keys, {} values",
                s.folder.display(),
                s.files,
                human_size(s.bytes),
                s.keys,
                s.values
            );
            if has(&args, "--msix") {
                let m = expunge_core::msix::build(&s.folder, &log)?;
                match &m.package {
                    Some(p) => println!("MSIX package: {} (unsigned; see MSIX-README.txt)", p.display()),
                    None => println!("MSIX layout: {} (pack with MakeAppx; see MSIX-README.txt)", m.layout.display()),
                }
            }
            if s.missing > 0 {
                eprintln!(
                    "{} recorded files no longer exist and were left out.",
                    s.missing
                );
            }
        }
        "transfer" => {
            let sub = args.get(1).map(String::as_str).unwrap_or("");
            let st = store()?;
            match sub {
                "export" => {
                    let out = args
                        .get(2)
                        .ok_or_else(|| anyhow::anyhow!("missing output file"))?;
                    let wanted: Vec<String> = args
                        .iter()
                        .skip(3)
                        .filter(|a| !a.starts_with("--"))
                        .map(|a| a.to_lowercase())
                        .collect();
                    if wanted.is_empty() {
                        anyhow::bail!("name at least one program");
                    }
                    let all = programs::list_programs(reg.as_ref(), false);
                    let chosen: Vec<_> = all
                        .into_iter()
                        .filter(|p| wanted.iter().any(|w| p.name.to_lowercase().contains(w)))
                        .collect();
                    if chosen.is_empty() {
                        anyhow::bail!("no installed program matches");
                    }
                    let logs = st.list_logs();
                    let changes: Vec<_> = chosen
                        .iter()
                        .map(|p| programs::log_for(&logs, p).map(|l| l.changes.clone()))
                        .collect();
                    let c =
                        expunge_core::transfer::collect(&chosen, &changes, reg.as_ref(), &|s| {
                            eprintln!("  {s}")
                        });
                    let m = expunge_core::transfer::export(
                        &c,
                        reg.as_ref(),
                        &PathBuf::from(out),
                        &|_| {},
                    )?;
                    for w in &m.warnings {
                        eprintln!("warning: {w}");
                    }
                    println!(
                        "Saved {out}: {} programs, {} files ({}), {} registry entries",
                        m.programs.len(),
                        m.files.len(),
                        human_size(m.bytes()),
                        m.keys.len() + m.values.len()
                    );
                }
                "check" | "import" => {
                    let pkg = PathBuf::from(
                        args.get(2)
                            .ok_or_else(|| anyhow::anyhow!("missing package"))?,
                    );
                    let m = expunge_core::transfer::read_manifest(&pkg)?;
                    let c = expunge_core::transfer::check(
                        &m,
                        reg.as_ref(),
                        &expunge_core::transfer::Vars::current(),
                    );
                    println!("From {} ({}), {}", m.computer, m.user, m.created);
                    for p in &m.programs {
                        println!("  {} {}", p.name, p.version);
                    }
                    for p in &c.problems {
                        println!("problem: {p}");
                    }
                    for w in &c.warnings {
                        println!("warning: {w}");
                    }
                    if !c.conflicts.is_empty() {
                        println!(
                            "{} files already exist (use --overwrite to replace them)",
                            c.conflicts.len()
                        );
                    }
                    if sub == "check" || !c.problems.is_empty() {
                        return Ok(if c.problems.is_empty() { 0 } else { 2 });
                    }
                    let names: Vec<String> = m.programs.iter().map(|p| p.name.clone()).collect();
                    let mut bw = BackupWriter::new(
                        &st.backups_dir(),
                        &format!("Transfer: {}", names.join(", ")),
                    )?;
                    let res = expunge_core::transfer::restore(
                        &pkg,
                        reg.as_ref(),
                        &mut bw,
                        has(&args, "--overwrite"),
                        &|s| eprintln!("  {s}"),
                    )?;
                    let b = bw.finish()?;
                    let failed = res.iter().filter(|r| !r.ok).count();
                    for r in res.iter().filter(|r| !r.ok) {
                        eprintln!("failed: {} — {}", r.description, r.message);
                    }
                    println!("Restored. Undo with: expunge restore {}", b.id);
                    return Ok(if failed == 0 { 0 } else { 2 });
                }
                _ => anyhow::bail!("usage: expunge transfer export|check|import …"),
            }
        }
        "disk" => {
            let folder = args
                .get(1)
                .filter(|a| !a.starts_with("--"))
                .cloned()
                .unwrap_or_else(|| {
                    if cfg!(windows) {
                        "C:\\".into()
                    } else {
                        "/".into()
                    }
                });
            let top: usize = arg_value(&args, "--top")
                .and_then(|v| v.parse().ok())
                .unwrap_or(20);
            let progress = expunge_core::snapshot::Progress::default();
            let map = expunge_core::diskmap::scan(std::path::Path::new(&folder), &progress);
            let entries = expunge_core::diskmap::largest(&map, top);
            if has(&args, "--json") {
                let items: Vec<_> = entries
                    .iter()
                    .map(|(path, size, dir)| {
                        serde_json::json!({ "path": path, "bytes": size, "folder": dir })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({
                        "root": folder,
                        "bytes": map.root.size,
                        "files": map.root.files,
                        "unreadable_folders": map.unreadable,
                        "largest": items,
                    })
                );
            } else {
                println!(
                    "{}: {} in {} files",
                    folder,
                    expunge_core::model::human_size(map.root.size),
                    map.root.files
                );
                for (path, size, dir) in &entries {
                    println!(
                        "{:>10}  {}{}",
                        expunge_core::model::human_size(*size),
                        path,
                        if *dir {
                            std::path::MAIN_SEPARATOR_STR
                        } else {
                            ""
                        }
                    );
                }
                if map.unreadable > 0 {
                    eprintln!("{} folders could not be read.", map.unreadable);
                }
            }
        }
        "duplicates" => {
            let mut roots = Vec::new();
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--min-size" {
                    i += 2;
                    continue;
                }
                if !args[i].starts_with("--") {
                    roots.push(std::path::PathBuf::from(&args[i]));
                }
                i += 1;
            }
            if roots.is_empty() {
                anyhow::bail!("name at least one folder to search");
            }
            let min_mb: f64 = arg_value(&args, "--min-size")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.0);
            let progress = expunge_core::snapshot::Progress::default();
            let r = expunge_core::duplicates::find(&roots, (min_mb * 1048576.0) as u64, &progress);
            if has(&args, "--json") {
                let groups: Vec<_> = r
                    .groups
                    .iter()
                    .map(|g| {
                        serde_json::json!({
                            "bytes": g.size,
                            "sha256": g.hash,
                            "files": g.files.iter().map(|f| f.path.to_string_lossy().to_string()).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({ "files_checked": r.files_scanned, "wasted_bytes": r.wasted(), "groups": groups })
                );
            } else if r.groups.is_empty() {
                println!(
                    "No duplicate files found ({} files checked).",
                    r.files_scanned
                );
            } else {
                for g in &r.groups {
                    println!(
                        "{} copies of {}:",
                        g.files.len(),
                        expunge_core::model::human_size(g.size)
                    );
                    for f in &g.files {
                        println!("  {}", f.path.display());
                    }
                }
                eprintln!(
                    "{} sets; keeping one copy of each would free {}. Remove copies from the app (Tools > Duplicate files), which keeps one copy and uses the Recycle Bin.",
                    r.groups.len(),
                    expunge_core::model::human_size(r.wasted())
                );
            }
        }
        "uninstall" => {
            let names: Vec<&String> = args[1..].iter().filter(|a| !a.starts_with("--")).collect::<Vec<_>>();
            let names: Vec<String> = {
                // Drop values that belong to flags.
                let mut out = Vec::new();
                let mut skip = false;
                for a in &args[1..] {
                    if skip { skip = false; continue; }
                    if a == "--leftovers" { skip = true; continue; }
                    if !a.starts_with("--") { out.push(a.clone()); }
                }
                let _ = names;
                out
            };
            if names.is_empty() {
                anyhow::bail!("name at least one program (see `expunge programs`)");
            }
            let silent_mode = has(&args, "--silent");
            let dry = has(&args, "--dry-run");
            let leftovers = arg_value(&args, "--leftovers").unwrap_or_else(|| "safe".into());
            let st = store()?;
            let mut failures = 0;
            let mut reboot = false;
            for name in &names {
                let progs = programs::list_programs(reg.as_ref(), false);
                let p = match pick_program(&progs, name) {
                    Ok(p) => p.clone(),
                    Err(e) => {
                        eprintln!("{name}: {e}");
                        failures += 1;
                        continue;
                    }
                };
                let engine = expunge_core::silent::detect(&p);
                let cmd = if silent_mode {
                    expunge_core::silent::silent_command(&p, engine)
                } else {
                    p.uninstall_command(false)
                };
                let Some(cmd) = cmd else {
                    eprintln!("{}: no {}uninstaller available", p.name, if silent_mode { "silent " } else { "" });
                    failures += 1;
                    continue;
                };
                println!("Uninstalling {} ({})…", p.name, engine.label());
                if dry {
                    println!("  would run: {cmd}");
                    continue;
                }
                while platform::installer_busy() {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
                let code = platform::spawn_command_line(&cmd)?.wait();
                match expunge_core::silent::exit_meaning(engine, code) {
                    expunge_core::silent::ExitMeaning::RebootRequired => reboot = true,
                    expunge_core::silent::ExitMeaning::Success => {}
                    m => {
                        eprintln!("  the uninstaller reported {m:?} (exit code {code})");
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
                if reg.exists(&p.key) {
                    eprintln!("  {} is still installed; skipping the leftover scan", p.name);
                    failures += 1;
                    continue;
                }
                if leftovers == "none" {
                    continue;
                }
                let mut plan = expunge_core::leftovers::find_leftovers(
                    &p,
                    reg.as_ref(),
                    &expunge_core::leftovers::LeftoverScope::default(),
                );
                // Unattended: only items labelled Safe.
                for i in &mut plan.items {
                    if expunge_core::revert::risk(i) != expunge_core::revert::Risk::Safe {
                        i.selected = false;
                    }
                }
                if plan.selected() == 0 {
                    println!("  no leftovers found");
                    continue;
                }
                let mut w = BackupWriter::new(&st.backups_dir(), &format!("Leftovers of {}", p.name))?;
                let results = execute_plan(&plan, reg.as_ref(), Some(&mut w), &ExecOptions::default());
                let b = w.finish()?;
                let ok = results.iter().filter(|r| r.ok).count();
                println!("  removed {ok} leftovers (backup {}; restore with: expunge restore {})", b.id, b.id);
                if ok < results.len() {
                    failures += 1;
                }
            }
            if reboot {
                println!("Restart Windows to finish removing everything.");
            }
            return Ok(if failures > 0 { 2 } else { 0 });
        }
        "clean" => {
            let cats = expunge_core::cleaner::categories();
            if has(&args, "--list") {
                for c in &cats {
                    let r = expunge_core::cleaner::scan(c);
                    println!(
                        "{:<20} {:>10}  {}{}",
                        c.id,
                        human_size(r.bytes),
                        if c.default_on && !expunge_core::cleaner::is_advanced(c.id) { "default" } else { "" },
                        if c.needs_admin { " (administrator)" } else { "" }
                    );
                }
                return Ok(0);
            }
            let wanted: Vec<String> = match arg_value(&args, "--categories") {
                Some(v) => v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                None => cats
                    .iter()
                    .filter(|c| c.default_on && !expunge_core::cleaner::is_advanced(c.id))
                    .map(|c| c.id.to_string())
                    .collect(),
            };
            let elevated = platform::is_elevated();
            let mut total = 0u64;
            for id in &wanted {
                let Some(c) = cats.iter().find(|c| c.id == id) else {
                    eprintln!("unknown category: {id} (see `expunge clean --list`)");
                    continue;
                };
                if c.needs_admin && !elevated {
                    eprintln!("{id}: skipped (needs administrator rights)");
                    continue;
                }
                let r = if has(&args, "--dry-run") {
                    expunge_core::cleaner::scan(c)
                } else {
                    expunge_core::cleaner::clean(c)
                };
                total += r.bytes;
                if !has(&args, "--scheduled") {
                    println!("{:<20} {:>10}  {} files", id, human_size(r.bytes), r.files);
                }
            }
            if has(&args, "--scheduled") {
                // Leave a trace for the user in the data folder.
                let line = format!("{}  cleaned {}\n", expunge_core::util::now_string(), human_size(total));
                let path = st_log_path()?;
                let mut old = std::fs::read_to_string(&path).unwrap_or_default();
                old.push_str(&line);
                let keep: Vec<&str> = old.lines().rev().take(100).collect();
                let text: String = keep.into_iter().rev().map(|l| format!("{l}\n")).collect();
                let _ = std::fs::write(&path, text);
            } else {
                println!("Total: {}", human_size(total));
            }
        }
        "schedule-clean" => {
            let what = args.get(1).map(|s| s.as_str()).unwrap_or("");
            if what == "off" {
                expunge_core::schedule::remove()?;
                println!("Automatic cleaning turned off.");
                return Ok(0);
            }
            let freq = match what {
                "daily" => expunge_core::schedule::Frequency::Daily,
                "weekly" => expunge_core::schedule::Frequency::Weekly,
                "monthly" => expunge_core::schedule::Frequency::Monthly,
                _ => anyhow::bail!("usage: expunge schedule-clean daily|weekly|monthly|off [--categories a,b]"),
            };
            let cats: Vec<String> = match arg_value(&args, "--categories") {
                Some(v) => v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                None => expunge_core::cleaner::categories()
                    .iter()
                    .filter(|c| c.default_on && !expunge_core::cleaner::is_advanced(c.id))
                    .map(|c| c.id.to_string())
                    .collect(),
            };
            let refs: Vec<&str> = cats.iter().map(|s| s.as_str()).collect();
            expunge_core::schedule::set(&refs, freq, platform::is_elevated())?;
            println!("Junk will be cleaned {} ({}).", what, cats.join(", "));
        }
        "startup" => {
            let entries = expunge_core::startup::list_startup(reg.as_ref());
            match args.get(1).map(|s| s.as_str()) {
                Some(op @ ("enable" | "disable")) => {
                    let name = args.get(2).ok_or_else(|| anyhow::anyhow!("name the startup program"))?;
                    let e = entries
                        .iter()
                        .find(|e| e.name.eq_ignore_ascii_case(name))
                        .ok_or_else(|| anyhow::anyhow!("no startup program called {name}"))?;
                    if op == "disable" && expunge_core::startup::is_essential(e) {
                        anyhow::bail!("{} is needed by Windows, security software or hardware and can't be disabled", e.name);
                    }
                    let st = store()?;
                    let mut w = BackupWriter::new(&st.backups_dir(), &format!("Startup: {} {}", op, e.name))?;
                    expunge_core::startup::set_enabled(reg.as_ref(), e, op == "enable", Some(&mut w))?;
                    w.finish()?;
                    println!("{} {}d.", e.name, op);
                }
                _ => {
                    if has(&args, "--json") {
                        let items: Vec<_> = entries
                            .iter()
                            .map(|e| serde_json::json!({
                                "name": e.name, "command": e.command, "location": e.location,
                                "enabled": e.enabled, "all_users": e.all_users, "missing": e.missing,
                            }))
                            .collect();
                        println!("{}", serde_json::to_string_pretty(&items)?);
                    } else {
                        let boot = expunge_core::boot::read();
                        if let Some(ms) = boot.as_ref().and_then(|b| b.last_boot_ms) {
                            println!("Last startup took {:.0} s.", ms as f64 / 1000.0);
                        }
                        for e in &entries {
                            let impact = boot
                                .as_ref()
                                .and_then(|b| b.impact_of(&e.command))
                                .map(|i| format!("  [impact: {}, {} ms]", i.level().label(), i.avg_ms))
                                .unwrap_or_default();
                            println!(
                                "[{}] {}{}{impact}  {}",
                                if e.enabled { "on " } else { "off" },
                                e.name,
                                if e.missing { " (missing)" } else { "" },
                                e.command
                            );
                        }
                    }
                }
            }
        }
        "reputation" => {
            let files: Vec<&String> = {
                let mut v = Vec::new();
                let mut skip = false;
                for a in &args[1..] {
                    if skip { skip = false; continue; }
                    if a == "--vt-key" { skip = true; continue; }
                    v.push(a);
                }
                v
            };
            if files.is_empty() {
                anyhow::bail!("name at least one file");
            }
            let key = arg_value(&args, "--vt-key").unwrap_or_else(|| store().map(|s| s.load_settings().virustotal_key).unwrap_or_default());
            let mut bad = false;
            for f in files {
                let hash = expunge_core::reputation::sha256_file(std::path::Path::new(f))?;
                match expunge_core::reputation::check(&hash, Some(&key)) {
                    Ok(v) => {
                        bad |= v.is_bad();
                        println!("{f}\n  sha256 {hash}\n  {v:?}\n  {}", expunge_core::reputation::virustotal_url(&hash));
                    }
                    Err(e) => println!("{f}\n  sha256 {hash}\n  lookup failed: {e}"),
                }
            }
            return Ok(if bad { 2 } else { 0 });
        }
        "watch" => {
            #[cfg(windows)]
            platform::windows::background_priority();
            let st = store()?;
            let base_path = st.snapshots_dir().join("auto-baseline.snap");
            let poll = std::time::Duration::from_secs(30);
            let take_baseline = |st: &Store| -> anyhow::Result<()> {
                let s = snapshot_now(reg.as_ref(), st)?;
                save_snapshot(&base_path, &s)?;
                Ok(())
            };
            while platform::installer_busy() {
                std::thread::sleep(poll);
            }
            take_baseline(&st)?;
            let mut keys = expunge_core::autowatch::uninstall_keys(reg.as_ref());
            let mut base_time = std::time::Instant::now();
            let mut stale = false;
            loop {
                std::thread::sleep(poll);
                // Manual monitoring records this installation itself.
                if st.pending().is_some() {
                    stale = true;
                    keys = expunge_core::autowatch::uninstall_keys(reg.as_ref());
                    continue;
                }
                let now = expunge_core::autowatch::uninstall_keys(reg.as_ref());
                let new = expunge_core::autowatch::added(&keys, &now);
                if new.is_empty() {
                    if now != keys {
                        // Something was uninstalled: start from a fresh baseline.
                        keys = now;
                        stale = true;
                    }
                    if (stale || base_time.elapsed() > std::time::Duration::from_secs(24 * 3600))
                        && !platform::installer_busy()
                    {
                        take_baseline(&st)?;
                        keys = expunge_core::autowatch::uninstall_keys(reg.as_ref());
                        base_time = std::time::Instant::now();
                        stale = false;
                    }
                    continue;
                }
                // Wait for the installation to settle: no installer running
                // and the program list unchanged for two checks (max. 30 min).
                let started = std::time::Instant::now();
                let mut last = now;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(20));
                    let again = expunge_core::autowatch::uninstall_keys(reg.as_ref());
                    let settled = again == last && !platform::installer_busy();
                    last = again;
                    if settled || started.elapsed() > std::time::Duration::from_secs(1800) {
                        break;
                    }
                }
                let before = load_snapshot(&base_path)?;
                let after = snapshot_now(reg.as_ref(), &st)?;
                let mut log = save_new_log(&st, "", "", diff(&before, &after))?;
                log.tags = vec![expunge_core::autowatch::TAG.to_string()];
                log.notes = "Recorded automatically by background monitoring.".into();
                st.save_log(&log)?;
                eprintln!("Recorded {}", log.name);
                save_snapshot(&base_path, &after)?;
                keys = last;
                base_time = std::time::Instant::now();
                stale = false;
            }
        }
        "orphans" => {
            let plan = expunge_core::orphans::scan_system(reg.as_ref());
            if has(&args, "--json") {
                let items: Vec<_> = plan
                    .items
                    .iter()
                    .map(|i| {
                        serde_json::json!({
                            "category": i.action.category(),
                            "kind": i.note,
                            "action": i.action.describe(),
                            "risk": format!("{:?}", expunge_core::revert::risk(i)),
                        })
                    })
                    .collect();
                println!("{}", serde_json::Value::Array(items));
            } else if plan.items.is_empty() {
                println!("No leftovers of removed programs found.");
            } else {
                for i in &plan.items {
                    println!("[{}] {}", i.note, i.action.describe());
                }
                eprintln!(
                    "{} items. Remove them from the app (Tools → Find leftovers of removed programs), where everything is backed up first.",
                    plan.items.len()
                );
            }
            return Ok(if plan.items.is_empty() { 0 } else { 2 });
        }
        "learn-noise" => {
            let secs: u64 = arg_value(&args, "--seconds")
                .and_then(|s| s.parse().ok())
                .unwrap_or(60);
            let st = store()?;
            let before = snapshot_now(reg.as_ref(), &st)?;
            eprintln!("Waiting {secs} s; don't install or change anything…");
            std::thread::sleep(std::time::Duration::from_secs(secs));
            let after = snapshot_now(reg.as_ref(), &st)?;
            let mut settings = st.load_settings();
            let roots: Vec<String> = settings.file_roots.iter().map(|r| r.path.clone()).collect();
            let l = expunge_core::noise::learn(
                &diff(&before, &after),
                &roots,
                &settings.registry_roots,
                &settings.effective_file_exclusions(),
                &settings.effective_registry_exclusions(),
            );
            if has(&args, "--json") {
                println!(
                    "{}",
                    serde_json::json!({ "registry": l.registry, "files": l.files })
                );
            } else if l.is_empty() {
                println!("Nothing changed on its own.");
            } else {
                for p in l.files.iter().chain(&l.registry) {
                    println!("{p}");
                }
            }
            if has(&args, "--apply") && !l.is_empty() {
                let date = chrono::Local::now().format("%Y-%m-%d").to_string();
                expunge_core::noise::append_learned(
                    &mut settings.registry_exclusions,
                    &l.registry,
                    &date,
                );
                expunge_core::noise::append_learned(&mut settings.file_exclusions, &l.files, &date);
                st.save_settings(&settings)?;
                eprintln!("Added to the exclusions.");
            }
        }
        "verify" => {
            let file = args.get(1).ok_or_else(|| anyhow::anyhow!("missing file"))?;
            let tr = expunge_core::trust::check_file(file);
            match &tr {
                expunge_core::trust::Trust::Signed { signer } => {
                    println!("Signed by {signer} (valid)")
                }
                expunge_core::trust::Trust::Invalid { signer, reason } => {
                    println!("Signature problem: {reason} {signer}")
                }
                other => println!("{}", other.label()),
            }
            return Ok(if matches!(tr, expunge_core::trust::Trust::Signed { .. }) {
                0
            } else {
                2
            });
        }
        "programs" if has(&args, "--json") => {
            let list = programs::list_programs(reg.as_ref(), has(&args, "--all"));
            if has(&args, "--trust") {
                let v: Vec<serde_json::Value> =
                    list.iter()
                        .map(|p| {
                            let mut j = serde_json::to_value(p).unwrap_or_default();
                            j["signature"] = serde_json::to_value(
                                expunge_core::trust::check_program(p, reg.as_ref()),
                            )
                            .unwrap_or_default();
                            j
                        })
                        .collect();
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&list)?);
            }
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
            let with_trust = has(&args, "--trust");
            for p in programs::list_programs(reg.as_ref(), has(&args, "--all")) {
                if with_trust {
                    let tr = expunge_core::trust::check_program(&p, reg.as_ref());
                    let sig = match &tr {
                        expunge_core::trust::Trust::Signed { signer } => {
                            format!("signed: {signer}")
                        }
                        expunge_core::trust::Trust::Invalid { reason, .. } => {
                            format!("INVALID: {reason}")
                        }
                        other => other.label().to_lowercase(),
                    };
                    println!("{}\t{}\t{}\t{}", p.name, p.version, p.publisher, sig);
                } else {
                    println!("{}\t{}\t{}\t{}", p.name, p.version, p.publisher, p.key);
                }
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
                use_recycle_bin: has(&args, "--recycle"),
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
