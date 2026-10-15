# Roadmap

Planned for future releases, roughly in priority order. Vote or comment on the
matching GitHub issue to move something up. Features that already exist are
listed in the [README](README.md#features).

## Next

Everything planned as *Next* has been built; the items below are the next candidates.

## Later

**Monitoring**
- **Per-value live registry view** — the live activity panel shows which registry areas change; listing each value as it changes would need event tracing (ETW).

**Moving programs**
- **MSIX registry support** — include the installation's registry entries in MSIX packages (Registry.dat).

**Uninstaller**

**Startup**

**Disk**

**Everything else**
- **More integration tests** — add WiX bundles and Squirrel installers, and run on Windows 11 client images when GitHub offers them.
- More languages — see [docs/TRANSLATING.md](docs/TRANSLATING.md).

## Considered and not planned
- **Kernel minifilter driver for monitoring** — a driver must be signed by Microsoft, which open-source projects can't easily get, and a driver bug can crash Windows. Snapshots plus event tracing (above) capture the same changes safely.
- **Rewrite in C#/.NET with WinUI 3** — it would add a runtime dependency and give up the single self-contained exe. The Rust engine is already native and fast.
- **SQLite index** — the program list is already filtered instantly in memory; the on-disk cache planned above covers the slow parts (sizes, icons, signatures) without a database.

## Known limitations
- Contents of files the installer *modified* or *deleted* can't be restored (only their existence is recorded); Expunge lists them so you know.
- A restored scheduled task file is not re-registered with Task Scheduler until it is re-imported.
- Snapshots cover the current user's profile; changes to other users' profiles aren't captured.
