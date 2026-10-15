# Changelog

All notable changes to Expunge are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Portable App Builder** (Tools > Portable App Builder; CLI `expunge portable-build`): turns a program's installer into a PortableApps.com Format package. Expunge snapshots Windows, watches for 30 seconds to learn background noise, runs the installer (silently for MSI, NSIS and Inno Setup when asked), lets you start the program once so its first-run settings are captured, snapshots again and builds the package from the difference: the program's files in `App\AppName`, `appinfo.ini`, icons taken from the program, `help.html`, the Launcher settings (`[RegistryKeys]`, `[DirectoriesMove]`, `[FilesMove]` and clean-ups, with paths written as Launcher variables) and, optionally, the first-run settings as `DefaultData`. The capture runs in **Windows Sandbox** when it is turned on, so the PC is never touched; otherwise it runs on this PC and is recorded as an install log so the program can be removed precisely afterwards. With the PortableApps.com Launcher and Installer installed, Expunge also compiles the launcher and the `.paf.exe` installer.
- **PortableApps.com Launcher settings export** from any snapshot comparison (Snapshots page) or install log (Export menu; CLI `expunge export <log-id> portable <file.ini>`), leaving out shortcuts, installer bookkeeping and the program's own folder.
- **Snapshots page** with a quick before-and-after compare: one click before, one click after, and the differences appear, ready to save as an install log or export.
- **System Restore manager** (Tools > System Restore): list restore points with their type, create one with your own description (Windows' once-a-day limit is lifted for it), roll Windows back to a point, open Windows' own wizard, and in Advanced mode delete restore points, turn System Protection on or off and set how much disk space restore points may use.
- **Merge install logs** (Install logs > *Merge with*; CLI `expunge merge`): combine a program and its update or plugins into one log; later changes win and the originals are kept.
- **Add a program by hand** (Programs > *Add program…*; CLI `expunge add-program`): list a program Windows doesn't know about, such as a portable program, by its folder, so it can be analyzed and uninstalled with its leftovers.
- **Monitoring profiles**: save monitoring scopes under a name (Settings > Monitoring scope) and switch between them on the Monitor and Snapshots pages; `--profile NAME` on the command line.
- **XML export** of install logs and **Print** (opens a print-ready report and the print dialog).
- **Korean and Polish** translations (11 languages in total), also in the installer.
- **Integration tests with real installers** (GitHub Actions, weekly, on release tags and on demand): a Windows Installer package, an NSIS installer and an Inno Setup installer are each installed under monitoring on a clean Windows runner, removed with Expunge, and checked to leave nothing behind. New CLI helpers: `expunge check-clean <log-id>` and `expunge revert --keep-log`.
- **Live activity** while monitoring an installation (Monitor page): files being added, changed and removed stream in as they happen, and registry areas report when something under them changes, using Windows' own change notifications (no driver). Excluded noise is filtered out; the exact list, including registry values, still comes from the final comparison.
- **Background auto-monitoring** (Advanced mode; Settings > *Background monitoring*): a low-priority `expunge watch` process started at sign-in keeps a snapshot and, when a new program registers itself, waits for the installation to settle and saves an install log tagged "auto", so even installations you forgot to monitor can be removed precisely. It steps aside while manual monitoring is running and refreshes its snapshot daily and after uninstalls.
- **Arm64 builds**: releases include a portable build for Windows 11 on Arm (Snapdragon and other Arm PCs), and CI builds it on every push.
- **Startup impact**: the Startup page shows how long the last Windows startup took and how much each startup program slowed it down (High, Medium or Low), measured by Windows' own boot diagnostics (needs Administrator rights). Also in `expunge startup`.
- **Delayed start**: *Delay* starts a startup program 2 minutes after you sign in instead of immediately, so Windows is ready sooner. It uses a Task Scheduler task and switches the original entry off without deleting it; *Start normally* undoes it exactly.
- **Browser privacy cleaning** in the Junk cleaner, each item off until you tick it: cookies, browsing history, website data (local storage, databases, offline data) and saved sessions for Chrome, Edge, Brave, Vivaldi, Opera and Firefox, LibreWolf, Waterfox and Floorp, in every profile. Firefox history is kept because it shares a file with bookmarks (its form history is removed). Plus the Windows font cache (Advanced mode).
- **Scheduled junk cleaning** (Cleaner page > *Clean automatically*; CLI `expunge schedule-clean daily|weekly|monthly|off`): cleans the ticked kinds of junk every day, week or month through Windows Task Scheduler, even when Expunge is closed. What each run freed is recorded in `cleaning.log` in the data folder.
- **Complete command line** for unattended maintenance: `expunge uninstall <name>... [--silent]` uninstalls programs one after another and removes their safe leftovers (backed up); `expunge clean [--categories] [--list] [--dry-run]` cleans junk; `expunge startup [enable|disable <name>]` lists and toggles startup programs (essential entries are protected).
- **Groups and tags for install logs**: give any log a group and tags (Install logs > Edit details). The list is arranged by group, shows tags under each name, can be limited to one group, and search also matches groups and tags. CLI: `expunge logs --group G --tag T [--json]`, `expunge tag <log-id> --group G --tags a,b`.
- Moving files to the Recycle Bin now asks before permanently deleting a file that is too large for it.
- **Install with Expunge** in the right-click menu of `.exe` and `.msi` files: installs under monitoring in one step. Added for all users by Setup, or per user from Settings (portable). Relaunching as administrator keeps the installer.
- Program icons in the program list and details.
- **Program trust info**: each program's digital signature is verified offline (Authenticode). Shows who signed it, flags unsigned programs (new "Unsigned" filter) and broken signatures, and warns before uninstalling a program whose signature fails.
- **Simple mode only offers safe options.** Hidden until you switch to Advanced mode: forced removal, system components, cleaner items that slow the PC while caches rebuild (shader, thumbnail, browser and Windows Update caches), deleting startup entries (broken ones excepted), and the switches that turn off backups and confirmations. Startup entries that Windows, security software or hardware need can't be disabled. Removal lists leave out items marked Careful and uncertain matches the scanner leaves unticked (ordinary leftovers such as shortcuts and the program's folders stay visible).
- **Deeper leftover scan**: besides folders, shortcuts and registry keys, finds everything else a removed program registered in its install folder: COM classes and type libraries, file types and associations, "Open with" entries, context-menu verbs and shell extensions, environment variables and `PATH` entries (only the program's own entries are removed from `PATH`), Windows Firewall rules, scheduled tasks, and folders that would be left empty (such as the publisher's folder). Registry data is read once per scan, so batch uninstalls stay fast. Items that affect shared parts of Windows are labelled *Careful* and appear only in Advanced mode.
- **Transfer programs to another PC**: pick one or more programs (Programs page: *Transfer to another PC…*, or tick several and *Transfer*), and Expunge packs everything that belongs to them — exactly from the install log for monitored programs, otherwise found with the leftover scanner (program folder, data folders, shortcuts, registry entries, file associations, PATH entries, services, scheduled tasks) — into one `.expunge` file. On the new PC, *File > Restore programs from a package* checks compatibility first (processor type, Windows version, missing drives, files that already exist, Administrator rights), puts files where they belong even with a different user name or drive, rewrites paths inside the registry to match, and records every change so the whole transfer can be undone from the Backups page. Programs known not to survive a copy (drivers, security software, machine-bound activation) are flagged. CLI: `expunge transfer export|check|import`.
- **Other autostarts** (Startup page, Advanced mode): the autostart locations beyond Run keys, Startup folders, services and tasks — Winlogon Shell/Userinit changes, Active Setup, program replacements (Image File Execution Options), AppInit DLLs, Explorer extensions and icon overlays, browser helper objects, boot-time programs and WMI event subscriptions. Entries whose file is gone are flagged; removable ones can be ticked and removed with the usual review, risk labels and backup. Windows' own entries are hidden unless asked for.
- **Deep cleanup** in the Junk cleaner (Advanced mode, Administrator), covering what Windows Slimmer–style tools trim, always with Windows' own tools: the previous Windows installation (Windows.old and upgrade leftovers), outdated component-store files (DISM component cleanup), files of turned-off Windows features, and every restore point except the newest. *Analyze* shows what each would free first; each is explained and confirmed separately, with an optional restore point before it runs. A link leads to removing unneeded pre-installed Store apps.
- New junk categories: Windows setup and update logs, system memory dumps, the Delivery Optimization cache, and Prefetch data (Advanced mode, since programs start a little slower afterwards).
- The program list's columns now always fit, so Size is never cut off.
- **Find leftovers of removed programs** (Tools menu and dashboard, Advanced mode; CLI `expunge orphans`): a system-wide scan for what programs uninstalled long ago left behind — COM classes and type libraries, file types and "Open with" entries, context-menu verbs, shell extensions whose class is gone, PATH entries and environment variables, App Paths, firewall rules and scheduled tasks — found because they point to files or folders that no longer exist. Windows' own entries are never included, 32-bit programs' paths are checked in both Program Files folders, and everything goes through the usual review, risk labels and backup.
- **Unattended batch uninstall**: tick any number of programs, choose *Unattended*, and walk away. Expunge recognises each uninstaller's technology (Windows Installer, Inno Setup, NSIS, WiX bundles, Squirrel, publisher-declared silent mode, Store apps) and adds the right silent and no-restart switches; runs uninstallers strictly one at a time and waits while any other Windows Installer job is running (retrying on error 1618); moves on if one hangs for 15 minutes or finishes without actually removing the program; and reports restarts that are needed at the end. Programs that can't run silently can be skipped. A live list shows each program's progress, and one combined leftover scan follows. Single programs get an *Uninstall silently* button.
- **Faster and lighter**: snapshots scan folders on several threads at once while reading the registry; program sizes and signature checks are cached between launches, so the program list fills in immediately after the first run; install logs and backups load in the background, and saving a large log or deleting a large backup no longer pauses the window; install logs are stored as compact JSON (smaller files). Snapshots and Settings no longer re-read the disk or registry on every frame. CI now records snapshot time, peak memory and signature-check time against budgets.
- **Full keyboard accessibility**: a clear focus ring follows Tab / Shift+Tab; arrow keys, Home/End and Page Up/Down move through the program and install-log lists; Space ticks programs for batch uninstall; Esc closes every dialog and window; Ctrl+Tab cycles pages; F10 opens the menu bar; Ctrl +/−/0 zoom is remembered. Dialogs start on their main button. Custom controls (navigation, dashboard tiles and actions, list items, checkboxes) are named for screen readers such as Narrator and NVDA. All shortcuts are listed in Settings → Keyboard.
- **Automatic exclusions**: a built-in, always-current list of volatile locations (temporary files, Windows logs and traces, shell and shader caches, and the caches of every Chromium and Firefox-based browser and Electron app, any profile) is ignored by snapshots, so install logs contain only the installer's real changes and snapshots run faster. On by default; can be turned off in Advanced mode.
- Exclusion patterns are compiled once per snapshot instead of for every file and key: matching is about 50× faster, so snapshots finish sooner.
- **Learn background noise** (Settings, Advanced mode; CLI `expunge learn-noise`): two snapshots a minute apart reveal what this PC changes on its own, and the findings can be added to the exclusions with one click. Never suggests excluding a whole monitored location.
- CLI: `expunge verify <file>` and `expunge programs --trust`.

### Removed
- Expunge now focuses on being a complete, free replacement for Total Uninstall. Tools outside that scope were removed: the Anti-Telemetry page, the disk space map, the duplicate file finder, the Windows Updates remover, winget updates, the system health panel, the online reputation check, installer lint and the uninstaller test, repackaging and MSIX output, uninstall scripts, the Windows features and app aliases views, and Find by window.

### Changed
- The project moved to **github.com/SeanShaffer88/Expunge** (formerly ExpungeUninstaller); old links redirect.
- The README now describes every tool in its own section.
- **Windows 11 only.** Expunge is engineered for Windows 11 (build 22000 or later): on older Windows the app and the installer explain this and stop, instead of running with rules meant for Windows 11.
- Darker red accent (#6B0700) across the interface and window border; red text uses a lighter shade of it for readability.
- "Administrator" is capitalized throughout.
- New icon in #6B0700: dark card with a red E at large sizes, and an inverted red card with a black E at taskbar sizes (16–48 px), for the window and sidebar.
- Backups also export registry entries as a double-clickable `.reg` file; optional Recycle Bin.

### Fixed
- Cleaner wildcard paths that mix `/` and `\` on Windows.

## [0.1.0] - 2026-10-14

First public release.

### Added
- Install monitoring: before/after snapshots of the registry and file system, run an installer under monitoring, or monitor manually; sessions survive restarts.
- Review captured changes, remove noise, and save them as an install log with auto-detected name, publisher and version.
- Precise uninstall from an install log: removes added files, folders, keys and values; restores changed and removed values; stops and deletes services; removes scheduled tasks; repairs `PATH` without touching later edits.
- Program list with 64-bit, 32-bit, per-user and Microsoft Store programs, sorting, search, details, real folder sizes, and a broken-entry finder.
- Uninstall wizard with vendor uninstaller, quiet uninstall, forced removal, batch uninstall, and leftover scanning.
- Backups of everything removed or changed (quarantined files plus a double-clickable `.reg` file for registry entries), with one-click restore.
- Optional: also send removed files to the Recycle Bin.
- Optional System Restore point before uninstalling.
- Windows Updates window: list installed updates and remove individual ones via DISM or WUSA.
- Startup manager for Run keys, Startup folders, services and scheduled tasks.
- Junk cleaner for temp files, crash dumps, caches, Windows Update downloads and the Recycle Bin.
- "Which program created this?" search across install logs.
- Export install logs as HTML report, `.reg` undo script, PowerShell removal script or JSON; import logs.
- Dashboard with status and quick actions.
- Simple and Advanced interface modes, first-run welcome, page help (F1), safety labels on removal items.
- Accessibility: keyboard shortcuts, high-contrast option, adjustable text size, screen-reader support.
- Installer Lab for developers: installer lint, uninstaller test with residue report, named snapshots and comparison.
- CLI `lint` and `test-uninstall` commands and `--json` output for CI pipelines.
- Dark interface with a native red Windows 11 window border and left-hand navigation.
- Translations: English, Spanish, German, French, Portuguese (Brazil), Italian, Russian, Simplified Chinese, Japanese; external `lang/*.json` files supported.
- Admin-rights check at every launch with relaunch as administrator.
- Command-line interface (`monitor`, `snapshot`, `diff`, `logs`, `programs`, `revert`, `export`, `backups`, `restore`).
- Installer (NSIS) and portable zip; single exe with no runtime dependencies.

[Unreleased]: https://github.com/SeanShaffer88/Expunge/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/SeanShaffer88/Expunge/releases/tag/v0.1.0
