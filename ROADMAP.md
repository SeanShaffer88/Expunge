# Roadmap

Planned for future releases, roughly in priority order. Vote or comment on the
matching GitHub issue to move something up.

## Next
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
