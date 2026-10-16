//! Installer Decrapifier: the background components an installation added
//! (services, scheduled tasks, autoruns, Startup shortcuts), each judged
//! *Required*, *Optional* or *Bloat* (telemetry, silent updaters, adware),
//! so the unwanted ones can be struck before the install log is saved.
//!
//! Judging is local rules first: names, commands, where the program file
//! lives and who signed it. What the rules can't decide is left *Unknown*,
//! and can be passed to a local language model if one is configured
//! (see [`crate::llm`]); nothing leaves the PC.

use crate::model::{fold, ChangeSet, KeyData};
use crate::platform::expand_env;
use crate::revert::Action;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Service,
    Task,
    Autorun,
    StartupShortcut,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Service => "Service",
            Kind::Task => "Scheduled task",
            Kind::Autorun => "Starts with Windows",
            Kind::StartupShortcut => "Startup folder shortcut",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// The program needs it to work.
    Required,
    /// Useful to some people (starting the program with Windows, a tray helper).
    Optional,
    /// Telemetry, silent updaters, ads: struck by default.
    Bloat,
    /// The rules couldn't tell.
    Unknown,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Required => "Required for the program",
            Verdict::Optional => "Optional",
            Verdict::Bloat => "Bloatware / telemetry / silent updater",
            Verdict::Unknown => "Unknown",
        }
    }
    /// Whether it is ticked for striking by default.
    pub fn strike_by_default(self) -> bool {
        self == Verdict::Bloat
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Telemetry,
    Updater,
    Advertising,
    Autostart,
    Helper,
    Driver,
    ProgramService,
    Other,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecidedBy {
    Rules,
    Model,
    /// The live guard disabled it during installation.
    Guard,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Component {
    pub kind: Kind,
    /// Service name, task path (`\Vendor\Task`), value name or file name.
    pub name: String,
    /// What it runs.
    pub command: String,
    /// Registry key or file that defines it.
    pub location: String,
    pub verdict: Verdict,
    pub category: Category,
    pub reasons: Vec<String>,
    pub decided_by: DecidedBy,
}

/// What the installation is, to tell its own parts from passengers.
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub app_name: String,
    pub publisher: String,
    /// Folders the program was installed into (lower case is fine).
    pub install_dirs: Vec<String>,
}

const SERVICES: &str = "hklm\\system\\currentcontrolset\\services\\";
const TASK_TREE: &str =
    "hklm\\software\\microsoft\\windows nt\\currentversion\\schedule\\taskcache\\tree\\";
const RUN_KEYS: &[&str] = &[
    "hklm\\software\\microsoft\\windows\\currentversion\\run",
    "hklm\\software\\microsoft\\windows\\currentversion\\runonce",
    "hklm\\software\\wow6432node\\microsoft\\windows\\currentversion\\run",
    "hklm\\software\\wow6432node\\microsoft\\windows\\currentversion\\runonce",
    "hkcu\\software\\microsoft\\windows\\currentversion\\run",
    "hkcu\\software\\microsoft\\windows\\currentversion\\runonce",
];

fn startup_folders() -> Vec<String> {
    [
        "%APPDATA%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup",
        "%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup",
    ]
    .iter()
    .map(|p| fold(&expand_env(p)))
    .filter(|p| !p.contains('%'))
    .collect()
}

/// Read a scheduled task's command from its definition file, if it exists.
pub type TaskReader<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The task's command, read from `%SystemRoot%\System32\Tasks`.
pub fn read_task_command(task: &str) -> Option<String> {
    let file = format!("{}{}", expand_env("%SystemRoot%\\System32\\Tasks"), task);
    let bytes = std::fs::read(&file).ok()?;
    Some(crate::startup::parse_task(task, &file, &bytes).command)
}

/// Every background component the change set adds, judged by the rules.
pub fn components(cs: &ChangeSet, ctx: &Context, read_task: TaskReader) -> Vec<Component> {
    let mut out = Vec::new();
    // Services: a new key directly under Services.
    for k in &cs.keys_added {
        let f = fold(&k.path);
        if let Some(name) = f.strip_prefix(SERVICES) {
            if !name.is_empty() && !name.contains('\\') {
                out.push(service_component(k));
            }
        }
    }
    // Scheduled tasks: their entry in the TaskCache tree (a task has an
    // `Id` value; folders don't).
    for k in &cs.keys_added {
        let f = fold(&k.path);
        if let Some(rest) = f.strip_prefix(TASK_TREE) {
            if k.get("Id").is_none() {
                continue;
            }
            let name = format!("\\{}", &k.path[k.path.len() - rest.len()..]);
            let command = read_task(&name).unwrap_or_default();
            out.push(Component {
                kind: Kind::Task,
                name: name.clone(),
                command,
                location: k.path.clone(),
                verdict: Verdict::Unknown,
                category: Category::Other,
                reasons: Vec::new(),
                decided_by: DecidedBy::Rules,
            });
        }
    }
    // Autoruns: values added to a Run key (new or existing).
    let mut autorun = |key: &str, name: &str, command: String| {
        if RUN_KEYS.contains(&fold(key).as_str()) {
            out.push(Component {
                kind: Kind::Autorun,
                name: name.to_string(),
                command,
                location: key.to_string(),
                verdict: Verdict::Unknown,
                category: Category::Other,
                reasons: Vec::new(),
                decided_by: DecidedBy::Rules,
            });
        }
    };
    for v in &cs.values_added {
        autorun(
            &v.key,
            &v.value.name,
            v.value.as_string().unwrap_or_default(),
        );
    }
    for k in &cs.keys_added {
        for v in k.values.values() {
            autorun(&k.path, &v.name, v.as_string().unwrap_or_default());
        }
    }
    // Startup folder shortcuts.
    let folders = startup_folders();
    for f in &cs.files_added {
        let p = fold(&f.path);
        if f.is_dir {
            continue;
        }
        if let Some(dir) = folders.iter().find(|d| p.starts_with(d.as_str())) {
            let name = f.path[dir.len()..].trim_start_matches('\\').to_string();
            out.push(Component {
                kind: Kind::StartupShortcut,
                name,
                command: f.path.clone(),
                location: f.path.clone(),
                verdict: Verdict::Unknown,
                category: Category::Other,
                reasons: Vec::new(),
                decided_by: DecidedBy::Rules,
            });
        }
    }
    for c in &mut out {
        classify(c, ctx);
    }
    out.sort_by(|a, b| {
        b.verdict
            .strike_by_default()
            .cmp(&a.verdict.strike_by_default())
            .then(a.kind.cmp(&b.kind))
            .then_with(|| fold(&a.name).cmp(&fold(&b.name)))
    });
    out
}

fn service_component(k: &KeyData) -> Component {
    let name = crate::platform::leaf_name(&k.path).to_string();
    let image = k.get_string("ImagePath").unwrap_or_default();
    let ty = k.get_u32("Type").unwrap_or(0x10);
    let mut c = Component {
        kind: Kind::Service,
        name,
        command: image,
        location: k.path.clone(),
        verdict: Verdict::Unknown,
        category: Category::Other,
        reasons: Vec::new(),
        decided_by: DecidedBy::Rules,
    };
    if let Some(d) = k.get_string("DisplayName") {
        if !d.is_empty() && !d.starts_with('@') {
            c.reasons.push(format!("Shown as \"{d}\""));
        }
    }
    // Kernel and file-system drivers (Type 1, 2): hardware and VPN software
    // depends on them.
    if ty & 0x3 != 0 {
        c.category = Category::Driver;
    }
    c
}

/// Words in a name or command that mark each kind of passenger.
const TELEMETRY: &[&str] = &[
    "telemetry",
    "analytics",
    "diagtrack",
    "ceip",
    "usagestat",
    "usage stat",
    "metrics",
    "tracking",
    "tracker",
    "beacon",
    "crashpad",
    "crashreport",
    "crash report",
    "crashhandler",
    "crash_handler",
    "bugreport",
    "feedback",
    "insights",
    "statistics",
    "datacollector",
];
const UPDATER: &[&str] = &[
    "update",
    "updater",
    "upgrade",
    "autoupdate",
    "auto-update",
    "googleupdate",
    "edgeupdate",
    "adobearm",
    "adobe arm",
    "maintenanceservice",
    "patch",
    "softwareupdate",
    "liveupdate",
];
const ADS: &[&str] = &[
    "offer",
    "promo",
    "advert",
    "ads",
    "deals",
    "coupon",
    "toolbar",
    "searchprotect",
    "browserprotect",
    "opencandy",
    "installcore",
    "rewards",
    "shopping",
];
const HELPER: &[&str] = &[
    "helper",
    "agent",
    "tray",
    "notifier",
    "assistant",
    "launcher",
    "startup",
    "minimized",
    "--startup",
    "/startup",
    "-autostart",
    "--hidden",
    "/background",
    "--background",
];

fn has_word(hay: &str, words: &[&'static str]) -> Option<&'static str> {
    // Long words match anywhere ("update" in "myappupdate.exe"); short
    // ones only as a whole token ("ads", not "downloads").
    for &w in words {
        if w.len() <= 4 {
            if hay.split(|c: char| !c.is_alphanumeric()).any(|t| t == w) {
                return Some(w);
            }
        } else if hay.contains(w) {
            return Some(w);
        }
    }
    None
}

/// The program file a command line runs.
fn program_of(command: &str) -> String {
    let c = command.trim();
    let c = expand_env(c);
    if let Some(rest) = c.strip_prefix('"') {
        return rest.split('"').next().unwrap_or("").to_string();
    }
    let lower = fold(&c);
    for ext in [".exe", ".dll", ".cmd", ".bat", ".lnk"] {
        if let Some(i) = lower.find(ext) {
            return c[..i + ext.len()].to_string();
        }
    }
    c.split_whitespace().next().unwrap_or("").to_string()
}

fn in_dirs(path: &str, dirs: &[String]) -> bool {
    let p = fold(path);
    dirs.iter()
        .map(|d| fold(d.trim_end_matches('\\')))
        .any(|d| !d.is_empty() && (p.starts_with(&format!("{d}\\")) || p == d))
}

/// Judge one component by the rules (filling verdict, category, reasons).
pub fn classify(c: &mut Component, ctx: &Context) {
    let hay = fold(&format!("{} {}", c.name, c.command));
    let program = program_of(&c.command);
    let own = !program.is_empty() && in_dirs(&program, &ctx.install_dirs);
    let app = fold(&ctx.app_name);
    if own {
        c.reasons
            .push("Runs a file the program installed".to_string());
    } else if !program.is_empty() {
        c.reasons
            .push(format!("Runs {program}, outside the program's folder"));
    }
    if let Some(w) = has_word(&hay, TELEMETRY) {
        c.verdict = Verdict::Bloat;
        c.category = Category::Telemetry;
        c.reasons
            .insert(0, format!("Collects usage or crash data (\"{w}\")"));
        return;
    }
    if let Some(w) = has_word(&hay, ADS) {
        c.verdict = Verdict::Bloat;
        c.category = Category::Advertising;
        c.reasons
            .insert(0, format!("Advertising or offers (\"{w}\")"));
        return;
    }
    if c.category == Category::Driver {
        c.verdict = Verdict::Required;
        c.reasons.insert(
            0,
            "A driver: hardware or network features depend on it".into(),
        );
        return;
    }
    // Squirrel apps start themselves at sign-in through their updater.
    if matches!(c.kind, Kind::Autorun | Kind::StartupShortcut) && hay.contains("--processstart") {
        c.verdict = Verdict::Optional;
        c.category = Category::Autostart;
        c.reasons.insert(
            0,
            "Starts the program itself with Windows (through its updater)".into(),
        );
        return;
    }
    if let Some(w) = has_word(&hay, UPDATER) {
        c.verdict = Verdict::Bloat;
        c.category = Category::Updater;
        c.reasons.insert(
            0,
            format!("Checks for and installs updates in the background (\"{w}\"); the program can still be updated by hand"),
        );
        return;
    }
    match c.kind {
        Kind::Autorun | Kind::StartupShortcut => {
            if let Some(w) = has_word(&hay, HELPER) {
                c.verdict = Verdict::Optional;
                c.category = Category::Helper;
                c.reasons.insert(
                    0,
                    format!("A background helper started with Windows (\"{w}\")"),
                );
            } else if own || (!app.is_empty() && hay.contains(&app)) {
                c.verdict = Verdict::Optional;
                c.category = Category::Autostart;
                c.reasons
                    .insert(0, "Starts the program itself with Windows".into());
            } else {
                c.verdict = Verdict::Unknown;
            }
        }
        Kind::Service => {
            if own {
                c.verdict = Verdict::Required;
                c.category = Category::ProgramService;
                c.reasons
                    .insert(0, "A service of the program itself".into());
            } else {
                c.verdict = Verdict::Unknown;
            }
        }
        Kind::Task => {
            if let Some(w) = has_word(&hay, HELPER) {
                c.verdict = Verdict::Optional;
                c.category = Category::Helper;
                c.reasons
                    .insert(0, format!("Starts a helper on a schedule (\"{w}\")"));
            } else {
                c.verdict = Verdict::Unknown;
            }
        }
    }
}

/// What striking a component does (all reversible from the backup).
pub fn strike_actions(c: &Component) -> Vec<Action> {
    match c.kind {
        Kind::Service => vec![
            Action::StopDeleteService {
                name: c.name.clone(),
            },
            Action::DeleteKey {
                path: c.location.clone(),
            },
        ],
        Kind::Task => vec![Action::DeleteTask {
            name: c.name.clone(),
        }],
        Kind::Autorun => vec![Action::DeleteValue {
            key: c.location.clone(),
            name: c.name.clone(),
        }],
        Kind::StartupShortcut => vec![Action::DeleteFile {
            path: c.location.clone(),
        }],
    }
}

/// Take a struck component out of the install log, so the log still
/// describes exactly what is left on the PC.
pub fn remove_from_log(cs: &mut ChangeSet, c: &Component) {
    let loc = fold(&c.location);
    match c.kind {
        Kind::Service => {
            let below = format!("{loc}\\");
            cs.keys_added.retain(|k| {
                let f = fold(&k.path);
                f != loc && !f.starts_with(&below)
            });
            cs.values_added.retain(|v| {
                let f = fold(&v.key);
                f != loc && !f.starts_with(&below)
            });
        }
        Kind::Task => {
            let tree = fold(&c.location);
            cs.keys_added.retain(|k| fold(&k.path) != tree);
            // Its TaskCache\Tasks entry and definition file.
            let task_file = fold(&format!(
                "{}{}",
                expand_env("%SystemRoot%\\System32\\Tasks"),
                c.name
            ));
            cs.files_added.retain(|f| fold(&f.path) != task_file);
        }
        Kind::Autorun => {
            let name = fold(&c.name);
            cs.values_added
                .retain(|v| !(fold(&v.key) == loc && fold(&v.value.name) == name));
            for k in &mut cs.keys_added {
                if fold(&k.path) == loc {
                    k.values.remove(&name);
                }
            }
        }
        Kind::StartupShortcut => cs.files_added.retain(|f| fold(&f.path) != loc),
    }
}

/// The program's identity and folders, from its install log: its
/// Uninstall entry's `InstallLocation`, and the top folders it created.
pub fn context_for(log: &crate::model::InstallLog) -> Context {
    let cs = &log.changes;
    let mut dirs: Vec<String> = Vec::new();
    if !log.uninstall_key.is_empty() {
        let key = fold(&log.uninstall_key);
        if let Some(k) = cs.keys_added.iter().find(|k| fold(&k.path) == key) {
            if let Some(loc) = k.get_string("InstallLocation") {
                let loc = loc
                    .trim()
                    .trim_matches('"')
                    .trim_end_matches('\\')
                    .to_string();
                if !loc.is_empty() {
                    dirs.push(loc);
                }
            }
        }
    }
    // Folders added whose parent already existed: the program's own roots.
    let added: std::collections::HashSet<String> = cs
        .files_added
        .iter()
        .filter(|f| f.is_dir)
        .map(|f| fold(&f.path))
        .collect();
    for f in cs.files_added.iter().filter(|f| f.is_dir) {
        let parent = crate::platform::parent_path(&f.path)
            .map(fold)
            .unwrap_or_default();
        if !added.contains(&parent) && !in_dirs(&f.path, &dirs) {
            dirs.push(f.path.clone());
        }
    }
    Context {
        app_name: log.name.clone(),
        publisher: log.publisher.clone(),
        install_dirs: dirs,
    }
}

/// Strike the chosen components now (everything backed up first), and
/// take the ones that went out of the log. Returns what each step did.
pub fn strike(
    log: &mut crate::model::InstallLog,
    chosen: &[&Component],
    reg: &dyn crate::platform::Registry,
    backup: Option<&mut crate::backup::BackupWriter>,
) -> Vec<crate::backup::ActionResult> {
    use crate::revert::{execute_plan, ExecOptions, Plan, PlanItem};
    let mut plan = Plan::default();
    let mut owner: Vec<usize> = Vec::new();
    for (i, c) in chosen.iter().enumerate() {
        for action in strike_actions(c) {
            plan.items.push(PlanItem {
                action,
                selected: true,
                exists: true,
                note: String::new(),
            });
            owner.push(i);
        }
    }
    let results = execute_plan(&plan, reg, backup, &ExecOptions::default());
    // execute_plan runs in its own order; match results back by description.
    let mut ok = vec![true; chosen.len()];
    for (item, i) in plan.items.iter().zip(&owner) {
        let desc = item.action.describe();
        if results.iter().any(|r| r.description == desc && !r.ok) {
            ok[*i] = false;
        }
    }
    for (c, done) in chosen.iter().zip(ok) {
        if done {
            remove_from_log(&mut log.changes, c);
        }
    }
    results
}

/// A sentence for the checklist header, e.g. "Discord installed, but
/// added 3 autorun tasks and a telemetry agent."
pub fn summary(app: &str, comps: &[Component]) -> String {
    let bloat: Vec<&Component> = comps
        .iter()
        .filter(|c| c.verdict == Verdict::Bloat)
        .collect();
    let app = if app.is_empty() { "The program" } else { app };
    if comps.is_empty() {
        return format!("{app} added nothing that runs in the background.");
    }
    let count = |cat: Category| bloat.iter().filter(|c| c.category == cat).count();
    let mut parts = Vec::new();
    for (cat, one, many) in [
        (
            Category::Telemetry,
            "a telemetry component",
            "telemetry components",
        ),
        (Category::Updater, "a silent updater", "silent updaters"),
        (
            Category::Advertising,
            "an advertising component",
            "advertising components",
        ),
    ] {
        match count(cat) {
            0 => {}
            1 => parts.push(one.to_string()),
            n => parts.push(format!("{n} {many}")),
        }
    }
    let others = comps.len() - bloat.len();
    if parts.is_empty() {
        return format!(
            "{app} added {} background component{}; none look like bloat.",
            comps.len(),
            if comps.len() == 1 { "" } else { "s" }
        );
    }
    let list = match parts.len() {
        1 => parts[0].clone(),
        n => format!("{} and {}", parts[..n - 1].join(", "), parts[n - 1]),
    };
    if others > 0 {
        format!(
            "{app} installed, but added {list} (and {others} other background item{}). Tick the ones to strike before finishing.",
            if others == 1 { "" } else { "s" }
        )
    } else {
        format!("{app} installed, but added {list}. Tick the ones to strike before finishing.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RegValue, ValueRef};

    fn ctx() -> Context {
        Context {
            app_name: "Discord".into(),
            publisher: "Discord Inc.".into(),
            install_dirs: vec!["C:\\Users\\a\\AppData\\Local\\Discord".into()],
        }
    }

    fn no_tasks(_: &str) -> Option<String> {
        None
    }

    fn run_value(key: &str, name: &str, cmd: &str) -> ValueRef {
        ValueRef {
            key: key.into(),
            value: RegValue::string(name, cmd),
        }
    }

    #[test]
    fn finds_and_judges_each_kind() {
        let mut cs = ChangeSet::default();
        cs.keys_added.push(
            KeyData::new("HKLM\\SYSTEM\\CurrentControlSet\\Services\\DiscordTelemetry")
                .with(RegValue::string(
                    "ImagePath",
                    "\"C:\\Program Files\\Vendor\\tele.exe\"",
                ))
                .with(RegValue::dword("Type", 0x10)),
        );
        cs.keys_added.push(
            KeyData::new("HKLM\\SYSTEM\\CurrentControlSet\\Services\\DiscordVpn")
                .with(RegValue::string(
                    "ImagePath",
                    "C:\\Users\\a\\AppData\\Local\\Discord\\vpn.exe --service",
                ))
                .with(RegValue::dword("Type", 0x10)),
        );
        cs.keys_added.push(
            KeyData::new("HKLM\\SYSTEM\\CurrentControlSet\\Services\\DiscordNet")
                .with(RegValue::string("ImagePath", "System32\\drivers\\dnet.sys"))
                .with(RegValue::dword("Type", 1)),
        );
        // A sub-key of a service is not another service.
        cs.keys_added.push(KeyData::new(
            "HKLM\\SYSTEM\\CurrentControlSet\\Services\\DiscordVpn\\Parameters",
        ));
        cs.keys_added.push(
            KeyData::new("HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Schedule\\TaskCache\\Tree\\Discord\\DiscordUpdateTaskMachineCore")
                .with(RegValue::string("Id", "{1}")),
        );
        cs.keys_added.push(KeyData::new(
            "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Schedule\\TaskCache\\Tree\\Discord",
        ));
        let run = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        cs.values_added.push(run_value(
            run,
            "Discord",
            "C:\\Users\\a\\AppData\\Local\\Discord\\Update.exe --processStart Discord.exe",
        ));
        cs.values_added.push(run_value(
            run,
            "DiscordHelper",
            "\"C:\\Users\\a\\AppData\\Local\\Discord\\helper.exe\" --minimized",
        ));
        cs.values_added
            .push(run_value("HKCU\\Software\\Other", "NotAutorun", "x.exe"));

        let tasks = |name: &str| -> Option<String> {
            (name == "\\Discord\\DiscordUpdateTaskMachineCore")
                .then(|| "C:\\Program Files (x86)\\Discord\\DiscordUpdate.exe /c".to_string())
        };
        let comps = components(&cs, &ctx(), &tasks);
        let by = |n: &str| comps.iter().find(|c| c.name == n).unwrap();
        assert_eq!(comps.len(), 6, "{comps:#?}");

        let t = by("DiscordTelemetry");
        assert_eq!(
            (t.kind, t.verdict, t.category),
            (Kind::Service, Verdict::Bloat, Category::Telemetry)
        );
        assert_eq!(by("DiscordVpn").verdict, Verdict::Required);
        assert_eq!(by("DiscordNet").category, Category::Driver);
        assert_eq!(by("DiscordNet").verdict, Verdict::Required);
        let task = by("\\Discord\\DiscordUpdateTaskMachineCore");
        assert_eq!(
            (task.kind, task.verdict, task.category),
            (Kind::Task, Verdict::Bloat, Category::Updater)
        );
        assert!(task.command.contains("DiscordUpdate.exe"));
        // Squirrel's "start the app at sign-in" goes through Update.exe.
        assert_eq!(by("Discord").verdict, Verdict::Optional);
        assert_eq!(by("Discord").category, Category::Autostart);
        assert_eq!(by("DiscordHelper").verdict, Verdict::Optional);
        assert_eq!(by("DiscordHelper").category, Category::Helper);
        // Bloat first: what will be struck is at the top of the checklist.
        assert_eq!(
            comps
                .iter()
                .take_while(|c| c.verdict == Verdict::Bloat)
                .count(),
            2
        );
        assert!(comps[0].verdict.strike_by_default());

        let s = summary("Discord", &comps);
        assert_eq!(s, "Discord installed, but added a telemetry component and a silent updater (and 4 other background items). Tick the ones to strike before finishing.");
    }

    #[test]
    fn short_words_need_a_whole_token() {
        let mut c = Component {
            kind: Kind::Autorun,
            name: "Loads".into(),
            command: "C:\\Tools\\downloads.exe".into(),
            location: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
            verdict: Verdict::Unknown,
            category: Category::Other,
            reasons: vec![],
            decided_by: DecidedBy::Rules,
        };
        classify(&mut c, &Context::default());
        assert_ne!(
            c.category,
            Category::Advertising,
            "\"ads\" inside \"downloads\" is not an ad"
        );
        assert_eq!(c.verdict, Verdict::Unknown);
    }

    #[test]
    fn striking_removes_it_from_the_log() {
        let mut cs = ChangeSet::default();
        let svc = "HKLM\\SYSTEM\\CurrentControlSet\\Services\\Tele";
        cs.keys_added
            .push(KeyData::new(svc).with(RegValue::string("ImagePath", "t.exe")));
        cs.keys_added
            .push(KeyData::new(&format!("{svc}\\Parameters")));
        cs.keys_added.push(KeyData::new("HKLM\\SOFTWARE\\Vendor"));
        let run = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        cs.values_added.push(run_value(run, "Upd", "upd.exe"));
        cs.values_added.push(run_value(run, "Keep", "keep.exe"));
        let comps = components(&cs, &Context::default(), &no_tasks);
        for c in comps.iter().filter(|c| c.name != "Keep") {
            remove_from_log(&mut cs, c);
            assert!(!strike_actions(c).is_empty());
        }
        assert_eq!(cs.keys_added.len(), 1);
        assert_eq!(cs.keys_added[0].path, "HKLM\\SOFTWARE\\Vendor");
        assert_eq!(cs.values_added.len(), 1);
        assert_eq!(cs.values_added[0].value.name, "Keep");
    }

    #[test]
    fn context_comes_from_install_location_and_new_top_folders() {
        use crate::model::{FileEntry, InstallLog};
        let mut log = InstallLog {
            name: "Discord".into(),
            uninstall_key: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Discord"
                .into(),
            ..Default::default()
        };
        log.changes
            .keys_added
            .push(KeyData::new(&log.uninstall_key).with(RegValue::string(
                "InstallLocation",
                "\"C:\\Users\\a\\AppData\\Local\\Discord\\\"",
            )));
        let dir = |p: &str| FileEntry {
            path: p.into(),
            is_dir: true,
            size: 0,
            mtime: 0,
        };
        log.changes
            .files_added
            .push(dir("C:\\Users\\a\\AppData\\Local\\Discord"));
        log.changes
            .files_added
            .push(dir("C:\\Users\\a\\AppData\\Local\\Discord\\app-1"));
        log.changes
            .files_added
            .push(dir("C:\\Users\\a\\AppData\\Roaming\\discord"));
        let c = context_for(&log);
        assert_eq!(c.app_name, "Discord");
        assert_eq!(
            c.install_dirs,
            [
                "C:\\Users\\a\\AppData\\Local\\Discord",
                "C:\\Users\\a\\AppData\\Roaming\\discord"
            ]
        );
    }

    #[test]
    fn striking_changes_the_registry_and_the_log() {
        use crate::model::InstallLog;
        use crate::platform::{MemRegistry, Registry};
        let reg = MemRegistry::new();
        let run = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        reg.set_value(run, &RegValue::string("VendorUpdater", "upd.exe /silent"))
            .unwrap();
        reg.set_value(run, &RegValue::string("Keep", "keep.exe"))
            .unwrap();
        let mut log = InstallLog::default();
        log.changes
            .values_added
            .push(run_value(run, "VendorUpdater", "upd.exe /silent"));
        log.changes
            .values_added
            .push(run_value(run, "Keep", "keep.exe"));
        let comps = components(&log.changes, &Context::default(), &no_tasks);
        let chosen: Vec<&Component> = comps
            .iter()
            .filter(|c| c.verdict.strike_by_default())
            .collect();
        assert_eq!(chosen.len(), 1);
        let res = strike(&mut log, &chosen, &reg, None);
        assert!(res.iter().all(|r| r.ok), "{res:?}");
        let k = reg.read_key(run).unwrap();
        assert!(k.get("VendorUpdater").is_none() && k.get("Keep").is_some());
        assert_eq!(log.changes.values_added.len(), 1);
    }

    #[test]
    fn nothing_in_the_background_says_so() {
        assert_eq!(
            summary("7-Zip", &[]),
            "7-Zip added nothing that runs in the background."
        );
    }
}
