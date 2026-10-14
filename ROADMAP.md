# Roadmap

Planned for future releases, roughly in priority order. Vote or comment on the
matching GitHub issue to move something up. Features that already exist are
listed in the [README](README.md#features).

## Next

**Moving programs**
- **Transfer programs to another PC** — pick installed or monitored programs (or a bundle), and Expunge packs their files, registry settings, services, shortcuts and file associations into one portable `.expunge` package. On the new PC, Expunge restores it, rewrites user-profile paths (different user names or drive letters), registers services and shortcuts, and keeps an undo log so a failed transfer can be rolled back in one click. A compatibility check runs first (Windows version, architecture, drivers, licensing or activation that is machine-bound), and programs known not to move (Office, drivers, anti-virus) are flagged.
- **Repackage** a monitored installation as a portable folder, a silent install script, or an MSIX, for deploying to many machines.

## Later

**Monitoring**
- **Live activity inspector** — watch an installer's file and registry changes stream in as they happen, using Windows' built-in event tracing (ETW), which needs no driver.
- **Background auto-monitoring** — notices installations and records them automatically, even ones you forgot to monitor.

**Uninstaller**
- **Pre- and post-uninstall scripts** — run a PowerShell or batch script before or after removing a specific program (for example, to export its settings or stop a service first). Advanced mode only.
- **"Hunter" mode** — drag a crosshair onto a window, tray icon or shortcut to identify and remove its program.

**Startup**
- **Startup impact** — how much each startup item delays boot, measured from Windows' own boot-performance diagnostics.
- **Delayed start** — launch non-essential startup programs a set time after sign-in to shorten boot.
- **Reputation check** (opt-in, off by default) — look up unknown startup items and installers by file hash in public malware databases. Only hashes are sent, never files.

**Disk**
- **Disk space map** — treemap of where space goes, to find large folders and forgotten files.
- **Duplicate file finder** — matches by size first, then by SHA-256 hash; never touches Windows or program folders.
- **Browser privacy cleaning** (opt-in per item) — cookies, history, site data and sessions for every Chromium and Firefox-based browser and profile, plus font caches.
- **Scheduled junk cleaning.**

**Everything else**
- **System health dashboard** — disk space trend, memory use, and services that stopped unexpectedly.
- **Complete command line** — every feature scriptable for unattended maintenance: classic uninstall by name, batch queue, junk cleaning, startup management.
- **Integration tests on clean Windows 11 virtual machines** — real NSIS, Inno Setup, MSI and WiX installers installed, monitored, uninstalled and verified clean on every release.
- **Groups and tags** for install logs.
- **Arm64 builds.**
- More languages — see [docs/TRANSLATING.md](docs/TRANSLATING.md).

## Considered and not planned
- **Kernel minifilter driver for monitoring** — a driver must be signed by Microsoft, which open-source projects can't easily get, and a driver bug can crash Windows. Snapshots plus event tracing (above) capture the same changes safely.
- **Rewrite in C#/.NET with WinUI 3** — it would add a runtime dependency and give up the single self-contained exe. The Rust engine is already native and fast.
- **SQLite index** — the program list is already filtered instantly in memory; the on-disk cache planned above covers the slow parts (sizes, icons, signatures) without a database.

## Known limitations
- Contents of files the installer *modified* or *deleted* can't be restored (only their existence is recorded); Expunge lists them so you know.
- A restored scheduled task file is not re-registered with Task Scheduler until it is re-imported.
- Snapshots cover the current user's profile; changes to other users' profiles aren't captured.
