# Changelog

All notable changes to Expunge are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Install with Expunge** in the right-click menu of `.exe` and `.msi` files: installs under monitoring in one step. Added for all users by Setup, or per user from Settings (portable). Relaunching as administrator keeps the installer.
- Program icons in the program list and details.
- **Program trust info**: each program's digital signature is verified offline (Authenticode). Shows who signed it, flags unsigned programs (new "Unsigned" filter) and broken signatures, and warns before uninstalling a program whose signature fails.
- **Simple mode only offers safe options.** Hidden until you switch to Advanced mode: the Windows Updates remover, forced removal, system components, cleaner items that slow the PC while caches rebuild (shader, thumbnail, browser and Windows Update caches), deleting startup entries (broken ones excepted), and the switches that turn off backups and confirmations. Startup entries that Windows, security software or hardware need can't be disabled. Removal lists leave out items marked Careful and uncertain matches.
- Installer lint flags unsigned or broken-signature executables (EXP030, EXP031).
- CLI: `expunge verify <file>` and `expunge programs --trust`.

### Changed
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

[Unreleased]: https://github.com/SeanShaffer88/ExpungeUninstaller/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/SeanShaffer88/ExpungeUninstaller/releases/tag/v0.1.0
