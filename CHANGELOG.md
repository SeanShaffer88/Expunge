# Changelog

All notable changes to Expunge are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-14

First public release.

### Added
- Install monitoring: before/after snapshots of the registry and file system, run an installer under monitoring, or monitor manually; sessions survive restarts.
- Review captured changes, remove noise, and save them as an install log with auto-detected name, publisher and version.
- Precise uninstall from an install log: removes added files, folders, keys and values; restores changed and removed values; stops and deletes services; removes scheduled tasks; repairs `PATH` without touching later edits.
- Program list with 64-bit, 32-bit, per-user and Microsoft Store programs, sorting, search, details, real folder sizes, and a broken-entry finder.
- Uninstall wizard with vendor uninstaller, quiet uninstall, forced removal, batch uninstall, and leftover scanning.
- Backups of everything removed or changed, with one-click restore.
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

[Unreleased]: https://github.com/SeanShaffer88/ExpungeUninstaller/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/SeanShaffer88/ExpungeUninstaller/releases/tag/v0.1.0
