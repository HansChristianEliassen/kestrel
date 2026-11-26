# Kestrel roadmap: what to add and improve next (research, November 2026)

Done so far:
- 0.14 "Health and space": drive health, Windows.old and update cleanup, pause Windows Update, About this PC.
- 0.15 "Control": privacy settings, Get apps, skipping app updates.
- 0.16 "Proof": Measure a game, Play, Disk usage.
- 0.17 "Set up this PC for gaming": one-click setup, startup apps, Game Mode for your games, memory integrity opt-in.
- 0.18 "Find the bottleneck": bottleneck in Measure, setup-mistake checks, NVIDIA shader cache.
- 0.19 "Game settings": presets for Unreal Engine games and RDR2 (verified values only), guides for Overwatch and Call of Duty.
- 0.20 "Simpler layout": five sections (Home, Games, Windows, Clean up, Apps) plus Settings; one page per game; Services.
- 0.21 "Know more": guides for Valorant, Apex Legends and Minecraft, hard drive warning, update counts on Home, Copy diagnostics, opt-in Start with Windows, memory in use on Startup apps.
- 0.22 "Clean test": a temporary, reviewed clean boot to rule out other software, measured with Measure.
- 0.23 "Profiles and System analysis": Balanced / Competitive / Extreme profiles across settings, services and tasks; System analysis with a baseline; detection of other optimizers; service grouping; more diagnostic services.
- 0.24 "Resizable BAR and MPO": a Resizable BAR check for supported cards and NVIDIA's MPO fix as an opt-in setting.
- 0.25 "New layout": a flat HD OPTI-style sidebar with GPU, Network, Controller & input and Tools pages, a sidebar Game Mode switch, a status bar, and a network test (ping, jitter, loss to the router and the internet).
- 0.26 "NVIDIA per-game settings": Prefer maximum performance and Low latency mode per game, in the game's NVIDIA driver profile.
- 0.27 "Driver Optimizer": NVIDIA driver without the App, its services, overlay, recorder and telemetry, from NVIDIA's lookup or a file, read from NVIDIA's own manifests, installed silently and verified.
- 0.28.1 "Safer driver installs": a visible install window, Kestrel closing itself first, and recovery from interrupted installs.
- 0.28 "One NVIDIA gaming profile": one global driver profile instead of per-game settings, the driver's own telemetry blocked in the hosts file with a live check, and rollback to the previous stripped driver.

Next releases:
- 0.28 "Input test": mouse polling rate (Raw Input events per second while moving the mouse, and how steady they are) and controller packet rate (XInput) on Controller & input. Read-only.
- 0.29 "More games": game-settings presets for Counter-Strike 2 and Cyberpunk 2077 (one calibration run each), RDR2 DLSS values once verified.
- 0.30 "AMD": the same per-game switches for AMD cards through ADLX (Anti-Lag, Radeon Chill off) and AMD's shader cache.
- Driver Optimizer follow-ups: laptop (notebook) packages tested on a laptop, Windows 10's older tar (falls back to 7-Zip when installed), and whether the NVIDIA Display Container can start on demand while the classic Control Panel keeps working (needs an administrator test).
- Research first: per-game server ping on the Network page; a DPC latency check in Tools.
- Skipped from HD OPTI: controller overclocking (hidusbf), forced MSI mode, NIC tuning, USB port check.

## Custom-OS parity plan (from the KernelOS/xOS teardown, November 2026)
Goal: reach what KernelOS, xOS and AtlasOS do on an official install, keeping only changes with a technical justification, each detected, reversible and honest about its cost.

**Audit: Kestrel against the custom builds**
| Area | Custom builds | Kestrel today | Verdict |
|---|---|---|---|
| Telemetry, location, maps, phone, retail demo, Insider, wallet, fax, link tracking, hotspot, printing | xOS, Atlas, Revi disable these | Services tab (safe list) | Done |
| Diagnostic hosts: `diagnosticshub.standardcollector.service`, `PcaSvc`, `WdiServiceHost`, `WdiSystemHost`, `DPS`, `wercplsupport`, `InventorySvc` | xOS, Revi (Atlas: `PcaSvc`) | Missing | Add to Services (0.23) |
| Service grouping (`SvcHostSplitThresholdInKB` above RAM / `SvcHostSplitDisable`) | Atlas, xOS | Missing | Add as an opt-in tweak, labeled as cosmetic: fewer processes, same work (0.23) |
| Before/after measurement of processes, threads, handles, svchost, RAM, CPU, services, tasks, security | None publish this | Missing | Add System analysis (0.23) |
| Detecting AtlasOS/xOS/ReviOS/KernelOS and manual changes | n/a | Missing | Add (0.23): Atlas `C:\Windows\AtlasDesktop`, xOS `HKLM\SOFTWARE\XOSToolkit`, plus state signals |
| Game Mode, Game DVR, GPU scheduling, Delivery Optimization, power mode, USB suspend, NVIDIA shader cache, startup apps | All | Done | Keep |
| Telemetry scheduled tasks | Atlas (5 tasks), xOS (`DisableScheduledTasks.ps1`) | Missing | 0.24, after reading xOS's script and checking each task exists on 24H2/25H2 |
| Profiles Balanced / Competitive / Extreme | Atlas, xOS options | Presets for Windows settings only | 0.24: one profile choice spanning settings, services and tasks, hardware-aware |
| Removing inbox apps, Copilot/Recall | KernelOS, xOS | Set up removes some preinstalled apps | 0.25 (Extreme): Store apps are reinstallable; Recall through its Windows feature |
| Defender, Windows Update, mitigations, VBS off | KernelOS, xOS | VBS opt-in only | 0.25 (Extreme), behind explicit confirmation with the anti-cheat consequences; needs your decision first |
| `Win32PrioritySeparation` 0x26 | Atlas, xOS | — | Skip: decodes to client Windows' default (short, variable, 3x boost) |
| `NetworkThrottlingIndex` 10, `SvcHostSplitThresholdInKB` 3670016 | Revi, xOS | — | Skip: both equal the Windows default |
| `SystemResponsiveness`, `DisablePagingExecutive`, `DisablePageCombining`, `QueuedPresentLimit` | xOS, Atlas | — | Skip: KernelOS reverted all four in v1.5.1 |
| Global timer resolution, `useplatformclock`, HPET device off, forced MSI, TCP tweaks | Various | — | Skip: per-process since Windows 10 2004 (Microsoft), or no evidence, or risk |
| Health check text "Kestrel never turns security features off" | — | Contradicts the memory integrity opt-in | Fix (0.23) |

**Stages**
All three stages shipped together in 0.23.0. Still open: inspecting KernelOS's unpublished timer and power-plan values in a VM, and per-game NVIDIA/AMD driver profiles.

Next:
- More games, each through documented sources or one calibration (Counter-Strike 2 and Cyberpunk 2077 need one calibration run each); RDR2 DLSS and Overwatch file values once verified.
- NVIDIA "Prefer maximum performance" per game (small measured effect; needs per-game driver profiles) and AMD driver settings (ADLX).
- From 0.15: warn when an app being updated is open; update counts on the Overview.

## Context
You asked which features to add or improve to make Kestrel a true all-in-one tool for Windows gaming, optimizing and management. I compared Kestrel 0.13 against the tools people combine today:
- **Maintenance:** Microsoft PC Manager, CCleaner, Advanced SystemCare.
- **Tweaks:** WinUtil (Chris Titus), O&O ShutUp10++ (privacy).
- **Apps:** UniGetUI (winget front end).
- **Gaming:** NVIDIA App, Razer Cortex, Process Lasso, Playnite (game library), Intel PresentMon (frame-time measurement).
- **Disks:** WizTree (disk usage), CrystalDiskInfo (drive health).

COMPETITORS.md (0.11) already covered the boosters. Most of the old ROADMAP.md is done (safe delete, per-game hub, GPU/overlay/storage checks, native Store API, bulk uninstall, updates).

**Where Kestrel stands:** the honest-optimizer part is strong and rare. A user still needs other tools for:
- disk health and what fills the disk;
- privacy settings;
- installing apps;
- pausing Windows Update before a match;
- proving whether a change helped.

Those gaps are what make other apps "all-in-one".

The guiding rules stay:
- every change is opt-in, previewed and reversible;
- nothing runs in the background unless you turn on Start with Windows;
- no FPS promises;
- Windows' own documented mechanisms only.

## New features (ranked)

| # | Feature | What it does | Fits Kestrel because | Effort |
|---|---|---|---|---|
| 1 | **Drive health** (Status check + Gaming health) | Per drive: health, temperature, SSD wear ("percentage used"), media errors, power-on hours. | Read-only, and fixes a real cause of stutter and data loss (CrystalDiskInfo is the usual separate install). Windows exposes it natively: the NVMe health log through `IOCTL_STORAGE_QUERY_PROPERTY` (protocol-specific) and `IOCTL_STORAGE_PREDICT_FAILURE` for SATA. No admin for most drives. | M |
| 2 | **Deep cleanup: Windows.old + component store** | Adds a "Previous Windows installation" category (often 10–30 GB after a feature update) and "Windows component store cleanup" through `DISM /StartComponentCleanup`, Microsoft's supported method. | The biggest space wins after a feature update. Both are supported methods (Disk Cleanup does the same). Admin, preview and confirm. | M |
| 3 | **Privacy settings** (new Gaming-page tab or a Settings-style page) | About 10 documented, reversible toggles: advertising ID, tailored experiences, diagnostic data to Required, activity history, Start/lock-screen suggestions, Bing results in Start search, Copilot, Recall snapshots. | ShutUp10++-style control is expected in an all-in-one tool. It reuses the existing Tweak + backup + Revert system exactly, and is labeled as privacy, never as performance. | M |
| 4 | **Measure a game (before/after)** | Click Measure, play 60 s, and get average FPS, 1% lows and frame-time stability. Compare runs before and after a change. | The honest answer to boosters' FPS claims: users see real numbers from their own PC. Built on Intel PresentMon (MIT licensed, the industry standard); Kestrel uses it as a tool, not its own capture. Open choice: install PresentMon via winget on first use (recommended, keeps one exe), or bundle it. | L |
| 5 | **App installer ("Get apps")** | Search the winget catalog and install. A curated "New PC" list (Steam, Discord, Epic, 7-Zip, browsers…) with tick-and-install-all. | WinUtil's most-used tab. Reuses 0.13's winget queue, confirmation and id validation. | M |
| 6 | **Pause Windows Update** | "Pause updates for 1 / 2 / 5 weeks" and "Resume", using the same values Settings > Windows Update writes, so the user can avoid a surprise restart or download before a tournament. Shows the active hours. | Documented and reversible, no service changes. Unlike WinUtil, Kestrel never disables or blocks updates. | S |
| 7 | **Play from Kestrel** | A Play button on each detected game (Gaming > Games): it turns on Game Mode with that game's profile, then starts the game through its launcher URI (`steam://rungameid/…`, Epic, etc.). | Kestrel already detects games and profiles. Playnite-style launching without a resident watcher. | S–M |
| 8 | **Disk usage map** (Cleanup > Large files) | "What's using space": the largest folders per drive (including game folders), drill-down, Show in folder. | WizTree-style insight. Read-only. A folder walk first; reading the NTFS MFT directly is the faster upgrade later. | M |
| 9 | **System summary** (Settings > About this PC, or the Overview) | CPU, GPU and driver, RAM and speed, drives, displays and refresh, Windows build, with Copy for support forums. | Kestrel reads all of this already; it just isn't shown together. Helps "can I run it" and troubleshooting. | S |

## Improvements to existing features
- **Updates page:**
  - **Skip this version / Ignore app**, as in UniGetUI.
  - When Kestrel runs as administrator, warn before updating apps that installers may refuse (the error you hit).
  - Warn when an app being updated is open ("Spotify is running and will close").
  - Show the last check's update counts on the Overview.
- **Optimize PC:** add the new findings as review-only items (drive health warnings, Windows.old size, a pending update restart).
- **Startup apps:** show each app's startup impact where Windows records it, and sort by impact.
- **Game Mode:** per-game profiles from the Play button (feature 7). Show "Game Mode on: <game>" in the tray tooltip.
- **Distribution (high value, outside the code):**
  - **Code-sign the exe** (Azure Trusted Signing, about $10/month) so SmartScreen stops warning. This is the biggest barrier to new users.
  - **Publish Kestrel to winget** (`winget install Kestrel`), a small manifest PR to winget-pkgs.

## Researched and rejected (consistent with COMPETITORS.md)
- **Third-party driver updaters and driver cleaners (DDU):** mismatched drivers, and a high risk if run wrongly.
- **Disabling Windows Update entirely, removing Edge, aggressive debloat or stopping services:** documented breakage, with security costs.
- **GPU overclocking or undervolting (Afterburner):** a hardware risk, and the vendor apps do it.
- **CPU/GPU temperature monitoring:** needs a kernel driver (WinRing0 has known vulnerabilities). GPU temperatures could come later from NVAPI/ADL, but are out of scope now.
- **Turning off memory integrity (VBS) automatically:** keep the current status and explanation only. The gain is real in CPU-bound games, but it's a security trade-off the user must make in Windows Security.
- **Process Lasso-style resident ProBalance and scheduled background maintenance:** both need a background process. Game Mode's efficiency mode covers the gaming case.

## Proposed next release (0.14): "Health and space"
Features **1 drive health**, **2 deep cleanup**, **6 pause Windows Update** and **9 system summary**:
- all read-only or using Windows' own supported methods;
- each S–M effort;
- no new dependencies;
- together they fill the biggest "still need another app" gaps.

Then:
- **0.15 "Control":** 3 privacy, 5 get apps, Updates improvements.
- **0.16 "Proof":** 4 measure a game, 7 play from Kestrel, 8 disk usage map.

Code signing and the winget listing can happen anytime.

**Files for 0.14:**
- `src/optimizer/health.rs` (new `drive_health()` check, next to `storage()`);
- `src/cleaner/scanner.rs` + `cleanup.rs` (new `Kind::WindowsOld`, `Kind::ComponentStore` using `run_hidden("dism.exe", …)`);
- `src/optimizer/tweaks.rs`, or a small settings module, for pause/resume (registry values under `HKLM\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings`, admin);
- `src/ui/mod.rs` Settings > About this PC.

**Reuse:**
- `health::gpu_drivers()`, `memory_speed`, `storage()`'s drive enumeration;
- `system::run_hidden`, `is_admin`, `create_restore_point`;
- the existing Check / Confirm / Job patterns.

## Verification (for 0.14)
- **Unit tests:** the NVMe health-log parser (fixture bytes), DISM output parsing, pause-date math.
- **Live read-only:** drive health on this PC (NVMe + any SATA), the Windows.old/component store size estimate (`/AnalyzeComponentStore`).
- **Manual, with your OK:** pause and resume Windows Update, checked in Settings > Windows Update. Component cleanup only if you agree (it is irreversible: it removes superseded Windows component versions, so older updates can no longer be uninstalled).
- **Final:** debug screenshots of every page, then fmt, clippy, tests and the release build.

## Sources
- [Microsoft PC Manager overview (Data Doctors)](https://wtop.com/tech/2026/01/data-doctors-should-i-be-using-microsoft-pc-manager), [BetaNews](https://betanews.com/article/microsoft-pc-manager-feels-like-ccleaners-little-brother)
- [WinUtil in 2026](https://christitus.com/winutil-in-2026/), [WinUtil user guide](https://winutil.christitus.com/guides/)
- [UniGetUI](https://marticliment.com/wingetui/), [AlternativeTo](https://alternativeto.net/software/wingetui/about)
- [O&O ShutUp10++ 2.2.1024](https://blog.oo-software.com/en/?p=10855), [2.0.1009](https://blog.oo-software.com/en/?p=10629)
- [PresentMon releases](https://github.com/GameTechDev/PresentMon/releases), [PresentMon 2.0 (Neowin)](https://www.neowin.net/news/intel-presentmon-gets-updated-to-version-200-brings-new-features-and-optimizations/)
- [NVIDIA App features](https://www.nvidia.com/en-us//geforce/news/nvidia-app-download-and-features), [NVIDIA overlay CPU stats (TechSpot)](https://www.techspot.com/news/113811-nvidia-app-adds-cpu-temperature-power-monitoring-overlay.html)
- [Process Lasso gaming docs](https://bitsum.com/docs/pl/gaming/)
- [Playnite](https://playnite.link), [comparison](https://api.playnite.link/docs/manual/gettingStarted/comparisonWithOtherSoftware.html)
- [WizTree](https://diskanalyzer.com/about)
- [CrystalDiskInfo / SSD health (TweakTown)](https://www.tweaktown.com/guides/11407/your-ssd-is-telling-you-its-about-to-die-heres-how-to-listen/index.html)
- [Game boosters roundup (iTechGuides)](https://www.itechguides.com/10-best-game-boosters-and-optimizers-for-windows-pc-in-2026/), [Tier1Settings tools comparison](https://tier1settings.com/best-pc-optimization-tools-for-gaming/)
- [Windows 11 gaming settings (Memento Mori Gaming)](https://mementomorigaming.com/guides/pc/windows-11-gaming-optimization), [WindowsForum tuning guide](https://windowsforum.com/news/windows-11-gaming-tuning-guide-2026-safe-consistent-performance.401517/)
