//! User settings, persisted as JSON in the data folder.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FileRoot {
    pub path: String,
    pub recursive: bool,
}

impl FileRoot {
    fn new(path: &str, recursive: bool) -> Self {
        FileRoot { path: path.into(), recursive }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemePref,
    /// UI zoom factor.
    pub zoom: f32,
    /// Registry keys captured by snapshots (whole subtrees).
    pub registry_roots: Vec<String>,
    /// Folders captured by snapshots. `%VAR%` references are expanded.
    pub file_roots: Vec<FileRoot>,
    /// Glob patterns for registry noise to ignore.
    pub registry_exclusions: Vec<String>,
    /// Glob patterns for file noise to ignore.
    pub file_exclusions: Vec<String>,
    /// Copy everything to a backup before deleting it.
    pub backup_before_removal: bool,
    /// Create a System Restore point before uninstalling.
    pub create_restore_point: bool,
    /// When removing a logged program folder, also remove files created in it
    /// after installation (settings, caches). Off = keep them.
    pub remove_unlogged_files: bool,
    /// Ask before destructive actions.
    pub confirm_actions: bool,
    /// Show entries flagged as SystemComponent in the program list.
    pub show_system_components: bool,
    /// Show Microsoft Store apps in the program list.
    pub show_store_apps: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemePref::System,
            zoom: 1.0,
            registry_roots: default_registry_roots(),
            file_roots: default_file_roots(),
            registry_exclusions: default_registry_exclusions(),
            file_exclusions: default_file_exclusions(),
            backup_before_removal: true,
            create_restore_point: false,
            remove_unlogged_files: false,
            confirm_actions: true,
            show_system_components: false,
            show_store_apps: false,
        }
    }
}

pub fn default_registry_roots() -> Vec<String> {
    [
        "HKLM\\SOFTWARE",
        "HKLM\\SYSTEM\\CurrentControlSet\\Services",
        "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
        "HKCU\\Software",
        "HKCU\\Environment",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub fn default_file_roots() -> Vec<FileRoot> {
    vec![
        FileRoot::new("%ProgramFiles%", true),
        FileRoot::new("%ProgramFiles(x86)%", true),
        FileRoot::new("%ProgramData%", true),
        FileRoot::new("%APPDATA%", true),
        FileRoot::new("%LOCALAPPDATA%", true),
        FileRoot::new("%USERPROFILE%\\AppData\\LocalLow", true),
        FileRoot::new("%PUBLIC%\\Desktop", true),
        FileRoot::new("%USERPROFILE%\\Desktop", false),
        FileRoot::new("%SystemRoot%\\System32", false),
        FileRoot::new("%SystemRoot%\\System32\\drivers", false),
        FileRoot::new("%SystemRoot%\\SysWOW64", false),
        FileRoot::new("%SystemRoot%\\System32\\Tasks", true),
        FileRoot::new("%SystemRoot%\\Fonts", false),
    ]
}

/// Registry locations Windows itself changes constantly; capturing them
/// would bury the real installer changes in noise.
pub fn default_registry_exclusions() -> Vec<String> {
    [
        "HKLM\\SOFTWARE\\Microsoft\\Windows Defender",
        "HKLM\\SOFTWARE\\Microsoft\\Windows Defender Security Center",
        "HKLM\\SOFTWARE\\Microsoft\\Windows Search",
        "HKLM\\SOFTWARE\\Microsoft\\Cryptography\\RNG",
        "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Perflib",
        "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Notifications",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Diagnostics",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\AppModel",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CloudStore",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Reliability",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Authentication",
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Component Based Servicing",
        "HKLM\\SOFTWARE\\Microsoft\\SecurityManager",
        "HKLM\\SOFTWARE\\Microsoft\\SQMClient",
        "HKLM\\SOFTWARE\\Microsoft\\Tracing",
        "HKLM\\SOFTWARE\\Microsoft\\WlanSvc",
        "HKLM\\SOFTWARE\\Microsoft\\Provisioning",
        "HKLM\\SOFTWARE\\Microsoft\\EdgeUpdate",
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate",
        "HKLM\\SYSTEM\\CurrentControlSet\\Services\\bam",
        "HKLM\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces",
        "HKLM\\SYSTEM\\CurrentControlSet\\Services\\NlaSvc\\Parameters\\Internet",
        "HKLM\\SYSTEM\\CurrentControlSet\\Services\\SharedAccess\\Epoch",
        "HKLM\\SYSTEM\\CurrentControlSet\\Services\\*\\Parameters\\Tcpip",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\UserAssist",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\RecentDocs",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\ComDlg32",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FeatureUsage",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SessionInfo",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\TypedPaths",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\RunMRU",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\MountPoints2",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartPage2",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\BamThrottling",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Streams",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\CloudStore",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Notifications",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\PushNotifications",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Search",
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Store",
        "HKCU\\Software\\Microsoft\\Windows\\Shell\\BagMRU",
        "HKCU\\Software\\Microsoft\\Windows\\Shell\\Bags",
        "HKCU\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\BagMRU",
        "HKCU\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\Bags",
        "HKCU\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache",
        "HKCU\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel",
        "HKCU\\Software\\Microsoft\\Windows NT\\CurrentVersion\\AppCompatFlags\\Compatibility Assistant",
        "HKCU\\Software\\Microsoft\\Internet Explorer",
        "HKCU\\Software\\Microsoft\\Edge",
        "HKCU\\Software\\Microsoft\\OneDrive",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub fn default_file_exclusions() -> Vec<String> {
    [
        "%TEMP%",
        "%LOCALAPPDATA%\\Temp",
        "%SystemRoot%\\Temp",
        "%LOCALAPPDATA%\\Packages",
        "%LOCALAPPDATA%\\Microsoft\\Windows\\INetCache",
        "%LOCALAPPDATA%\\Microsoft\\Windows\\Explorer",
        "%LOCALAPPDATA%\\Microsoft\\Windows\\WebCache",
        "%LOCALAPPDATA%\\Microsoft\\Windows\\Notifications",
        "%LOCALAPPDATA%\\Microsoft\\Windows\\History",
        "%LOCALAPPDATA%\\Microsoft\\Edge\\User Data",
        "%LOCALAPPDATA%\\Microsoft\\OneDrive\\logs",
        "%LOCALAPPDATA%\\Google\\Chrome\\User Data",
        "%LOCALAPPDATA%\\BraveSoftware\\Brave-Browser\\User Data",
        "%LOCALAPPDATA%\\Mozilla\\Firefox\\Profiles",
        "%APPDATA%\\Mozilla\\Firefox\\Profiles",
        "%LOCALAPPDATA%\\D3DSCache",
        "%LOCALAPPDATA%\\CrashDumps",
        "%LOCALAPPDATA%\\ConnectedDevicesPlatform",
        "%LOCALAPPDATA%\\NVIDIA\\DXCache",
        "%LOCALAPPDATA%\\NVIDIA\\GLCache",
        "%APPDATA%\\Microsoft\\Windows\\Recent",
        "%ProgramData%\\Microsoft\\Windows Defender",
        "%ProgramData%\\Microsoft\\Search",
        "%ProgramData%\\Microsoft\\Diagnosis",
        "%ProgramData%\\Microsoft\\Windows\\WER",
        "%ProgramData%\\Microsoft\\Windows\\AppRepository",
        "%ProgramData%\\Microsoft\\Network",
        "%ProgramData%\\Packages",
        "%ProgramData%\\USOPrivate",
        "%ProgramData%\\USOShared",
        "%ProgramFiles%\\WindowsApps",
        "%ProgramFiles%\\Windows Defender Advanced Threat Protection",
        "*.etl",
        "*\\desktop.ini",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
