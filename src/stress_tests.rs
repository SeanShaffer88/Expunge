//! Edge cases a removal must survive: very deep registry trees, thousands
//! of files, paths longer than the old 260-character limit, files that are
//! in use, and links that point outside the folder being removed. Each one
//! must degrade gracefully (backed up, reported, scheduled for the next
//! restart) and never leave things half done without a backup.

use crate::backup::{list_backups, restore_backup, BackupWriter};
use crate::model::{KeyData, RegValue};
use crate::platform::{MemRegistry, Registry};
use crate::revert::{execute_plan, Action, ExecOptions, Plan, PlanItem};
use std::path::{Path, PathBuf};

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

fn run(p: &Plan, reg: &dyn Registry, broot: &Path) -> (Vec<crate::backup::ActionResult>, String) {
    let mut w = BackupWriter::new(broot, "stress").unwrap();
    let res = execute_plan(p, reg, Some(&mut w), &ExecOptions::default());
    let id = w.finish().unwrap().id;
    (res, id)
}

#[test]
fn deep_and_wide_registry_trees_are_removed_and_restored() {
    let tmp = tempfile::tempdir().unwrap();
    let reg = MemRegistry::new();
    let root = "HKCU\\Software\\StressVendor";
    // 400 levels deep, and 2,000 siblings with values.
    let mut path = root.to_string();
    for i in 0..400 {
        path.push_str(&format!("\\L{i}"));
        reg.put(KeyData::new(&path).with(RegValue::dword("depth", i)));
    }
    for i in 0..2000 {
        reg.put(
            KeyData::new(&format!("{root}\\Wide\\K{i}"))
                .with(RegValue::string("name", &format!("value {i}"))),
        );
    }
    let p = plan(vec![Action::DeleteKey { path: root.into() }]);
    let (res, id) = run(&p, &reg, tmp.path());
    assert!(res.iter().all(|r| r.ok), "{res:?}");
    assert!(!reg.exists(root));

    let back = restore_backup(tmp.path(), &id, &reg).unwrap();
    assert!(back.iter().all(|r| r.ok), "{back:?}");
    assert_eq!(reg.read_key(&path).unwrap().get_u32("depth"), Some(399));
    assert_eq!(
        reg.read_key(&format!("{root}\\Wide\\K1999"))
            .unwrap()
            .get_string("name")
            .as_deref(),
        Some("value 1999")
    );
}

#[test]
fn thousands_of_files_are_backed_up_quickly() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Program Files").join("Bulk");
    for d in 0..30 {
        let sub = dir.join(format!("d{d}"));
        std::fs::create_dir_all(&sub).unwrap();
        for f in 0..100 {
            std::fs::write(sub.join(format!("f{f}.dat")), format!("{d}/{f}")).unwrap();
        }
    }
    let reg = MemRegistry::new();
    let start = std::time::Instant::now();
    let p = plan(vec![Action::DeleteTree {
        path: dir.to_string_lossy().into(),
    }]);
    let (res, id) = run(&p, &reg, &tmp.path().join("backups"));
    assert!(res.iter().all(|r| r.ok), "{res:?}");
    assert!(!dir.exists());
    // One manifest write for the whole folder keeps this fast.
    assert!(
        start.elapsed() < std::time::Duration::from_secs(30),
        "3,000 files took {:?}",
        start.elapsed()
    );
    let b = &list_backups(&tmp.path().join("backups"))[0];
    assert!(b.bytes() > 0);
    let back = restore_backup(&tmp.path().join("backups"), &id, &reg).unwrap();
    assert!(back.iter().all(|r| r.ok));
    assert_eq!(
        std::fs::read_to_string(dir.join("d29").join("f99.dat")).unwrap(),
        "29/99"
    );
}

fn long_dir(base: &Path) -> PathBuf {
    let mut p = base.join("LongVendor");
    while p.to_string_lossy().len() < 320 {
        p.push("a_rather_long_folder_name_0123456789");
    }
    p
}

#[test]
fn paths_longer_than_260_characters() {
    let tmp = tempfile::tempdir().unwrap();
    let deep = long_dir(tmp.path());
    std::fs::create_dir_all(&deep).unwrap();
    let file = deep.join("settings.ini");
    std::fs::write(&file, "x=1").unwrap();
    assert!(file.to_string_lossy().len() > 260);
    let reg = MemRegistry::new();
    let top = tmp.path().join("LongVendor");
    let p = plan(vec![Action::DeleteTree {
        path: top.to_string_lossy().into(),
    }]);
    let (res, id) = run(&p, &reg, &tmp.path().join("backups"));
    assert!(res.iter().all(|r| r.ok), "{res:?}");
    assert!(!top.exists());
    restore_backup(&tmp.path().join("backups"), &id, &reg).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "x=1");
}

#[test]
fn a_link_inside_a_removed_folder_never_reaches_its_target() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tmp.path().join("Precious");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("thesis.docx"), "years of work").unwrap();
    let app = tmp.path().join("App");
    std::fs::create_dir_all(&app).unwrap();
    let link = app.join("data");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    if std::os::windows::fs::symlink_dir(&outside, &link).is_err() {
        return; // Symbolic links need Developer Mode or elevation.
    }
    let reg = MemRegistry::new();
    // The link itself, then the folder holding it.
    let p = plan(vec![
        Action::DeleteTree {
            path: link.to_string_lossy().into(),
        },
        Action::DeleteTree {
            path: app.to_string_lossy().into(),
        },
    ]);
    let (res, _) = run(&p, &reg, &tmp.path().join("backups"));
    assert!(res.iter().all(|r| r.ok), "{res:?}");
    assert!(!app.exists());
    assert_eq!(
        std::fs::read_to_string(outside.join("thesis.docx")).unwrap(),
        "years of work"
    );
}

/// A file held open without sharing can't be deleted now: it is backed up
/// and scheduled for the next restart (or reported), never a crash.
#[cfg(windows)]
#[test]
fn files_in_use_are_scheduled_not_fatal() {
    use std::os::windows::fs::OpenOptionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Busy");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("locked.dll");
    std::fs::write(&file, "in use").unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    let reg = MemRegistry::new();
    let p = plan(vec![
        Action::DeleteFile {
            path: file.to_string_lossy().into(),
        },
        Action::DeleteTree {
            path: dir.to_string_lossy().into(),
        },
    ]);
    let mut w = BackupWriter::new(&tmp.path().join("backups"), "busy").unwrap();
    let res = execute_plan(&p, &reg, Some(&mut w), &ExecOptions::default());
    drop(held);
    for r in &res {
        assert!(
            r.ok || !r.message.is_empty(),
            "every outcome is explained: {r:?}"
        );
    }
    // Once released, nothing stops a second attempt.
    let again = execute_plan(&p, &reg, None, &ExecOptions::default());
    assert!(again.iter().all(|r| r.ok), "{again:?}");
}
