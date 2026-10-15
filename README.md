<p align="center">
  <img src="assets/logo-wide.png" alt="Expunge" width="600">
</p>

<p align="center">
  <b>Open-source install monitor, uninstaller, and privacy toolkit for Windows 11</b><br>
  Record exactly what an installation changes, then remove exactly that, and nothing else.
</p>

<p align="center">
  <a href="https://github.com/SeanShaffer88/Expunge/releases/latest"><img alt="Download" src="https://img.shields.io/github/v/release/SeanShaffer88/Expunge?label=download&color=e53935"></a>
  <a href="https://github.com/SeanShaffer88/Expunge/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/SeanShaffer88/Expunge/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: GPL v3" src="https://img.shields.io/badge/license-GPLv3-blue"></a>
  <img alt="Platform: Windows 11" src="https://img.shields.io/badge/platform-Windows%2011-0078d4">
</p>

<p align="center">
  <a href="#overview">Overview</a> •
  <a href="#the-tools">The tools</a> •
  <a href="#download">Download</a> •
  <a href="#getting-started">Getting started</a> •
  <a href="#command-line">Command line</a> •
  <a href="#building-from-source">Building</a>
</p>

---

## Overview

Most uninstallers run the vendor's own uninstaller and then *guess* at what was left behind. Snapshot tools such as Regshot can show a before-and-after comparison, but they stop there.

**Expunge closes that gap.** It takes a snapshot of the system before and after an installation and records every file, folder, registry key and value, service, and scheduled task that changed. Later, it turns that record into a precise, reviewable removal plan. Everything Expunge deletes or modifies is backed up first, so its changes can be undone.

Beyond uninstalling, Expunge brings together the tools needed to keep Windows 11 lean and private: a startup manager, a disk space map, a duplicate file finder, a junk cleaner, a debloater, and an Anti-Telemetry page with reversible privacy settings.

Expunge is a free, GPL-licensed alternative to commercial tools such as Total Uninstall Pro. It is approachable enough for someone new to computers and thorough enough for developers testing their own installers.

## The tools

Expunge is organized into pages, each with its own job. Every page has built-in help (the **Help** button or **F1**), and anything that could harm Windows or slow the PC appears only in **Advanced mode**.

### Dashboard

<img src="docs/screenshots/dashboard.png" alt="Dashboard" width="720">

Your system at a glance.

- **Summary tiles:** installed programs, monitored programs, broken entries, and backups (with the space they use). Click a tile to jump straight to it.
- **System health:** free space on every drive with its 30-day trend and a small chart, a link to the disk space map when a drive is nearly full, memory use, and any services that stopped unexpectedly in the last week.
- **Quick actions:** install a program with monitoring, uninstall programs, speed up startup, clean junk files, find leftovers of removed programs (Advanced mode), and manage Windows updates (Advanced mode).
- **Recently monitored:** the latest install logs, one click away.

### Programs (uninstaller)

<img src="docs/screenshots/programs.png" alt="Programs" width="720">

Every installed program, and the safest way to remove it.

- **Complete program list:** 64-bit, 32-bit, and per-user desktop programs with their icons, plus Microsoft Store apps. Search, sort by name, publisher, version, date, or size, and filter by *Updates*, *Monitored*, *Broken*, *Unsigned*, or *Store apps*.
- **Program details:** publisher, version, install date, size, location, digital signature, uninstaller, and registry key, with buttons to open its folder, open its registry key, search online, or visit its website.
- **Precise uninstallation of monitored programs:** added files, folders, registry keys, and values are deleted; values the installer changed or removed are **restored**; services are stopped and deleted; scheduled tasks are removed; and only the `PATH` entries the installer added are taken out.
- **Classic uninstallation with a deep leftover scan:** after the vendor's uninstaller finishes, Expunge looks for remaining folders, shortcuts, registry keys, autostart entries, App Paths, services, COM registrations, file types and associations, context-menu entries, shell extensions, environment variables, `PATH` entries, firewall rules, scheduled tasks, and folders left empty. Every item is labeled *Safe*, *Review*, or *Careful* before anything is removed.
- **Unattended batch uninstallation:** tick any number of programs and walk away. Silent switches are detected for each installer technology (Windows Installer, Inno Setup, NSIS, WiX, and Squirrel); uninstallers run one at a time; restarts are held until the end; stalled uninstallers are skipped; and one combined leftover scan follows.
- **Silent uninstallation** of a single program, and **forced removal** (Advanced mode) when a program's uninstaller is broken or missing.
- **Find by window:** click *Find by window*, then click any window, and Expunge selects the program it belongs to. Dropping a shortcut or program file onto the page does the same.
- **Trust information:** who digitally signed each program, with unsigned programs and broken signatures (possible tampering) flagged.
- **Updates through winget:** see which programs have newer versions and update one or all of them silently.
- **Pre- and post-uninstall scripts** (Advanced mode): your own commands that run before and after a specific program is removed, for example to export its settings or stop a service.
- **Windows features and app aliases** (Advanced mode): turn optional Windows features on or off, and see which app execution aliases are active.
- **Locked files** are scheduled for deletion at the next restart, and **user data is respected**: files a program created later are kept unless you choose otherwise.
- **Transfer to another PC:** pack programs with their files, settings, and registry entries into one file and restore them on a new PC, with a compatibility check, path rewriting for different user names or drives, and one-click undo.

### Monitor (install monitoring)

Record an installation so it can be removed precisely later.

- **Install and monitor:** choose an installer, or right-click it in File Explorer and select **Install with Expunge**. Expunge takes a snapshot, runs the installer, and records every change.
- **Manual monitoring** for anything else, including Microsoft Store apps, winget packages, scripts, and portable applications.
- **Live activity:** files being added, changed, and removed stream in while the installer runs, and registry areas report when something under them changes.
- **Restart-safe:** monitoring survives restarts, so installers that need a reboot are captured completely.
- **Review before saving:** inspect every captured change and discard background noise with one click.
- **Automatic detection** of the program's name, publisher, and version.
- **Automatic exclusions:** temporary files, logs, and browser and application caches are ignored, so each log contains only the installer's real changes. **Learn background noise** (Settings) finds what your PC changes on its own and excludes that too.
- **Background auto-monitoring** (Advanced mode, optional): a low-priority process started at sign-in records installations automatically, even the ones you forgot to monitor.

### Install logs

Everything each monitored installation changed.

- **Complete records** of added, changed, and removed files, folders, registry keys and values, services, and scheduled tasks.
- **Groups and tags:** organize logs into groups and label them with tags; search and filter by either.
- **"Which program created this?":** type part of any file or registry path to find the monitored program it belongs to.
- **Export** a log as an HTML report, a `.reg` undo script, a PowerShell removal script, or JSON for import on another PC.
- **Repackaging:** turn a monitored installation into a deployable folder with a silent `install.ps1`, an `install.reg`, and an `uninstall.ps1`, or into an MSIX package (Advanced mode).
- **Uninstall from the log** with the same precise removal as the Programs page.

### Backups

Undo any removal, cleanup, or startup change.

- **Automatic backups** of everything Expunge removes or changes. Files go to a quarantine folder, and registry entries are also exported to a timestamped `.reg` file that can be restored with a double-click, even without Expunge.
- **One-click restore** of any operation, putting everything back exactly where it was.
- **Optional extras:** a System Restore point before uninstalling, and copies of removed files in the Recycle Bin.

### Startup

<img src="docs/screenshots/startup.png" alt="Startup manager" width="720">

Choose what starts with Windows.

- **Startup programs** from Run keys and Startup folders, turned on and off exactly like Task Manager does, so the change is reversible.
- **Startup impact:** how long the last Windows startup took and how much each program slowed it down (High, Medium, or Low), measured by Windows itself.
- **Delayed start:** start a program two minutes after you sign in instead of immediately, so Windows is ready sooner. *Start normally* undoes it exactly.
- **Reputation check** (optional, off by default): look a startup program up online by its SHA-256 fingerprint. Only the hash is sent, never the file; an optional free VirusTotal key adds antivirus detection counts.
- **Services and scheduled tasks** (Advanced mode), with Windows' own entries hidden unless you ask for them.
- **Other autostarts** (Advanced mode): Winlogon, Active Setup, program replacements (IFEO), AppInit DLLs, Explorer extensions, icon overlays, browser helper objects, boot-time programs, and WMI subscriptions. Entries whose file is gone are flagged.
- **Protection:** entries that Windows, security software, or hardware need can't be switched off in Simple mode, and deleted entries are backed up.

### Cleaner (junk cleaner)

<img src="docs/screenshots/cleaner.png" alt="Junk cleaner" width="720">

Free the space taken by temporary files and caches.

- **Junk categories:** your and Windows' temporary files, crash dumps and error reports, system memory dumps, Windows setup and update logs, Delivery Optimization files, Windows Update downloads, shader and thumbnail caches, browser caches, Prefetch data, the font cache, and the Recycle Bin. Each one is explained, and *Scan* shows how much space it uses before anything is deleted.
- **Browser privacy cleaning** (each item off until you tick it): cookies, browsing history, website data, and saved sessions for Chrome, Edge, Brave, Vivaldi, Opera, Firefox, LibreWolf, Waterfox, and Floorp, in every profile. Bookmarks and passwords are never touched.
- **Clean automatically** every day, week, or month through Windows Task Scheduler, even when Expunge is closed.
- **Deep cleanup** (Advanced mode): the previous Windows installation (Windows.old), superseded Windows components, files of turned-off features, and old restore points, using Windows' own tools. Each item is analyzed first and confirmed separately.

### Anti-Telemetry

<img src="docs/screenshots/anti-telemetry.png" alt="Anti-Telemetry" width="720">

Stop Windows collecting data, remove clutter, and tune Windows. Every switch shows whether it is on, off, or partly on.

- **Telemetry and data collection:** send only the required diagnostic data; turn off the advertising ID, app-launch tracking, typing and handwriting data collection, Bing and Cortana in Windows Search, and the inventory and compatibility reporters.
- **Privacy and app permissions:** deny Store apps access to the camera, microphone, location, contacts, calendar, call history, email, and messages; turn off location services; stop Store apps running in the background; and remove tips, ads, and automatically installed apps.
- **Bloatware and AI features:** remove pre-installed consumer apps (optionally for new user accounts as well), and turn off Windows Copilot and Recall.
- **Explorer and search:** the full right-click menu, File Explorer opening on *This PC* with file extensions and hidden files visible, no transparency effects, and (Advanced mode) no search indexer.
- **Services:** turn off DiagTrack, dmwappushservice, Windows Error Reporting, and optionally SysMain.
- **Windows Update:** delay feature updates by a year and security updates by four days, or pause updates for seven days.
- **Network:** no automatic connections to open hotspots, and one-click secure DNS (Cloudflare, Quad9, Google, or AdGuard, or back to automatic).
- **Performance and repair:** the Ultimate Performance power plan, plus System File Checker, DISM RestoreHealth, a network reset, and winget re-registration.
- **Safeguards:** Minimal, Standard, and Advanced presets (Simple mode offers only Minimal, which can't break anything); a restore point before every change; and a record of every previous value, so unticking a switch and applying again restores it exactly.

### Disk space

<img src="docs/screenshots/disk-space.png" alt="Disk space map" width="720">

See which folders and files take up the most space.

- **Treemap of any drive or folder:** each block's size matches the space it uses, nested several levels deep and colored by file type (videos, music, pictures, archives and disk images, programs, documents).
- **Navigate by clicking:** click a block to look inside a folder; a breadcrumb bar and Backspace take you back up.
- **Size-sorted list** beside the map, for keyboard and screen-reader users, with *Show in File Explorer* for the selected item.
- **Fast and light:** folders are measured in parallel, links are not followed, and memory stays small even for a whole drive.
- **Read-only:** nothing is deleted here, so it is safe in Simple mode.

### Duplicate files

Find identical copies of files and free the space they waste.

- **Compares contents, not names:** files are grouped by size, compared by a quick hash, and confirmed with a full SHA-256, so only truly identical files are listed.
- **Searches your personal folders** (Desktop, Documents, Downloads, Pictures, Music, and Videos) or any folder you add, ignoring files smaller than the size you choose.
- **Smart selection:** tick every copy except the oldest, the newest, or the one with the shortest path, or choose yourself.
- **Safety:** Windows, program, and app-data folders are never searched; cloud-only files are never downloaded; the last copy of a file can never be ticked; every file is checked again right before removal; and removed copies go to the Recycle Bin.

### Windows Updates (Advanced mode)

- **Installed updates** listed with their KB numbers and names, searchable.
- **Uninstall an update** that causes problems, using Windows' own DISM tool, with an optional restore point first. Permanent servicing-stack updates are clearly labeled.

### Installer Lab (Advanced mode, for developers)

- **Installer lint:** reviews a recorded installation for common packaging mistakes, such as a missing or incomplete uninstall entry, files written to System32, per-user data from per-machine installations, `PATH` edits, autostart entries, services, scheduled tasks, shell extensions, file associations, firewall rules, and changes to shared values. Each finding includes advice, and the report can be exported as Markdown.
- **Uninstaller test:** runs your program's own uninstaller and reports exactly what it left behind or failed to restore, then optionally cleans it up.
- **Snapshot comparison:** take named snapshots at any time and compare any two.
- **CI-ready command line:** `lint`, `test-uninstall`, and `check-clean` commands with exit codes and `--json` output for build pipelines.

### Settings and accessibility

- **Simple and Advanced modes**, chosen on first launch. Simple mode uses plain language and offers only options that cannot harm Windows or slow the PC; Advanced mode shows every tool and technical detail.
- **Safety options:** backups before removal, confirmations, restore points, and Recycle Bin copies (always on in Simple mode).
- **Monitoring scope:** which registry keys and folders snapshots cover, with exclusions and background-noise learning.
- **Explorer integration:** the **Install with Expunge** entry in the right-click menu of installers.
- **Full keyboard access:** a visible focus ring, arrow-key navigation in lists, Esc to close dialogs, and shortcuts for every page (Ctrl+1 to Ctrl+9, Ctrl+Tab, Ctrl+F, F5, Delete, and F1).
- **Screen-reader support** for Narrator and NVDA, adjustable text size, and a high-contrast option.
- **Eleven languages:** English, Español, Deutsch, Français, Português (Brasil), Italiano, Polski, Русский, 简体中文, 日本語, and 한국어. Additional languages can be added without recompiling (see [Translating](docs/TRANSLATING.md)).

### Design and distribution

- **Built for Windows 11:** a dark interface with a native red Windows 11 window border, and an administrator check at every launch with a one-click relaunch as administrator.
- **No dependencies:** a single native executable, with no .NET, no Visual C++ runtime, and nothing else to download.
- **Installer or portable**, for x64 and Arm64 PCs.
- **Full command line** for scripting and automation.

## Download

Download the latest version from the [Releases page](https://github.com/SeanShaffer88/Expunge/releases/latest).

| File | Description |
|---|---|
| `Expunge-Setup-x.y.z.exe` | Installer: installs to Program Files and adds a Start menu shortcut and an *Installed apps* entry. |
| `Expunge-x.y.z-portable.zip` | Portable: unzip anywhere; all data is stored next to the executable. |
| `Expunge-x.y.z-arm64-portable.zip` | Portable build for Windows 11 on Arm PCs. |

**System requirements:** 64-bit Windows 11 (build 22000 or later), on x64 or Arm64. Expunge is designed exclusively for Windows 11 and will not start on earlier versions of Windows. Administrator rights are required for full functionality; Expunge offers to relaunch itself with elevated rights.

## Getting started

1. Select **Monitor → Choose installer…** and pick a setup file. Wait for the "before" snapshot to finish.
2. Install the program as usual. Optionally, run it once so that its first-run settings are captured as well.
3. Click **Finish monitoring**, review the changes, and click **Save install log**.
4. When you want to remove the program, go to **Programs** (or **Install logs**) and click **Uninstall**. Review the plan, then confirm. If you change your mind, use **Backups → Restore**.

## Command line

```
expunge monitor --name "Photo Editor" -- C:\Downloads\setup.exe
expunge logs [--group G] [--tag T] [--json]
expunge revert <log-id> --dry-run
expunge revert <log-id>
expunge export <log-id> reg undo.reg
expunge backups
expunge restore <backup-id>
expunge learn-noise [--seconds N] [--apply]
expunge orphans
expunge disk C:\Users --top 20
expunge duplicates %USERPROFILE%\Pictures --min-size 1
expunge uninstall "Photo Editor" "Old Toolbar" --silent
expunge clean --dry-run
expunge schedule-clean weekly
expunge startup disable Spotify
expunge reputation C:\Downloads\setup.exe
expunge updates
expunge update --all
expunge transfer export moved.expunge "Photo Editor"
expunge transfer import moved.expunge
expunge repackage <log-id> C:\Packages --msix
expunge --help
```

Installer quality checks in a build pipeline:

```
expunge monitor --name MyApp --no-prompt --json -- dist\MyApp-Setup.exe /S
expunge lint <log-id> --strict          # exits with code 2 on errors or warnings
expunge test-uninstall <log-id> --quiet # exits with code 2 if anything was left behind
```

## Building from source

```
git clone https://github.com/SeanShaffer88/Expunge
cd Expunge
cargo build --release
```

Expunge builds with stable Rust on Windows. If Visual Studio is not installed, push to GitHub and let the included Actions workflow build it, or cross-compile from Linux (see `scripts/cross-env.sh`). Releases are built automatically when a `v*` tag is pushed.

The core engine (`src/`, excluding `gui/`) is fully unit-tested against an in-memory registry: `cargo test --lib`. Real installers are also installed, removed, and checked on clean Windows runners by the Integration workflow.

## How it works

| Stage | What happens |
|---|---|
| Snapshot | Registry keys and values (raw data of every type) and file metadata are recorded under configurable locations, skipping known Windows noise. |
| Comparison | Added, removed, and changed keys, values, and files are identified, compared case-insensitively as Windows does. |
| Plan | Services and tasks are handled first, then files, then folders (deepest first), then registry deletions, and finally restorations. |
| Execution | Every step is backed up first; protected locations are refused; locked files are scheduled for deletion at restart. |
| Undo | Backups are replayed in reverse order to restore everything. |

## Why you can feel safe using Expunge

A lot of work has gone into making Expunge careful, thorough, and fast. Here is what has been done.

**Every change can be checked and undone**
- Everything Expunge removes or changes is backed up first, and any operation can be restored with one click on the Backups page.
- Registry backups are also saved as standard `.reg` files, so they can be restored by double-clicking, even without Expunge.
- A System Restore point can be created before uninstalling, and one is always created before Anti-Telemetry changes (if System Protection is off, Expunge tells you and lets you decide).
- Anti-Telemetry records the previous value of every setting it touches, so each switch can be reversed exactly.
- Startup changes work the way Task Manager's do (the entry stays and can be switched back on), and delayed starts can be undone with one click.
- Duplicate files and other removed files can go to the Recycle Bin, and Windows asks before permanently deleting anything too large for it.
- You see a full plan before anything is removed, with every item labeled *Safe*, *Review*, or *Careful*, and you confirm it first. The command line offers `--dry-run` to preview changes.

**Built-in protection**
- Critical Windows folders and registry keys are hard-coded as protected and can never be deleted, whatever an install log or scan says.
- Registry entries that belong to Windows itself (its security components, app packages, and Task Scheduler bookkeeping) are recognized and left alone, even if Windows changed them while an installation was being monitored.
- Simple mode hides every option that could harm Windows or slow the PC, keeps backups and confirmations switched on, and offers only the safest privacy settings.
- Startup entries that Windows, security software, or hardware need can't be switched off in Simple mode.
- The duplicate finder always keeps at least one copy of every file, checks each file again right before removing it, and never searches Windows, program, or app-data folders.
- The junk cleaner only touches well-known temporary and cache locations, never their parent folders, and skips files that are in use.
- Expunge checks at every launch that it is running on Windows 11, and tells you when it needs administrator rights instead of failing silently.

**Private by design**
- Expunge has no telemetry, no accounts, and no ads, and it never sends information about you or your PC anywhere.
- It only goes online when you use a feature that needs to: winget updates, and the optional reputation check, which is off by default and sends only a file's fingerprint (hash), never the file itself.
- It is open source under the GPLv3, so anyone can read exactly what it does.

**Tested at every step**
- Around 90 automated tests run on every change, on both Windows and Linux, together with static code analysis (Clippy).
- Integration tests install real installers (Windows Installer, NSIS, and Inno Setup) on clean Windows machines, remove them with Expunge, and verify that nothing is left behind. These tests have already found and fixed real problems before release.
- Performance is measured automatically: snapshot speed and memory, time to open the window, and CPU and memory use while idle, each against a fixed budget; a check fails if any budget is exceeded.
- The Arm64 version is built and checked on every change, and translations are checked automatically for missing placeholders.
- Releases are built by GitHub Actions directly from the public source code, and every download is published with a SHA-256 checksum so you can verify it.

**Fast and light**
- Expunge is a single native program written in Rust, with no runtime to install and no just-in-time compilation, built with full link-time optimization.
- Heavy work (snapshots, scans, signature checks, measurements) runs in the background, so the window never freezes, and folders are scanned in parallel.
- Results that are expensive to compute are cached, and the window is only redrawn when something changes, so Expunge uses no CPU while it sits idle.
- Background monitoring, when you turn it on, runs at low priority so it never competes with your work.

**Made for everyone**
- Plain-language help on every page, full keyboard access, screen-reader support for Narrator and NVDA, adjustable text size, high contrast, and eleven languages.

## Use with care

Expunge has been extensively tested, both with automated tests and with real installers, but like any software that changes Windows it may still contain bugs. Many safety measures are built in: Simple mode hides anything that could harm Windows or slow the PC, critical folders and registry keys are protected and can never be deleted, every removal item is labeled by risk, and you review and confirm changes before they happen. Even so, please use Expunge with caution, and stay in Simple mode unless you know what you are doing.

If something does go wrong, almost anything Expunge does can be undone. Every file, folder, and registry entry it removes or changes is backed up first and can be put back with one click on the **Backups** page (registry entries are also saved as `.reg` files you can restore without Expunge); Anti-Telemetry settings record every previous value and can be reversed exactly; startup changes and delayed starts are reversible; duplicate files go to the Recycle Bin; and you can create a System Restore point before uninstalling or changing settings. The few actions that can't be undone, such as deleting temporary files in the junk cleaner or the deep cleanup of Windows itself, are clearly marked and confirmed separately before they run.

## Roadmap

See [ROADMAP.md](ROADMAP.md). Ideas and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Copyright © 2026 Sean Shaffer.

Expunge is free software: you can redistribute it and/or modify it under the terms of the **GNU General Public License, version 3** (or, at your option, any later version). See [LICENSE](LICENSE) for details.
