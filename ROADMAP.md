# Roadmap

Planned for future releases, roughly in priority order. Vote or comment on the
matching GitHub issue to move something up. Features that already exist are
listed in the [README](README.md#features).

## Next

Everything planned as *Next* has been built; the items below are the next candidates.

## Later

**Monitoring**
- **Live activity inspector** — watch an installer's file and registry changes stream in as they happen, using Windows' built-in event tracing (ETW), which needs no driver.
- **Background auto-monitoring** — notices installations and records them automatically, even ones you forgot to monitor.

**Moving programs**
- **MSIX output** for repackaged installations (needs Microsoft's free MSIX Packaging Tool to sign the result).

**Uninstaller**

**Startup**
- **Reputation check** (opt-in, off by default) — look up unknown startup items and installers by file hash in public malware databases. Only hashes are sent, never files.

**Disk**

**Everything else**
- **System health dashboard** — disk space trend, memory use, and services that stopped unexpectedly.
- **Integration tests on clean Windows 11 virtual machines** — real NSIS, Inno Setup, MSI and WiX installers installed, monitored, uninstalled and verified clean on every release.
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
