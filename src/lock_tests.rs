//! Locked files on real Windows: a file another program has open, a DLL a
//! process has loaded (as Explorer does with shell extensions), and a
//! service that keeps its files open until its process has exited.
//!
//! The holders are played by `examples/lockholder.rs`; build it first with
//! `cargo build --example lockholder` (CI does). The service test needs an
//! elevated prompt and is skipped without one.

use crate::locks::{self, HolderKind, ServiceStop};
use crate::platform::{self, MemRegistry};
use crate::revert::{execute_plan_with, Action, ExecOptions, Plan, PlanItem};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn helper() -> Option<PathBuf> {
    // target/<profile>/deps/expunge_core-<hash>.exe -> target/<profile>/examples/
    let exe = std::env::current_exe().ok()?;
    let p = exe
        .parent()?
        .parent()?
        .join("examples")
        .join("lockholder.exe");
    if p.is_file() {
        return Some(p);
    }
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        panic!(
            "{} is missing: run `cargo build --example lockholder` first",
            p.display()
        );
    }
    eprintln!("skipped: build the helper with `cargo build --example lockholder`");
    None
}

/// A helper process, killed when the test ends however it ends.
struct Holder(Child);

impl Holder {
    fn start(helper: &Path, args: &[&str]) -> Holder {
        let mut child = Command::new(helper)
            .args(args)
            .stdout(Stdio::piped())
            .spawn()
            .expect("start lockholder");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(
            line.trim(),
            "ready",
            "lockholder did not get hold of the file"
        );
        Holder(child)
    }
    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn plan(actions: Vec<Action>) -> Plan {
    Plan {
        items: actions
            .into_iter()
            .map(|action| PlanItem {
                action,
                selected: true,
                exists: true,
                note: String::new(),
            })
            .collect(),
        info: Vec::new(),
    }
}

fn run(actions: Vec<Action>, opts: &ExecOptions) -> Vec<crate::backup::ActionResult> {
    execute_plan_with(&plan(actions), &MemRegistry::new(), None, opts, &[])
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A real DLL to load under another name: any small system one will do.
fn copy_system_dll(to: &Path) {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    std::fs::copy(Path::new(&root).join("System32").join("version.dll"), to).unwrap();
}

#[test]
fn a_file_open_in_another_program_names_that_program() {
    let Some(helper) = helper() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("settings.db");
    std::fs::write(&f, b"data").unwrap();
    let h = Holder::start(&helper, &["open", &path_str(&f), "120"]);

    let holders = locks::file_holders(&[&f]);
    assert!(
        holders
            .iter()
            .any(|x| x.pid == h.pid() && x.kind == HolderKind::Program),
        "{holders:?}"
    );

    let res = run(
        vec![Action::DeleteFile { path: path_str(&f) }],
        &ExecOptions::default(),
    );
    assert_eq!(res.len(), 1);
    // Scheduled for the next restart when elevated, refused otherwise; either
    // way the user is told who has it open.
    let pid = format!("(process {})", h.pid());
    assert!(res[0].message.contains("held by"), "{res:?}");
    assert!(res[0].message.contains(&pid), "{res:?}");
    assert!(f.exists(), "an open file can't be gone yet");
}

#[test]
fn a_dll_loaded_by_another_process_is_reported_and_the_rest_removed() {
    let Some(helper) = helper() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("App");
    std::fs::create_dir_all(app.join("shell")).unwrap();
    std::fs::write(app.join("readme.txt"), b"x").unwrap();
    let dll = app.join("shell").join("ContextMenu64.dll");
    copy_system_dll(&dll);
    let h = Holder::start(&helper, &["load", &path_str(&dll), "120"]);

    assert!(
        locks::file_holders(&[&dll])
            .iter()
            .any(|x| x.pid == h.pid()),
        "a loaded DLL is reported like an open file"
    );
    let res = run(
        vec![Action::DeleteTree {
            path: path_str(&app),
        }],
        &ExecOptions {
            restart_explorer: true,
            ..Default::default()
        },
    );
    // Not Explorer, so nothing was closed: one result, for the folder.
    assert_eq!(res.len(), 1, "{res:?}");
    assert!(
        !app.join("readme.txt").exists(),
        "unlocked files go at once"
    );
    assert!(dll.exists());
    if platform::windows::is_elevated() {
        assert!(res[0].ok, "{res:?}");
        assert!(res[0].message.contains("held by"), "{res:?}");
        assert!(res[0].message.contains("next restart"), "{res:?}");
    }
}

#[test]
fn only_explorer_is_ever_closed() {
    let Some(helper) = helper() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dll = tmp.path().join("Ext.dll");
    copy_system_dll(&dll);
    let mut h = Holder::start(&helper, &["load", &path_str(&dll), "120"]);
    assert!(locks::pause_explorer(&[&dll]).is_none());
    assert!(
        h.0.try_wait().unwrap().is_none(),
        "a program holding the file must keep running"
    );
}

/// A test service, removed when the test ends however it ends.
struct TestService(String);

impl Drop for TestService {
    fn drop(&mut self) {
        let _ = platform::run_hidden("sc.exe", &["stop", &self.0]);
        let _ = platform::run_hidden("sc.exe", &["delete", &self.0]);
    }
}

#[test]
fn a_service_is_stopped_and_its_files_released_before_they_are_deleted() {
    let Some(helper) = helper() else { return };
    if !platform::windows::is_elevated() {
        eprintln!("skipped: creating a service needs an elevated prompt");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Svc");
    std::fs::create_dir_all(&dir).unwrap();
    // The service runs from the program's own folder, as real ones do, so
    // its executable is one of the files to remove.
    let exe = dir.join("lockholder.exe");
    std::fs::copy(&helper, &exe).unwrap();
    let data = dir.join("service.db");
    std::fs::write(&data, b"data").unwrap();
    let name = format!("ExpungeLockTest{}", std::process::id());
    let bin = format!(
        "\"{}\" service {name} \"{}\"",
        exe.display(),
        data.display()
    );
    let (code, out) = platform::run_hidden(
        "sc.exe",
        &["create", &name, "binPath=", &bin, "start=", "demand"],
    )
    .unwrap();
    assert_eq!(code, 0, "sc create: {out}");
    let _svc = TestService(name.clone());
    let (code, out) = platform::run_hidden("sc.exe", &["start", &name]).unwrap();
    assert_eq!(code, 0, "sc start: {out}");

    // Wait until it has the file open, and check it's reported as the service.
    let deadline = Instant::now() + Duration::from_secs(30);
    let held = loop {
        let hs = locks::file_holders(&[&data]);
        if !hs.is_empty() || Instant::now() > deadline {
            break hs;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert!(
        held.iter()
            .any(|x| x.kind == HolderKind::Service && x.service.as_deref() == Some(name.as_str())),
        "{held:?}"
    );
    assert_eq!(
        locks::describe(&held).unwrap(),
        format!("held by the {name} service")
    );

    // The service reports "stopped" two seconds before its process lets go
    // of the file; the removal must still delete everything at once.
    let res = run(
        vec![
            Action::DeleteFile {
                path: path_str(&data),
            },
            Action::StopDeleteService { name: name.clone() },
            Action::DeleteFile {
                path: path_str(&exe),
            },
        ],
        &ExecOptions::default(),
    );
    let msgs: Vec<&str> = res.iter().map(|r| r.message.as_str()).collect();
    assert!(res.iter().all(|r| r.ok), "{res:#?}");
    assert_eq!(
        msgs,
        ["stopped and deleted", "deleted", "deleted"],
        "{res:#?}"
    );
    assert!(!data.exists() && !exe.exists());
}

#[test]
fn stopping_a_service_that_does_not_exist_is_not_an_error() {
    if !platform::windows::is_elevated() {
        // Opening the service manager to stop things needs rights we lack;
        // what matters here is the answer for an elevated removal.
        return;
    }
    let r = locks::stop_service("ExpungeNoSuchService-1b0e", Duration::from_secs(5))
        .unwrap_or_else(|e| panic!("stop_service: {e:#}"));
    assert_eq!(r, ServiceStop::NotFound);
}
