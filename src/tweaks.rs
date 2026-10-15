//! Anti-telemetry and privacy tweaks, in the spirit of O&O ShutUp10++,
//! W10Privacy and WinUtil — every one reversible.
//!
//! Each tweak is a list of operations with a *target* (the tweak applied)
//! and a *default* (what Windows uses when nobody changed it). Applying a
//! tweak records each setting's previous state in a JSON state file, so it
//! can be undone exactly; without a record, undo returns to the default.

use crate::model::{fold, RegValue};
use crate::platform::{self, Registry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// How aggressive a tweak is. Presets select everything up to a tier.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Safe for everyone: no feature anyone relies on stops working.
    Minimal,
    /// Typical daily-driver choices; some convenience features turn off.
    Standard,
    /// Aggressive: can break features or delay security fixes.
    Advanced,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Minimal => "Minimal",
            Tier::Standard => "Standard",
            Tier::Advanced => "Advanced",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    Dword(u32),
    Str(&'static str),
}

impl Val {
    fn to_reg(self, name: &str) -> RegValue {
        match self {
            Val::Dword(d) => RegValue::dword(name, d),
            Val::Str(s) => RegValue::string(name, s),
        }
    }

    fn matches(self, v: &RegValue) -> bool {
        match self {
            Val::Dword(d) => v.as_u32() == Some(d),
            Val::Str(s) => v.as_string().is_some_and(|x| x == s),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Op {
    /// Set a registry value; `default` None means "value absent".
    Reg {
        key: &'static str,
        name: &'static str,
        value: Val,
        default: Option<Val>,
    },
    /// Set a service's start type (2 automatic, 3 manual, 4 disabled) and
    /// stop or start it right away.
    Service {
        name: &'static str,
        start: u32,
        default: u32,
    },
    /// Disable a scheduled task (enabled again on undo).
    Task { path: &'static str },
    /// Create and activate the hidden Ultimate Performance power plan.
    UltimatePower,
    /// Pause Windows Update for this many days (as Settings does).
    PauseUpdates { days: u32 },
}

pub struct Tweak {
    pub id: &'static str,
    pub category: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub tier: Tier,
    pub ops: &'static [Op],
}

impl Tweak {
    pub fn needs_admin(&self) -> bool {
        self.ops.iter().any(|o| match o {
            Op::Reg { key, .. } => key.starts_with("HKLM"),
            _ => true,
        })
    }
}

const fn reg(key: &'static str, name: &'static str, value: Val, default: Option<Val>) -> Op {
    Op::Reg {
        key,
        name,
        value,
        default,
    }
}

const ADV: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced";
const CDM: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager";
const APP_PRIVACY: &str = "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AppPrivacy";

use Val::{Dword as D, Str as S};

pub const CATEGORIES: [&str; 8] = [
    "Telemetry and data collection",
    "Privacy and app permissions",
    "Bloatware and AI features",
    "Explorer and search",
    "Services and background tasks",
    "Windows Update",
    "Network",
    "Performance",
];

pub static TWEAKS: &[Tweak] = &[
    // --- Telemetry ---------------------------------------------------------
    Tweak {
        id: "diagnostic_data",
        category: "Telemetry and data collection",
        title: "Send only required diagnostic data",
        description: "Limits what Windows reports to Microsoft to the lowest level your edition allows, and turns off the compatibility and customer-experience data tasks.",
        tier: Tier::Minimal,
        ops: &[
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection", "AllowTelemetry", D(0), None),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection", "DoNotShowFeedbackNotifications", D(1), None),
            Op::Task { path: "\\Microsoft\\Windows\\Customer Experience Improvement Program\\Consolidator" },
            Op::Task { path: "\\Microsoft\\Windows\\Customer Experience Improvement Program\\UsbCeip" },
        ],
    },
    Tweak {
        id: "advertising_id",
        category: "Telemetry and data collection",
        title: "Turn off the advertising ID",
        description: "Apps can no longer use a unique ID to track you across apps for personalised ads.",
        tier: Tier::Minimal,
        ops: &[
            reg("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\AdvertisingInfo", "Enabled", D(0), Some(D(1))),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AdvertisingInfo", "DisabledByGroupPolicy", D(1), None),
        ],
    },
    Tweak {
        id: "app_launch_tracking",
        category: "Telemetry and data collection",
        title: "Stop tracking which apps you open",
        description: "Windows stops recording app launches to fill Start menu recommendations.",
        tier: Tier::Minimal,
        ops: &[reg(ADV, "Start_TrackProgs", D(0), Some(D(1)))],
    },
    Tweak {
        id: "typing_data",
        category: "Telemetry and data collection",
        title: "Stop collecting typing and handwriting data",
        description: "Microsoft no longer receives text and ink samples to \"improve\" its language models.",
        tier: Tier::Minimal,
        ops: &[
            reg("HKCU\\Software\\Microsoft\\InputPersonalization", "RestrictImplicitTextCollection", D(1), Some(D(0))),
            reg("HKCU\\Software\\Microsoft\\InputPersonalization", "RestrictImplicitInkCollection", D(1), Some(D(0))),
            reg("HKCU\\Software\\Microsoft\\InputPersonalization\\TrainedDataStore", "HarvestContacts", D(0), Some(D(1))),
            reg("HKCU\\Software\\Microsoft\\Input\\TIPC", "Enabled", D(0), Some(D(1))),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\HandwritingErrorReports", "PreventHandwritingErrorReports", D(1), None),
        ],
    },
    Tweak {
        id: "cloud_search",
        category: "Telemetry and data collection",
        title: "Keep Windows Search local",
        description: "Searches in the taskbar no longer go to Bing, and Cortana is turned off.",
        tier: Tier::Standard,
        ops: &[
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Windows Search", "AllowCortana", D(0), None),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Windows Search", "DisableWebSearch", D(1), None),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\Windows Search", "ConnectedSearchUseWeb", D(0), None),
            reg("HKCU\\Software\\Policies\\Microsoft\\Windows\\Explorer", "DisableSearchBoxSuggestions", D(1), None),
            reg("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Search", "BingSearchEnabled", D(0), None),
        ],
    },
    Tweak {
        id: "compat_inventory",
        category: "Telemetry and data collection",
        title: "Stop the app inventory and compatibility reporting",
        description: "Turns off the Inventory Collector and the Program Compatibility Assistant, which scan installed programs and report them.",
        tier: Tier::Standard,
        ops: &[
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AppCompat", "DisableInventory", D(1), None),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AppCompat", "AITEnable", D(0), None),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AppCompat", "DisablePCA", D(1), None),
            Op::Task { path: "\\Microsoft\\Windows\\Application Experience\\Microsoft Compatibility Appraiser" },
            Op::Task { path: "\\Microsoft\\Windows\\Application Experience\\ProgramDataUpdater" },
        ],
    },
    // --- Privacy -----------------------------------------------------------
    Tweak {
        id: "suggested_content",
        category: "Privacy and app permissions",
        title: "Turn off tips, ads and automatic app installs",
        description: "No more suggestions in Start, Settings and the lock screen, \"tailored experiences\", or sponsored apps installed by themselves.",
        tier: Tier::Minimal,
        ops: &[
            reg(CDM, "SilentInstalledAppsEnabled", D(0), Some(D(1))),
            reg(CDM, "SystemPaneSuggestionsEnabled", D(0), Some(D(1))),
            reg(CDM, "SoftLandingEnabled", D(0), Some(D(1))),
            reg(CDM, "RotatingLockScreenOverlayEnabled", D(0), Some(D(1))),
            reg(CDM, "SubscribedContent-338388Enabled", D(0), Some(D(1))),
            reg(CDM, "SubscribedContent-338389Enabled", D(0), Some(D(1))),
            reg(CDM, "SubscribedContent-338393Enabled", D(0), Some(D(1))),
            reg(CDM, "SubscribedContent-353694Enabled", D(0), Some(D(1))),
            reg(CDM, "SubscribedContent-353696Enabled", D(0), Some(D(1))),
            reg("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Privacy", "TailoredExperiencesWithDiagnosticDataEnabled", D(0), Some(D(1))),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\CloudContent", "DisableWindowsConsumerFeatures", D(1), None),
        ],
    },
    Tweak {
        id: "location",
        category: "Privacy and app permissions",
        title: "Turn off location services",
        description: "Turns off location for the whole PC, including geofencing. Maps, weather and Find my device can no longer find where you are.",
        tier: Tier::Standard,
        ops: &[reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\LocationAndSensors", "DisableLocation", D(1), None)],
    },
    Tweak {
        id: "background_apps",
        category: "Privacy and app permissions",
        title: "Stop Store apps running in the background",
        description: "Store apps only run while open. Their notifications and widgets may stop updating.",
        tier: Tier::Standard,
        ops: &[reg(APP_PRIVACY, "LetAppsRunInBackground", D(2), None)],
    },
    Tweak {
        id: "app_permissions",
        category: "Privacy and app permissions",
        title: "Deny Store apps access to camera, microphone and personal data",
        description: "Store apps can no longer use the camera, microphone, location, contacts, calendar, call history, email or messages. Desktop programs aren't affected. Video-call Store apps stop working.",
        tier: Tier::Advanced,
        ops: &[
            reg(APP_PRIVACY, "LetAppsAccessCamera", D(2), None),
            reg(APP_PRIVACY, "LetAppsAccessMicrophone", D(2), None),
            reg(APP_PRIVACY, "LetAppsAccessLocation", D(2), None),
            reg(APP_PRIVACY, "LetAppsAccessContacts", D(2), None),
            reg(APP_PRIVACY, "LetAppsAccessCalendar", D(2), None),
            reg(APP_PRIVACY, "LetAppsAccessCallHistory", D(2), None),
            reg(APP_PRIVACY, "LetAppsAccessEmail", D(2), None),
            reg(APP_PRIVACY, "LetAppsAccessMessaging", D(2), None),
        ],
    },
    // --- Bloatware & AI ----------------------------------------------------
    Tweak {
        id: "copilot",
        category: "Bloatware and AI features",
        title: "Turn off Windows Copilot",
        description: "Removes the Copilot button and turns Copilot off by policy.",
        tier: Tier::Standard,
        ops: &[
            reg("HKCU\\Software\\Policies\\Microsoft\\Windows\\WindowsCopilot", "TurnOffWindowsCopilot", D(1), None),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsCopilot", "TurnOffWindowsCopilot", D(1), None),
            reg(ADV, "ShowCopilotButton", D(0), Some(D(1))),
        ],
    },
    Tweak {
        id: "recall",
        category: "Bloatware and AI features",
        title: "Turn off Recall snapshots",
        description: "Windows stops saving snapshots of your screen for Recall.",
        tier: Tier::Standard,
        ops: &[
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsAI", "DisableAIDataAnalysis", D(1), None),
            reg("HKCU\\Software\\Policies\\Microsoft\\Windows\\WindowsAI", "DisableAIDataAnalysis", D(1), None),
        ],
    },
    // --- Explorer & search -------------------------------------------------
    Tweak {
        id: "classic_context_menu",
        category: "Explorer and search",
        title: "Full right-click menu",
        description: "Windows 11 shows the full right-click menu straight away instead of hiding it under \"Show more options\". Takes effect after signing out or restarting Explorer.",
        tier: Tier::Standard,
        ops: &[reg(
            "HKCU\\Software\\Classes\\CLSID\\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\\InprocServer32",
            "",
            S(""),
            None,
        )],
    },
    Tweak {
        id: "explorer_defaults",
        category: "Explorer and search",
        title: "Explorer: open This PC, show extensions and hidden files",
        description: "File Explorer opens on This PC, shows file extensions and hidden files, and stops listing recent and frequent files.",
        tier: Tier::Standard,
        ops: &[
            reg(ADV, "LaunchTo", D(1), Some(D(2))),
            reg(ADV, "HideFileExt", D(0), Some(D(1))),
            reg(ADV, "Hidden", D(1), Some(D(2))),
            reg("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer", "ShowRecent", D(0), Some(D(1))),
            reg("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer", "ShowFrequent", D(0), Some(D(1))),
        ],
    },
    Tweak {
        id: "transparency",
        category: "Explorer and search",
        title: "Turn off transparency effects",
        description: "The taskbar, Start menu and windows become solid, which saves a little graphics work.",
        tier: Tier::Standard,
        ops: &[reg("HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize", "EnableTransparency", D(0), Some(D(1)))],
    },
    Tweak {
        id: "search_indexing",
        category: "Explorer and search",
        title: "Turn off search indexing",
        description: "Stops the Windows Search indexer (WSearch) to save disk and processor time. Searching files, Start and Outlook becomes slower.",
        tier: Tier::Advanced,
        ops: &[Op::Service { name: "WSearch", start: 4, default: 2 }],
    },
    // --- Services ----------------------------------------------------------
    Tweak {
        id: "diagtrack",
        category: "Services and background tasks",
        title: "Turn off the telemetry service (DiagTrack)",
        description: "Stops Connected User Experiences and Telemetry, the service that sends diagnostic data.",
        tier: Tier::Standard,
        ops: &[Op::Service { name: "DiagTrack", start: 4, default: 2 }],
    },
    Tweak {
        id: "dmwappush",
        category: "Services and background tasks",
        title: "Turn off WAP push message routing",
        description: "Stops dmwappushservice, which routes device-management and telemetry messages.",
        tier: Tier::Standard,
        ops: &[Op::Service { name: "dmwappushservice", start: 4, default: 3 }],
    },
    Tweak {
        id: "error_reporting",
        category: "Services and background tasks",
        title: "Turn off Windows Error Reporting",
        description: "Crash reports and dumps are no longer sent to Microsoft.",
        tier: Tier::Standard,
        ops: &[
            reg("HKLM\\SOFTWARE\\Microsoft\\Windows\\Windows Error Reporting", "Disabled", D(1), None),
            Op::Service { name: "WerSvc", start: 4, default: 3 },
        ],
    },
    Tweak {
        id: "sysmain",
        category: "Services and background tasks",
        title: "Turn off SysMain (Superfetch)",
        description: "Stops Windows preloading frequently used programs. Can help on hard drives or unusual workloads; on most PCs it makes no difference or makes programs start slower.",
        tier: Tier::Advanced,
        ops: &[Op::Service { name: "SysMain", start: 4, default: 2 }],
    },
    // --- Windows Update ----------------------------------------------------
    Tweak {
        id: "delay_updates",
        category: "Windows Update",
        title: "Delay feature updates a year and security updates 4 days",
        description: "Big Windows upgrades wait 365 days and monthly fixes wait 4 days, to avoid buggy releases. Needs Windows Pro or higher. Delaying security fixes leaves you exposed for longer.",
        tier: Tier::Advanced,
        ops: &[
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsUpdate", "DeferFeatureUpdates", D(1), None),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsUpdate", "DeferFeatureUpdatesPeriodInDays", D(365), None),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsUpdate", "DeferQualityUpdates", D(1), None),
            reg("HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsUpdate", "DeferQualityUpdatesPeriodInDays", D(4), None),
        ],
    },
    Tweak {
        id: "pause_updates",
        category: "Windows Update",
        title: "Pause updates for 7 days",
        description: "Windows Update installs nothing for a week, the same as the Pause button in Settings. Undo resumes updates straight away.",
        tier: Tier::Advanced,
        ops: &[Op::PauseUpdates { days: 7 }],
    },
    // --- Network -----------------------------------------------------------
    Tweak {
        id: "wifi_sense",
        category: "Network",
        title: "Don't connect to open hotspots automatically",
        description: "Windows stops joining suggested open Wi-Fi hotspots and sharing hotspot reports.",
        tier: Tier::Minimal,
        ops: &[
            reg("HKLM\\SOFTWARE\\Microsoft\\PolicyManager\\default\\WiFi\\AllowAutoConnectToWiFiSenseHotspots", "value", D(0), None),
            reg("HKLM\\SOFTWARE\\Microsoft\\PolicyManager\\default\\WiFi\\AllowWiFiHotSpotReporting", "value", D(0), None),
        ],
    },
    // --- Performance -------------------------------------------------------
    Tweak {
        id: "ultimate_power",
        category: "Performance",
        title: "Ultimate Performance power plan",
        description: "Creates and switches to Windows' hidden Ultimate Performance plan (no processor parking, lowest latency). Uses more power; not recommended on battery.",
        tier: Tier::Advanced,
        ops: &[Op::UltimatePower],
    },
];

pub fn get(id: &str) -> Option<&'static Tweak> {
    TWEAKS.iter().find(|t| t.id == id)
}

/// Whether a tweak is currently in effect.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    On,
    Off,
    Partly,
}

const SERVICES: &str = "HKLM\\SYSTEM\\CurrentControlSet\\Services";
const UX_SETTINGS: &str = "HKLM\\SOFTWARE\\Microsoft\\WindowsUpdate\\UX\\Settings";
const PAUSE_VALUES: [&str; 5] = [
    "PauseUpdatesExpiryTime",
    "PauseFeatureUpdatesStartTime",
    "PauseFeatureUpdatesEndTime",
    "PauseQualityUpdatesStartTime",
    "PauseQualityUpdatesEndTime",
];

fn iso_day(days_from_now: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::days(days_from_now))
        .format("%Y-%m-%dT00:00:00Z")
        .to_string()
}

/// Updates are paused if the expiry date is still in the future.
fn paused(reg: &dyn Registry) -> bool {
    reg.read_key(UX_SETTINGS)
        .and_then(|k| k.get_string("PauseUpdatesExpiryTime"))
        .is_some_and(|s| s.as_str() > iso_day(0).as_str())
}
const ULTIMATE_GUID: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";
const BALANCED_GUID: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";

fn reg_matches(reg: &dyn Registry, key: &str, name: &str, value: Val) -> bool {
    reg.read_key(key)
        .and_then(|k| k.get(name).cloned())
        .is_some_and(|v| value.matches(&v))
}

/// Current status. Scheduled tasks don't count (they can be missing on
/// some editions); the power plan is checked by name.
pub fn status(t: &Tweak, reg: &dyn Registry, active_power_scheme: &str) -> Status {
    let mut on = 0;
    let mut total = 0;
    for op in t.ops {
        let hit = match op {
            Op::Reg {
                key, name, value, ..
            } => reg_matches(reg, key, name, *value),
            Op::Service { name, start, .. } => {
                // A service that isn't installed can't be running.
                let key = format!("{SERVICES}\\{name}");
                !reg.exists(&key) || reg_matches(reg, &key, "Start", D(*start))
            }
            Op::Task { .. } => continue,
            Op::UltimatePower => fold(active_power_scheme).contains("ultimate"),
            Op::PauseUpdates { .. } => paused(reg),
        };
        total += 1;
        on += hit as usize;
    }
    match on {
        0 => Status::Off,
        n if n == total => Status::On,
        _ => Status::Partly,
    }
}

/// What a setting looked like before a tweak changed it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Previous {
    /// `None`: the value didn't exist.
    Value {
        key: String,
        name: String,
        old: Option<RegValue>,
    },
    Task {
        path: String,
    },
    /// The power scheme GUID that was active.
    Power {
        scheme: String,
    },
}

/// Saved state for undo, kept in the data folder.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct State {
    pub applied: BTreeMap<String, Vec<Previous>>,
}

impl State {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

fn run(program: &str, args: &[&str]) -> anyhow::Result<String> {
    let (code, out) = platform::run_hidden(program, args)?;
    if code != 0 {
        anyhow::bail!(
            "{program} exit code {code}: {}",
            out.lines().last().unwrap_or("")
        );
    }
    Ok(out)
}

/// The active power scheme as printed by `powercfg /getactivescheme`.
pub fn active_power_scheme() -> String {
    platform::run_hidden("powercfg.exe", &["/getactivescheme"])
        .map(|(_, o)| o)
        .unwrap_or_default()
}

fn guid_in(s: &str) -> Option<String> {
    s.split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .find(|w| w.len() == 36 && w.matches('-').count() == 4)
        .map(str::to_string)
}

fn set_service(reg: &dyn Registry, name: &str, start: u32) -> anyhow::Result<()> {
    let key = format!("{SERVICES}\\{name}");
    if !reg.exists(&key) {
        return Ok(()); // not installed on this edition
    }
    reg.set_value(&key, &RegValue::dword("Start", start))?;
    let action = if start == 4 { "stop" } else { "start" };
    let _ = platform::run_hidden("sc.exe", &[action, name]);
    Ok(())
}

/// Apply a tweak, recording previous state in `state`.
pub fn apply(t: &Tweak, reg: &dyn Registry, state: &mut State) -> anyhow::Result<()> {
    let mut prev = Vec::new();
    for op in t.ops {
        match op {
            Op::Reg {
                key, name, value, ..
            } => {
                let old = reg.read_key(key).and_then(|k| k.get(name).cloned());
                if !reg.exists(key) {
                    reg.create_key(key)?;
                }
                reg.set_value(key, &value.to_reg(name))?;
                prev.push(Previous::Value {
                    key: key.to_string(),
                    name: name.to_string(),
                    old,
                });
            }
            Op::Service { name, start, .. } => {
                let key = format!("{SERVICES}\\{name}");
                if reg.exists(&key) {
                    let old = reg.read_key(&key).and_then(|k| k.get("Start").cloned());
                    set_service(reg, name, *start)?;
                    prev.push(Previous::Value {
                        key,
                        name: "Start".into(),
                        old,
                    });
                }
            }
            Op::Task { path } => {
                // Missing tasks (other editions) are fine.
                if platform::run_hidden("schtasks.exe", &["/Change", "/TN", path, "/Disable"])
                    .is_ok_and(|(c, _)| c == 0)
                {
                    prev.push(Previous::Task {
                        path: path.to_string(),
                    });
                }
            }
            Op::PauseUpdates { days } => {
                if !reg.exists(UX_SETTINGS) {
                    reg.create_key(UX_SETTINGS)?;
                }
                let end = iso_day(*days as i64);
                let start = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
                for name in PAUSE_VALUES {
                    let old = reg.read_key(UX_SETTINGS).and_then(|k| k.get(name).cloned());
                    let v = if name.contains("Start") { &start } else { &end };
                    reg.set_value(UX_SETTINGS, &RegValue::string(name, v))?;
                    prev.push(Previous::Value {
                        key: UX_SETTINGS.into(),
                        name: name.into(),
                        old,
                    });
                }
            }
            Op::UltimatePower => {
                let before = guid_in(&active_power_scheme()).unwrap_or(BALANCED_GUID.into());
                let out = run("powercfg.exe", &["-duplicatescheme", ULTIMATE_GUID])?;
                let new = guid_in(&out)
                    .ok_or_else(|| anyhow::anyhow!("powercfg did not create the plan"))?;
                run("powercfg.exe", &["/setactive", &new])?;
                prev.push(Previous::Power { scheme: before });
            }
        }
    }
    // Keep the oldest record if a tweak is applied twice.
    state.applied.entry(t.id.to_string()).or_insert(prev);
    Ok(())
}

/// Undo a tweak: put back what was there before, or the Windows default.
pub fn undo(t: &Tweak, reg: &dyn Registry, state: &mut State) -> anyhow::Result<()> {
    if let Some(prev) = state.applied.remove(t.id) {
        for p in prev.iter().rev() {
            match p {
                Previous::Value { key, name, old } => {
                    let is_service = fold(key).starts_with(&fold(SERVICES)) && name == "Start";
                    match old {
                        Some(v) if is_service => {
                            set_service(reg, platform::leaf_name(key), v.as_u32().unwrap_or(3))?
                        }
                        Some(v) => reg.set_value(key, v)?,
                        None => {
                            if reg.read_key(key).is_some_and(|k| k.get(name).is_some()) {
                                reg.delete_value(key, name)?;
                            }
                        }
                    }
                }
                Previous::Task { path } => {
                    let _ =
                        platform::run_hidden("schtasks.exe", &["/Change", "/TN", path, "/Enable"]);
                }
                Previous::Power { scheme } => {
                    run("powercfg.exe", &["/setactive", scheme])?;
                }
            }
        }
        return Ok(());
    }
    // Applied outside Expunge (or the record was lost): return to defaults.
    for op in t.ops {
        match op {
            Op::Reg {
                key, name, default, ..
            } => match default {
                Some(d) => reg.set_value(key, &d.to_reg(name))?,
                None => {
                    if reg.read_key(key).is_some_and(|k| k.get(name).is_some()) {
                        reg.delete_value(key, name)?;
                    }
                }
            },
            Op::Service { name, default, .. } => set_service(reg, name, *default)?,
            Op::Task { path } => {
                let _ = platform::run_hidden("schtasks.exe", &["/Change", "/TN", path, "/Enable"]);
            }
            Op::UltimatePower => run("powercfg.exe", &["/setactive", BALANCED_GUID]).map(|_| ())?,
            Op::PauseUpdates { .. } => {
                for name in PAUSE_VALUES {
                    if reg
                        .read_key(UX_SETTINGS)
                        .is_some_and(|k| k.get(name).is_some())
                    {
                        reg.delete_value(UX_SETTINGS, name)?;
                    }
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Pre-installed apps
// ---------------------------------------------------------------------------

/// Pre-installed Store apps people commonly remove: (package name, display name).
pub const BLOATWARE: &[(&str, &str)] = &[
    ("Microsoft.XboxApp", "Xbox (old)"),
    ("Microsoft.GamingApp", "Xbox"),
    ("Microsoft.Xbox.TCUI", "Xbox TCUI"),
    ("Microsoft.XboxSpeechToTextOverlay", "Xbox speech overlay"),
    (
        "Microsoft.MicrosoftSolitaireCollection",
        "Solitaire Collection",
    ),
    ("Microsoft.GetHelp", "Get Help"),
    ("Microsoft.Getstarted", "Tips"),
    ("Microsoft.WindowsFeedbackHub", "Feedback Hub"),
    ("Microsoft.MixedReality.Portal", "Mixed Reality Portal"),
    ("Microsoft.MSPaint", "Paint 3D"),
    ("Microsoft.Microsoft3DViewer", "3D Viewer"),
    ("Microsoft.ZuneMusic", "Media Player / Groove Music"),
    ("Microsoft.ZuneVideo", "Movies & TV"),
    ("Microsoft.BingNews", "News"),
    ("Microsoft.BingWeather", "Weather"),
    ("Microsoft.BingSearch", "Bing Search"),
    ("Microsoft.MicrosoftOfficeHub", "Microsoft 365 (Office hub)"),
    ("Microsoft.OutlookForWindows", "Outlook (new)"),
    ("Microsoft.People", "People"),
    ("Microsoft.WindowsMaps", "Maps"),
    ("Microsoft.SkypeApp", "Skype"),
    ("Microsoft.Todos", "Microsoft To Do"),
    ("Microsoft.PowerAutomateDesktop", "Power Automate"),
    ("Microsoft.549981C3F5F10", "Cortana"),
    ("Microsoft.Copilot", "Copilot"),
    ("Microsoft.Windows.DevHome", "Dev Home"),
    ("Clipchamp.Clipchamp", "Clipchamp"),
    ("MicrosoftTeams", "Teams (personal)"),
    ("MSTeams", "Teams"),
    ("Microsoft.YourPhone", "Phone Link"),
    ("Microsoft.WindowsSoundRecorder", "Sound Recorder"),
    ("Microsoft.WindowsAlarms", "Clock"),
];

/// Friendly name of a known bloatware package.
pub fn bloat_name(package: &str) -> Option<&'static str> {
    BLOATWARE
        .iter()
        .find(|(p, _)| p.eq_ignore_ascii_case(package))
        .map(|(_, n)| *n)
}

/// Stop new user accounts getting this app: remove it from the provisioned
/// (pre-staged) packages.
pub fn deprovision(package: &str) -> anyhow::Result<()> {
    let safe: String = package
        .chars()
        .filter(|c| c.is_alphanumeric() || "._-".contains(*c))
        .collect();
    run(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "Get-AppxProvisionedPackage -Online | Where-Object DisplayName -eq '{safe}' | Remove-AppxProvisionedPackage -Online | Out-Null"
            ),
        ],
    )
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// DNS
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DnsProvider {
    pub name: &'static str,
    pub description: &'static str,
    /// Empty = automatic (from the router / DHCP).
    pub servers: &'static [&'static str],
}

pub const DNS_PROVIDERS: &[DnsProvider] = &[
    DnsProvider {
        name: "Automatic",
        description: "Whatever your router or network provides",
        servers: &[],
    },
    DnsProvider {
        name: "Cloudflare",
        description: "Fast and private (1.1.1.1)",
        servers: &[
            "1.1.1.1",
            "1.0.0.1",
            "2606:4700:4700::1111",
            "2606:4700:4700::1001",
        ],
    },
    DnsProvider {
        name: "Quad9",
        description: "Blocks known malware domains (9.9.9.9)",
        servers: &["9.9.9.9", "149.112.112.112", "2620:fe::fe", "2620:fe::9"],
    },
    DnsProvider {
        name: "Google",
        description: "Google Public DNS (8.8.8.8)",
        servers: &[
            "8.8.8.8",
            "8.8.4.4",
            "2001:4860:4860::8888",
            "2001:4860:4860::8844",
        ],
    },
    DnsProvider {
        name: "AdGuard",
        description: "Blocks ads and trackers (94.140.14.14)",
        servers: &[
            "94.140.14.14",
            "94.140.15.15",
            "2a10:50c0::ad1:ff",
            "2a10:50c0::ad2:ff",
        ],
    },
];

/// PowerShell that sets DNS on every connected network adapter.
pub fn dns_script(p: &DnsProvider) -> String {
    let set = if p.servers.is_empty() {
        "-ResetServerAddresses".to_string()
    } else {
        format!(
            "-ServerAddresses @({})",
            p.servers
                .iter()
                .map(|s| format!("'{s}'"))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    format!(
        "Get-NetAdapter | Where-Object Status -eq 'Up' | ForEach-Object {{ Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex {set} }}; Clear-DnsClientCache"
    )
}

pub fn set_dns(p: &DnsProvider) -> anyhow::Result<()> {
    run(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", &dns_script(p)],
    )
    .map(|_| ())
}

/// Which provider the connected adapters currently use (by IPv4 address).
pub fn current_dns() -> Option<&'static DnsProvider> {
    let (_, out) = platform::run_hidden(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-NetAdapter | Where-Object Status -eq 'Up' | Get-DnsClientServerAddress -AddressFamily IPv4).ServerAddresses -join ' '",
        ],
    )
    .ok()?;
    let first = out.split_whitespace().next()?;
    DNS_PROVIDERS
        .iter()
        .find(|p| p.servers.first() == Some(&first))
}

// ---------------------------------------------------------------------------
// Maintenance commands
// ---------------------------------------------------------------------------

pub struct Repair {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub commands: &'static [(&'static str, &'static [&'static str])],
    pub advanced: bool,
}

pub const REPAIRS: &[Repair] = &[
    Repair {
        id: "sfc",
        title: "Check Windows system files",
        description: "Runs sfc /scannow, which finds and repairs damaged Windows files. Takes 5–15 minutes.",
        commands: &[("sfc.exe", &["/scannow"])],
        advanced: false,
    },
    Repair {
        id: "dism",
        title: "Repair the Windows image",
        description: "Runs DISM /RestoreHealth, which repairs the store sfc repairs from (downloads clean files from Windows Update). Takes 10–30 minutes.",
        commands: &[("dism.exe", &["/English", "/Online", "/Cleanup-Image", "/RestoreHealth"])],
        advanced: false,
    },
    Repair {
        id: "network",
        title: "Reset the network stack",
        description: "Resets Winsock and TCP/IP and clears the DNS cache, which fixes many connection problems. Needs a restart; VPN software may need reinstalling.",
        commands: &[
            ("netsh.exe", &["winsock", "reset"]),
            ("netsh.exe", &["int", "ip", "reset"]),
            ("ipconfig.exe", &["/flushdns"]),
        ],
        advanced: true,
    },
    Repair {
        id: "winget",
        title: "Repair winget",
        description: "Re-registers the App Installer package that provides winget.",
        commands: &[(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", "Add-AppxPackage -RegisterByFamilyName -MainPackage Microsoft.DesktopAppInstaller_8wekyb3d8bbwe"],
        )],
        advanced: false,
    },
];

/// sfc writes UTF-16 when redirected; turn either encoding into text.
pub fn decode_output(bytes: &[u8]) -> String {
    let nuls = bytes.iter().filter(|b| **b == 0).count();
    if bytes.len() > 4 && nuls * 3 > bytes.len() {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units).replace('\0', "")
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

/// Run a repair; returns the last meaningful line of output.
pub fn run_repair(r: &Repair, progress: &dyn Fn(String)) -> anyhow::Result<String> {
    let mut last = String::new();
    for (prog, args) in r.commands {
        progress(format!("{prog} {}", args.join(" ")));
        let mut c = std::process::Command::new(prog);
        c.args(*args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let out = c.output()?;
        let text = decode_output(&out.stdout);
        last = text
            .lines()
            .map(str::trim)
            .filter(|l| {
                !l.is_empty()
                    && !l.chars().all(|c| {
                        c == '='
                            || c == '.'
                            || c == '%'
                            || c.is_ascii_digit()
                            || c == ' '
                            || c == '['
                            || c == ']'
                    })
            })
            .next_back()
            .unwrap_or("")
            .to_string();
        if !out.status.success() && r.id != "sfc" {
            anyhow::bail!("{prog}: {last}");
        }
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::KeyData;
    use crate::platform::MemRegistry;

    #[test]
    fn tweaks_are_well_formed() {
        let mut ids = std::collections::BTreeSet::new();
        for t in TWEAKS {
            assert!(ids.insert(t.id), "duplicate id {}", t.id);
            assert!(CATEGORIES.contains(&t.category), "{}", t.id);
            assert!(!t.ops.is_empty());
        }
    }

    #[test]
    fn apply_and_undo_restore_previous_values() {
        let reg = MemRegistry::new();
        // A user who had already changed one value themselves.
        reg.put(
            KeyData::new(ADV)
                .with(RegValue::dword("Start_TrackProgs", 1))
                .with(RegValue::dword("HideFileExt", 1)),
        );
        reg.put(KeyData::new(&format!("{SERVICES}\\DiagTrack")).with(RegValue::dword("Start", 2)));
        let mut state = State::default();

        for id in [
            "app_launch_tracking",
            "explorer_defaults",
            "diagtrack",
            "advertising_id",
        ] {
            let t = get(id).unwrap();
            apply(t, &reg, &mut state).unwrap();
            assert_eq!(status(t, &reg, ""), Status::On, "{id}");
        }
        let v = |k: &str, n: &str| {
            reg.read_key(k)
                .and_then(|x| x.get(n).cloned())
                .and_then(|v| v.as_u32())
        };
        assert_eq!(v(ADV, "HideFileExt"), Some(0));
        assert_eq!(v(&format!("{SERVICES}\\DiagTrack"), "Start"), Some(4));

        // Round trip through the state file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tweaks.json");
        state.save(&path).unwrap();
        let mut state = State::load(&path);

        for id in ["explorer_defaults", "diagtrack", "advertising_id"] {
            undo(get(id).unwrap(), &reg, &mut state).unwrap();
        }
        assert_eq!(v(ADV, "HideFileExt"), Some(1));
        assert_eq!(
            v(ADV, "LaunchTo"),
            None,
            "value that didn't exist is removed again"
        );
        assert_eq!(v(&format!("{SERVICES}\\DiagTrack"), "Start"), Some(2));
        assert_eq!(
            v(
                "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\AdvertisingInfo",
                "DisabledByGroupPolicy"
            ),
            None
        );
        assert!(state.applied.contains_key("app_launch_tracking"));
        assert!(!state.applied.contains_key("diagtrack"));
    }

    #[test]
    fn undo_without_record_returns_to_defaults() {
        let reg = MemRegistry::new();
        reg.put(KeyData::new(ADV).with(RegValue::dword("Start_TrackProgs", 0)));
        let mut state = State::default();
        undo(get("app_launch_tracking").unwrap(), &reg, &mut state).unwrap();
        assert_eq!(
            reg.read_key(ADV)
                .unwrap()
                .get("Start_TrackProgs")
                .and_then(|v| v.as_u32()),
            Some(1)
        );
    }

    #[test]
    fn pause_updates_round_trip() {
        let reg = MemRegistry::new();
        let mut state = State::default();
        let t = get("pause_updates").unwrap();
        assert_eq!(status(t, &reg, ""), Status::Off);
        apply(t, &reg, &mut state).unwrap();
        assert_eq!(status(t, &reg, ""), Status::On);
        undo(t, &reg, &mut state).unwrap();
        assert_eq!(status(t, &reg, ""), Status::Off);
        assert!(reg
            .read_key(UX_SETTINGS)
            .unwrap()
            .get("PauseUpdatesExpiryTime")
            .is_none());
    }

    #[test]
    fn helpers() {
        assert_eq!(
            guid_in("Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)").unwrap(),
            BALANCED_GUID
        );
        assert!(dns_script(&DNS_PROVIDERS[1]).contains("'1.1.1.1','1.0.0.1'"));
        assert!(dns_script(&DNS_PROVIDERS[0]).contains("-ResetServerAddresses"));
        let utf16: Vec<u8> = "Verification 100% complete.\r\n"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert!(decode_output(&utf16).contains("Verification 100% complete."));
        assert_eq!(
            bloat_name("microsoft.zunemusic"),
            Some("Media Player / Groove Music")
        );
    }
}
