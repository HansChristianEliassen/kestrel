# Kestrel 0.6.1 — Research & Feature Audit (plan)

## Context
An evidence-based audit of Kestrel (Rust/egui Windows cleaner + gaming optimizer) at version 0.6.1: what to add, fix, improve and remove. Findings come from reading the source, behaviour verified on the development PC, and research. **Confirmed** = seen in code or measured; **Suspected/Unverified** = marked as such.

On approval, the plan is: (1) save this report as `kestrel/AUDIT.md`, (2) implement **release 0.7 "Reliability"** (section 10), nothing beyond it without a further go-ahead.

---

## 1. Executive summary
Kestrel is in good shape for a 0.6 product: small honest tweak set, version-aware labels, handle-based safe deletion (junction attack closed), per-change backups with Restore everything, crash-safe Game Mode, native APIs instead of PowerShell on hot paths, 0 ms idle CPU, a coherent Windows-style UI. The weak spots are **restore integrity** (non-atomic backup file, Game Mode vs Restore-everything conflict), **one tweak doing more than it says** (background recording), **a few unsafe-when-elevated or never-returning paths**, and **unnecessary repeated disk work**. Biggest *new* value: laptop/battery awareness, leftover detection after uninstall, a large-files view, and clearer "always vs while playing" separation between Gaming and Game Mode.

## 2. Current feature audit
| Area | State | Notes |
|---|---|---|
| Cleaner (12 categories) | **Verified** (scan, delete, junction test) | Thumbnail cache mostly locked by Explorer (low yield); Windows Update cache deleted without stopping services (see C/H) |
| Uninstaller (desktop + Store) | **Verified** list/sizes; uninstall verified once (Git, Epic, µTorrent case) | Store removal via PackageManager not exercised live; "Stop waiting" can't interrupt a hung uninstaller |
| Startup (Run, folders, tasks) | **Verified** listing; toggles for Run/folders verified earlier | Scheduled-task toggles offered to standard users, then fail |
| Gaming tweaks (9) | Implemented; status detection verified | See optimization audit (§4) |
| Per-game (3 switches + detection of 8 launchers) | **Verified** detection (CoD, Overwatch) | Toggles run on UI thread |
| Health checks (9) | **Verified** on your PC | Storage check re-walks every game folder each run |
| Game Mode (manual/auto, profiles, recovery) | **Verified** on/off, crash restore, auto detect (charmap stand-in) | Not tested with a real anti-cheat game; conflicts with Restore everything |
| Restore everything / backups | Implemented, partly verified | Non-atomic file write, no locking |
| Optimize PC | **Verified** review dialog | Applies Performance preset regardless of laptop/battery |
| UI 0.6.1 | **Verified** screenshots, keyboard focus | No app icon (title bar shows egui "e"), no light/high-contrast theme |

Missing vs. what users expect from such tools: leftover detection after uninstall, large-file view, battery-aware behaviour, settings export/import, cleanup file preview, first-run guidance.

## 3. Critical problems (fix first)
| # | Problem | Evidence (file) | Impact | Fix |
|---|---|---|---|---|
| C1 | **Game Mode vs Restore everything**: Restore everything while Game Mode is on gets undone when Game Mode ends (its snapshot writes the pre-restore value back) | Confirmed: `app/mod.rs` `Action::RestoreAll` (≈L298) never checks `gamemode::is_active()` | Broken restoration — user believes settings are restored | Block Restore everything while Game Mode is on (or end Game Mode first, then restore) |
| C2 | **Backup file written non-atomically, no lock**: `fs::write` truncates then writes; UI thread (startup/per-game toggles) and job threads (apply/restore) load-modify-save concurrently | Confirmed: `utils/backup.rs:28`; callers on both threads | Crash/power loss mid-write wipes all saved originals; concurrent writes can drop an entry | Write temp file + `std::fs::rename` (atomic replace), guard all backup ops with one `static Mutex` |
| C3 | **"Turn off background game recording" disables all capture**: sets `AppCaptureEnabled=0` (+`GameDVR_Enabled=0`) but the text says manual recording still works | Confirmed config `optimizer/tweaks.rs:153-154`; community docs map "Record what happened" to `HistoricalCaptureEnabled` | Misleading; users lose Win+Alt+R recording | Use `HKCU\…\GameDVR\HistoricalCaptureEnabled=0` only (verify on your PC); keep old keys only in restore path for existing backups |

## 4. Optimization audit
| Tweak (where) | What it changes | Works / supported | Evidence of benefit | Admin | Reversible | Verdict |
|---|---|---|---|---|---|---|
| Enable Game Mode | HKCU `GameBar\AutoGameModeEnabled=1` | Yes, Win10/11 (on by default) | Microsoft documents Game Mode; gains modest | No | Yes | **Keep** (mostly a status check) |
| Background recording off | `GameDVR_Enabled`, `AppCaptureEnabled` | Works but too broad | Real when capture was on | No | Yes | **Replace** (C3) |
| Limit background Store apps | `GlobalUserDisabled` | Win10 only (already gated) | Small; Win10 support ended Oct 2025 | No | Yes | **Remove** (keep only restore of existing backups) |
| Transparency off | `EnableTransparency=0` | Yes | Negligible on dedicated GPUs | No | Yes | **Improve**: label "cosmetic", drop from Performance preset |
| Window animations off | `SPI_SETANIMATION` | Yes | Cosmetic | No | Yes | **Improve**: merge with transparency into one optional "Reduce visual effects" |
| Optimizations for windowed games | `DirectXUserGlobalSettings SwapEffectUpgradeEnable=1` | Win11 (gated) | Microsoft support doc + DirectX blog | No | Yes | **Keep** |
| High performance power plan | `powercfg /setactive SCHEME_MIN` | Fails on Modern Standby laptops; your plan GUID is renamed by another tool ("Paragon Core Power Plan") | Small on modern CPUs; costs battery | No | Yes | **Improve**: battery/Modern-Standby aware, detect renamed/third-party plans, exclude from presets on laptops |
| USB selective suspend off | `powercfg` AC value, active plan only | Yes | Weak (devices in use don't suspend) | No | Yes (per plan) | **Investigate** → move to Advanced, out of presets |
| Mouse acceleration off | `SPI_SETMOUSE` | Yes | Preference; raw-input games unaffected | No | Yes | **Keep** |
| Per-game high-performance GPU | `UserGpuPreferences GpuPreference=2` | Yes | Same as Settings > Graphics | No | Yes | **Keep** |
| Per-game fullscreen optimizations off | AppCompat `DISABLEDXMAXIMIZEDWINDOWEDMODE` | Still honoured per guides; 24H2 behaviour changed | Mixed; may break Auto HDR/VRR | No | Yes | **Keep + Improve** wording ("test per game; may disable Auto HDR/VRR") |
| Per-game windowed-opt opt-out | per-app `SwapEffectUpgradeEnable=0` | Win11 | Settings exposes the same | No | Yes | **Keep** |
| Refresh-rate fix | `ChangeDisplaySettingsExW` + 15 s revert | Verified on your PC (60→165 Hz) | Large, measurable | No | Yes | **Keep** (highest-value feature) |
| Game Mode: close listed apps | `WM_CLOSE` to listed apps' windows | Verified | Frees RAM on small systems | No | Not restorable (stated) | **Keep** |
| Cleaner: Windows Update cache | deletes `SoftwareDistribution\Download` | Works | Space only | Yes | No | **Improve**: only when BITS/wuauserv idle (Microsoft procedure stops services first) |
| Cleaner: thumbnail cache | `thumbcache_*` | Mostly locked by Explorer | Low yield | No | n/a | **Keep**, show "in use" honestly |
| Others (temp, WER, dumps, logs, app/browser caches, shader cache, Delivery Optimization) | — | Verified | Space | some | n/a | **Keep** |

Never added (good): timer resolution, priority/affinity, memory cleaners, service disabling, network/TCP/IPv6 tweaks, MSI mode.

## 5. Bugs & technical debt (beyond §3)
| Sev | Issue | Where | Fix |
|---|---|---|---|
| High | Elevated Kestrel launches **HKCU** `UninstallString` with admin rights — when a standard user elevates with another admin's credentials, a user-planted command runs as admin | `uninstaller/installed_apps.rs` `uninstall` + `system/windows.rs` `shell_execute` | When elevated, refuse HKCU uninstallers (explain) or launch them unelevated |
| High | "Stop waiting" can't interrupt `WaitForSingleObject(…, INFINITE)`; a hung uninstaller blocks the Apps page forever | `system/windows.rs:66` | Wait in 500 ms slices, check the stop flag |
| High | Windows Update cache deleted while WU may be downloading | `cleaner/scanner.rs:150` | Check service state (SCM `QueryServiceStatus`), skip with a note if running |
| High | Health check walks every game folder (up to 20k files each) on **every** run, and runs after every apply/toggle | `optimizer/health.rs:223`; `app/mod.rs` L327/360/427 | Use launcher manifests' install drives (no walk); don't rerun health after each action |
| High | Scheduled-task toggles offered to standard users, then fail | `startup/manager.rs:120` (`machine=false`) | Read the task's principal/ACL; disable toggle with a reason |
| Med | Registry/COM work on the UI thread (startup toggles, per-game toggles) | `ui/mod.rs:495`, games tab | Run via existing `Job` |
| Med | Notification error detection by string matching | `ui/components.rs:577` | Return typed results (`Ok/Err` lists) |
| Med | Game Mode matches games by file name only | `gamemode/mod.rs` `running_game` | Match full path via `QueryFullProcessImageNameW` with `PROCESS_QUERY_LIMITED_INFORMATION` (Investigate anti-cheat tolerance first) |
| Med | Build depends on hand-set `RUSTFLAGS`, `CARGO_TARGET_DIR` and GNU-only crate pins | build setup | `.cargo/config.toml`, or switch to MSVC; add CI |
| Low | `history.log` grows forever | `utils/logging.rs` | Rotate at ~1 MB |
| Low | No app icon / version info (title bar shows egui "e") | `main.rs` viewport | `ViewportBuilder::with_icon`; `.rc` manifest/version with MSVC |
| Low | Tests for registry code write to the real HKCU | `gamemode` live tests | Make the registry root configurable for tests |

## 6. Competitor research (verified vs claimed)
| Tool | Useful idea | Verdict for Kestrel |
|---|---|---|
| HDOpti | Game Mode auto-detect; restore point on dashboard; debloater; per-game configs | Already have the safe parts; configs/hidden driver flags/network packs unverifiable → skip |
| Microsoft PC Manager | Deep cleanup, **storage view of large files**, startup apps, health check; "Boost" memory/process cleanup | Add a read-only large-files view; **skip Boost** (Windows manages memory; closing processes is placebo/risky) |
| WinUtil (Chris Titus) | Restore point before tweaks; "Caution" grouping | Already do both |
| Razer Cortex | Session booster | Covered by Game Mode (without killing processes) |
| Hone | Persistent tweaks + network claims | Network claims vendor-only → skip; "changed back" detection already done |
| PowerToys | Settings-app layout conventions | Already followed in 0.6.1 UI |

## 7. Recommended new features
| Feature | Problem | Benefit | Evidence | Cx | Risk | Priority | Rec. |
|---|---|---|---|---|---|---|---|
| Battery/laptop awareness | Performance preset/HP plan drain laptops; HP plan missing on Modern Standby | Safer defaults | `GetSystemPowerStatus`, Modern Standby docs | Low | Low | **High** | Add |
| Leftover check after uninstall | Folders/AppData remain | Space, tidiness | InstallLocation + name match | Med | Med (read-only list, open in Explorer, never auto-delete) | **High** | Add |
| Cleanup preview ("View files") | Users can't see what's deleted | Trust | Original spec asked for preview | Low | Low | **High** | Add |
| "Always" vs "While playing" clarity | Gaming tweaks and Game Mode overlap | Less confusion | UX review | Low | Low | **High** | Improve |
| Large-files view (read-only) | Space hogs invisible | Space | PC Manager storage view | Med | Low | Med | Add |
| Current vs Windows default + "applies on next game launch / restart" labels | Unclear effects | Clarity | UX | Low | Low | Med | Add |
| Export/import settings + backups | Reinstalls/new PC | Convenience | — | Low | Low | Med | Add |
| Windows Update / restart-pending status | Pending updates hurt sessions | Awareness | WU Agent API `ISystemInformation::RebootRequired` | Low | Low | Med | Add |
| System Protection status | Restore point button silently useless if protection is off | Trust | Checkpoint-Computer docs | Low | Low | Med | Add |
| Bulk uninstall queue (per-app confirm) | Many removals tedious | Convenience | — | Med | Med | Low | Add later |
| Opt-in tray mode for Game Mode auto | Auto only works while window open | Convenience | Shell_NotifyIcon; explicit opt-in, ~0 CPU | Med | Low | Low | Add later |
| Light theme / high contrast / Windows text-size | Accessibility | A11y | WinUI guidance | Med | Low | Low | Add later |
| Store-app startup entries | Not listed | Completeness | Only undocumented registry | Med | Med | Low | **Investigate** |

## 8. UI/UX improvements
1. Overview Status: show only items needing attention plus "7 checks OK" (expandable); today 9 rows push actions down.
2. Gaming vs Game Mode: rename sections "Always on" (Gaming) and "Only while playing" (Game Mode); show in Game Mode when an action is already permanently on.
3. Apps: sortable column headers; virtualised rows (`ScrollArea::show_rows`) for large lists; tooltip on "Protected".
4. Cleanup: per-category "View files"; show "in use" counts after cleaning.
5. Add app icon; remember window size (eframe persistence, local only).
6. Restart/relaunch labels on tweaks that need them (HAGS, windowed games, FSO).

## 9. Do not implement
Memory "boost"/RAM cleaners, killing or suspending processes, CPU priority/affinity, timer-resolution/HPET/bcdedit, network/TCP/Nagle/IPv6 tweaks, MSI-mode/interrupt tweaks, disabling services/Defender/Update, prefetch/SysMain wiping, registry cleaners, third-party driver updaters, hidden NVIDIA profile flags, controller-overclock drivers, duplicate-file *deletion* (too easy to delete wanted copies), FPS overlays/monitoring dashboards, scheduled automatic cleanup. Reasons: unverified or negative evidence, security cost, or conflict with Kestrel's philosophy (§9 of your brief).

## 10. Roadmap
- **0.7 — Reliability (next; implement on approval):** C1, C2, C3 · uninstall wait fix · elevated-HKCU uninstall guard · WU-cache service check · health-check without folder walks + no rerun after every action · task-toggle admin detection · startup/per-game toggles off the UI thread · typed results for notifications · remove "Limit background Store apps" (Win10-only) and move USB suspend/visual effects out of presets · battery-aware presets and HP plan · app icon · `.cargo/config.toml` so the moved project builds with plain `cargo build --release`.
- **0.8 — Usefulness:** cleanup preview, leftover check, large-files view, "always vs while playing" UX, current-vs-default + restart labels, compact Overview status, export/import, WU/restart-pending + System Protection status.
- **0.9+ — Optional:** bulk uninstall queue, opt-in tray for Game Mode, full-path game matching, light/high-contrast/text-size support, MSVC packaging with signed exe + winget manifest, CI.

## Top 10 recommended actions (by expected value)
1. Make backups atomic and locked (C2).
2. Block Restore everything while Game Mode is on (C1).
3. Fix background-recording tweak to `HistoricalCaptureEnabled` (C3).
4. Battery/laptop-aware presets and power plan.
5. Stop re-walking game folders in the health check; don't rerun after every action.
6. Fix "Stop waiting" + elevated HKCU uninstall guard.
7. Check Windows Update services before deleting its cache.
8. Cleanup "View files" preview.
9. Leftover check after uninstall (read-only).
10. Separate "Always on" vs "Only while playing" in the UI.

---

## Implementation plan for 0.7 (after approval)
Critical files: `kestrel/src/utils/backup.rs`, `app/mod.rs`, `optimizer/tweaks.rs`, `optimizer/gaming.rs`, `optimizer/health.rs`, `system/windows.rs`, `uninstaller/installed_apps.rs`, `startup/manager.rs`, `startup/tasks.rs`, `cleaner/scanner.rs`, `ui/mod.rs`, `ui/components.rs`, `main.rs`, new `.cargo/config.toml`, `CHANGELOG.md`, `AUDIT.md`.
Reuse: `Job` (`app/state.rs`) for off-thread work; `backup::save_original/get/remove`; `Tweak::unsupported` for gating; `info_bar`/`status` components; `running_processes` for checks; SCM access via `windows` crate (`Win32_System_Services` feature).

## Verification
- `cargo fmt --check`, `cargo clippy --all-targets` (0 warnings), `cargo test` (+ new tests: atomic backup round-trip under concurrent writers, Restore-everything blocked while Game Mode active, recording tweak keys, battery gating logic).
- Live (ignored) tests on your PC: Game Mode on → Restore everything refused → Game Mode off restores cleanly; recording tweak apply/revert leaves Win+Alt+R working (manual check); uninstall wait cancellation with a dummy long-running process.
- Release build from the new folder with plain `cargo build --release`; screenshots of changed screens; idle CPU stays 0 ms.

## Sources
- Microsoft: [Optimizations for windowed games](https://support.microsoft.com/en-us/topic/3f006843-2c7e-4ed0-9a5e-f9389e535952) · [Windows Update resources (stop services before clearing cache)](https://learn.microsoft.com/en-us/windows/deployment/update/windows-update-resources) · [Checkpoint-Computer](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/checkpoint-computer) · [Windows 11 typography](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/typography)
- Game DVR keys (community, unverified by Microsoft): [ElevenForum: Record what happened](https://elevenforum.com/t/enable-or-disable-record-what-happened-for-gaming-captures-in-windows-11.17900/) · [Microsoft Q&A answer](https://learn.microsoft.com/en-us/answers/a/7594237) · [TheWindowsClub: Game DVR](https://www.thewindowsclub.com/enable-disable-game-dvr-windows-1)
- Fullscreen optimizations on 24H2 (community): [GeekRewind: what breaks](https://geekrewind.com/what-breaks-when-you-turn-off-full-screen-optimizations-in-windows-11-advanced-app-properties/) · [ElevenForum](https://elevenforum.com/t/enable-or-disable-fullscreen-optimizations-for-games-in-windows-11.4945/)
- PC Manager: [WTOP / Data Doctors (2026)](https://wtop.com/tech/2026/01/data-doctors-should-i-be-using-microsoft-pc-manager) · [BetaNews review](https://betanews.com/article/microsoft-pc-manager-feels-like-ccleaners-little-brother) · [HotHardware](https://amp.hothardware.com/news/microsoft-pc-manager-could-speed-up-performance-or-slow-it-down)
- Earlier session research reused: HDOpti site/patch notes; cleaner junction CVEs (ZDI CCleaner/Trend Micro, AdwCleaner CVE-2025-67905); IPv6 guidance; WinUtil docs; Hone/Razer Cortex comparisons.

---

## Addendum (0.10): why users noticed little, and what changed

**Findings**
- Most preset changes were already Windows 11 defaults: Game Mode is on by default, "Record what happened" is off by default, and HAGS is on by default on supported GPUs. Applying them changes nothing, so nothing can be felt.
- Transparency and window animations are cosmetic. USB selective suspend and mouse acceleration fix specific problems, not general performance.
- Game Mode's Balanced profile repeated those defaults, so beyond Windows' own Game Mode it did almost nothing.
- The old Performance step (High performance plan) is a legacy plan. Windows 11's supported switch is the **power mode** (Best performance), which also exists on Modern Standby laptops that lack the old plans. Power modes only apply on the Balanced plan.
- On the developer's PC every setting was already optimized (High performance plan, Best performance mode, HAGS on, 165 Hz): a well-configured PC has nothing left to gain from settings, and Kestrel should say so instead of re-applying them.
- What makes a noticeable difference is fixing a real bottleneck: refresh rate below the monitor's maximum, memory not at rated speed (XMP/EXPO), heavy background apps eating memory, a laptop held back by its power mode, games on a hard drive, or overlays. These are what the health checks and Game Mode now target.

**Changes**
- Added the Best performance power mode (setting, preset and Game Mode step). It reports "Not needed" when the plan isn't Balanced.
- Added a Memory in use check that names the largest open apps.
- Settings already set or without effect are skipped and reported instead of being re-applied, and Optimize PC's result summary is grouped.
- Windows 11 only.

**Deliberately not changed**
- Memory integrity / Virtual Machine Platform: Microsoft has said turning these off can help gaming, but they're security features, so Kestrel only reports their state.
- HAGS: it's required for DLSS Frame Generation and on by default, but benchmarks are mixed (about -5 to +5 %) and changing it needs admin rights and a restart, so the check stays informative.
- Network "latency tweaks", timer resolution, process priority and service changes: still no credible evidence of benefit.

**Evidence and status**
- Power mode functions (`PowerGetActualOverlayScheme`, `PowerSetActiveOverlayScheme` in powrprof.dll) are what Settings > Power uses, but they are **not in Microsoft's public documentation**. Kestrel loads them at run time and fails cleanly if they're missing. Tested without admin rights on Windows 11 build 26200.
- Mode GUIDs (Best performance `ded574b5-…`, Balanced all zeros, Best power efficiency `961cc777-…`) come from community sources and match this PC's registry.
- Sources: [Optimizations for windowed games in Windows 11](https://support.microsoft.com/en-us/topic/3f006843-2c7e-4ed0-9a5e-f9389e535952) · [PowerSetActiveScheme (documented plan API)](https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powersetactivescheme) · [Set-PowerMode script using the overlay API (community)](https://www.powershellgallery.com/packages/ARC/0.0.1/Content/Public%5COS%5CPower%5CSet-PowerMode.ps1) · [Microsoft's VBS/memory integrity gaming guidance, as reported by BetaNews](https://betanews.com/2022/10/07/microsoft-says-that-gamers-can-boost-performance-by-lowering-windows-11-security-settings) · [Pureinfotech summary](https://pureinfotech.com/optimize-gaming-performance-windows-11/) · [HAGS and DLSS Frame Generation (Nixxes)](https://support.nixxes.com/hc/en-us/articles/48432027293971-How-to-use-DLSS-Frame-Generation) · [PCWorld on HAGS](https://www.pcworld.com/article/2339130/should-you-enable-hardware-accelerated-gpu-scheduling-in-windows-11.html)