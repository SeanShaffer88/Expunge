# Roadmap

Planned for future releases, roughly in priority order. Vote or comment on the
matching GitHub issue to move something up.

## Next
- **Smarter automatic exclusions** — on top of today's built-in list, ignore volatile files by pattern (`*.tmp`, `*.log`, `*.etl`, `*\Cache\*`, `*\Code Cache\*`, `*\GPUCache\*`, Prefetch, event logs) for every browser and profile, and learn background noise automatically: two quick snapshots with nothing installed in between reveal what Windows changes on its own, and those paths are suggested as exclusions.
- **Full keyboard accessibility** — every page, table, dialog and wizard usable without a mouse: logical Tab order, visible focus ring, arrow-key navigation in lists, Enter/Space/Esc everywhere, mnemonic shortcuts, and screen-reader names for every control (via egui's AccessKit support, which exposes the UI to Narrator and NVDA).
- **Lightweight footprint and a never-blocking UI** — audit that every disk, registry, network and process operation runs off the UI thread (it already does for scans, snapshots, uninstalls and icon loading) and stream large results instead of loading them whole. Add memory and CPU budgets checked in CI: idle RAM, startup time, and snapshot throughput. Release cached icons, snapshots and scan results when their page is closed.
- **Windows Slimmer–style cleanup** in the Junk cleaner, in two groups:
  - *Regular*: Windows and app logs, Delivery Optimization cache, Windows Error Reporting, memory dumps, old Defender scan history, font and icon caches, Store app caches, Firefox/Brave/Opera/Vivaldi caches, Prefetch, DirectX shader cache, browser crash reports.
  - *One-time (advanced, each explained and confirmed)*: Windows.old and previous-upgrade files, superseded Windows Update components (DISM component cleanup), old System Restore points (keep the newest), the hibernation file, Reserved Storage, Compact OS (compress Windows files), removing unused built-in Store apps, and unused optional Windows features.
  - Also: large-file and duplicate-file finders and a broken-shortcut fixer.
- **Transfer programs to another PC** — pick installed or monitored programs (or a bundle), and Expunge packs their files, registry settings, services, shortcuts and file associations into one portable `.expunge` package. On the new PC, Expunge restores it, rewrites user-profile paths (different user names or drive letters), registers services and shortcuts, and keeps an undo log so a failed transfer can be rolled back in one click. A compatibility check runs first (Windows version, architecture, drivers, licensing or activation that is machine-bound), and programs known not to move (Office, drivers, anti-virus) are flagged.
- **winget integration** — show which installed programs have updates and upgrade them in place.

## Later
- **Background auto-monitoring** — a lightweight service that notices installations and records them automatically, even ones you forgot to monitor.
- **"Hunter" mode** — drag a crosshair onto a window, tray icon or shortcut to identify and remove its program.
- **Scheduled junk cleaning.**
- **Groups and tags** for install logs.
- **Repackage** a monitored installation as a portable folder or MSIX.
- **Arm64 builds.**
- More languages — see [docs/TRANSLATING.md](docs/TRANSLATING.md).

## Known limitations
- Contents of files the installer *modified* or *deleted* can't be restored (only their existence is recorded); Expunge lists them so you know.
- A restored scheduled task file is not re-registered with Task Scheduler until it is re-imported.
- Snapshots cover the current user's profile; changes to other users' profiles aren't captured.
