# Roadmap

Planned for future releases, roughly in priority order. Vote or comment on the
matching GitHub issue to move something up.

## Next
- **Full keyboard accessibility** — every page, table, dialog and wizard usable without a mouse: logical Tab order, visible focus ring, arrow-key navigation in lists, Enter/Space/Esc everywhere, mnemonic shortcuts, and screen-reader names for every control (via egui's AccessKit support, which exposes the UI to Narrator and NVDA).
- **Lightweight footprint and a never-blocking UI** — audit that every disk, registry, network and process operation runs off the UI thread (it already does for scans, snapshots, uninstalls and icon loading) and stream large results instead of loading them whole. Add memory and CPU budgets checked in CI: idle RAM, startup time, and snapshot throughput. Release cached icons, snapshots and scan results when their page is closed.
- **Windows Slimmer–style cleanup** in the Junk cleaner, covering what [Auslogics Windows Slimmer](https://www.auslogics.com/en/software/windows-slimmer/) trims. Each item is explained and confirmed, with the space it frees shown first:
  - *Unneeded system files*: Windows.old and previous-upgrade leftovers, Windows logs, error reports and memory dumps, Delivery Optimization cache.
  - *Windows Update files*: downloaded update packages and update cache.
  - *Outdated Windows libraries*: superseded component-store (WinSxS) versions, removed with Windows' own DISM component cleanup.
  - *Disabled system components*: remove the stored files of optional Windows features that are turned off.
  - *Old System Restore points*: delete all but the newest.
  - *Unneeded apps*: suggest pre-installed Store apps that can be removed (already removable from the Programs page).
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
