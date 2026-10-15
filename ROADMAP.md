# Roadmap

Planned for future releases, roughly in priority order. Vote or comment on the
matching GitHub issue to move something up. Features that already exist are
listed in the [README](README.md#features).

## Next

Every planned feature has been built. The items below are ideas for after version 1.0.

## Later

**Monitoring**
- **Per-value live registry view** — the live activity panel shows which registry areas change; listing each value as it changes would need event tracing (ETW).

**Portable App Builder**
- **Drive-letter and path fixing** — find absolute paths inside captured settings files and write matching `[FileWrite]` sections, so settings follow the drive.
- **A sandbox for Windows 11 Home** — Windows Sandbox needs Pro, Enterprise or Education; a Hyper-V or other virtual machine option would keep Home PCs untouched too.

**Release readiness**
- **Code signing** — sign `Expunge.exe`, the installer and the portable zip for both x64 and Arm64 in the release workflow, through a free open-source signing service (SignPath Foundation) or Azure Artifact Signing, so Windows SmartScreen and antivirus software trust the downloads.
- **SmartScreen reputation** — until signed builds have download history, Windows may warn on first run; the download section and release notes should say so and show how to check the signature.
- **Antivirus false-positive pre-screening** — uninstallers and monitors read system files and stop processes, which heuristic scanners sometimes flag. Upload each release candidate to VirusTotal from the release workflow and submit any false positives to Microsoft Defender, Malwarebytes and other vendors before publishing.
- **Clean-VM validation matrix** — run the full monitor, uninstall and check-clean cycle on a freshly provisioned, snapshot-isolated Windows 11 virtual machine (the CI runners are Windows Server), with complex installers: large MSI suites, Inno Setup, NSIS, Wise, InstallShield, WiX bundles and Squirrel.
- **Edge-case stress tests** — locked and in-use files, running services, COM objects still referenced by Explorer, very deep registry trees and long paths, checking that removal degrades gracefully (scheduled for the next restart, reported, never half-done).
- **Crash-safe removal journal** — write each step of a removal to a journal before doing it, so a crash or power loss mid-cleanup can be finished or rolled back from the backup on the next start.
- **Protected-location tests** — a test suite that tries recursive deletion of every critical root (`C:\Windows`, `Program Files`, the profile, drive roots, `HKLM\SOFTWARE`, `HKLM\SYSTEM` and other hive roots) through every removal path and proves each one is refused.
- **Native API audit** — review every `unsafe` Windows API call for buffer sizes, error handling and handle cleanup (closing handles with RAII guards on every path), with Clippy's pedantic lints in CI.
- **Long-running soak test** — run background monitoring and live activity for days under a profiler to confirm memory, handle and thread counts stay flat and no files stay locked.
- **Startup and size budget** — keep cold start under a set time and the exe under a set size, enforced by the existing performance check in CI.

**Everything else**
- **More integration tests** — add WiX bundles and Squirrel installers, and run on Windows 11 client images when GitHub offers them.
- More languages and native-speaker reviews of the existing ones — see [docs/TRANSLATING.md](docs/TRANSLATING.md).

## Considered and not planned
- **Kernel minifilter driver for monitoring** — a driver must be signed by Microsoft, which open-source projects can't easily get, and a driver bug can crash Windows. Snapshots plus Windows' change notifications capture the same changes safely.
- **Rewrite in C#/.NET with WinUI 3** — it would add a runtime dependency and give up the single self-contained exe. The Rust engine is already native and fast.
- **Always require administrator rights in the manifest** — Expunge starts as the current user and offers a one-click relaunch as administrator, because much of it (browsing programs, junk cleaning in your profile, the Ask, Snapshots and Right-click menu pages for your account) needs no elevation, and the command line must run in unelevated scripts. Actions that need administrator rights say so and relaunch elevated.
- **Self-contained .NET deployment, trimming and ReadyToRun** — not applicable: Expunge is a native Rust program with no runtime to bundle, already built as separate x64 and Arm64 executables with link-time optimization.
- **SQLite index** — the program list is already filtered instantly in memory; the on-disk cache planned above covers the slow parts (sizes, icons, signatures) without a database.

## Known limitations
- Contents of files the installer *modified* or *deleted* can't be restored (only their existence is recorded); Expunge lists them so you know.
- A restored scheduled task file is not re-registered with Task Scheduler until it is re-imported.
- Snapshots cover the current user's profile; changes to other users' profiles aren't captured.
