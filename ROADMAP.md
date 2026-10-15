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
- **Code signing certificate** — the release workflow already signs `Expunge.exe` (x64 and Arm64) and the installer, with a timestamp, as soon as the `CODESIGN_PFX` and `CODESIGN_PASSWORD` repository secrets are set. What's left is getting a certificate: a free open-source one (SignPath Foundation), Azure Artifact Signing, or an OV certificate.
- **Antivirus false-positive reports** — the release workflow already uploads every download to VirusTotal when the `VIRUSTOTAL_API_KEY` secret is set, warns about any detection and links each report in the release notes. What's left is a free VirusTotal account for the key, and submitting any false positives to Microsoft Defender, Malwarebytes and other vendors before publishing.
- **Files held open by services** — the stress tests cover files another program holds open; also cover running services and COM objects Explorer still has loaded.
- **Clean-VM validation matrix** — run the full monitor, uninstall and check-clean cycle on a freshly provisioned, snapshot-isolated Windows 11 virtual machine (the CI runners are Windows Server), with complex installers: large MSI suites, Inno Setup, NSIS, Wise, InstallShield, WiX bundles and Squirrel.
- **Long-running soak test** — run background monitoring and live activity for days under a profiler to confirm memory, handle and thread counts stay flat and no files stay locked.

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
