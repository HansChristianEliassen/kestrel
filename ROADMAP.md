# Kestrel roadmap: what to add and improve next (research, November 2026)

0.14 implemented "Health and space", 0.15 "Control" (privacy, Get apps, skipping app updates) 0.16 "Proof" (Measure a game, Play, Disk usage) 0.17 "Set up this PC for gaming" and 0.18 "Find the bottleneck" (bottleneck in Measure, setup-mistake checks, NVIDIA shader cache). Deferred: NVIDIA "Prefer maximum performance" per game (small measured effect; needs per-game driver profiles) and AMD driver settings (ADLX). Not yet done from 0.15: the running-app warning before an update and update counts on the Overview. The previous roadmap (most of it now shipped) is in git history.

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
- nothing runs in the background;
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
