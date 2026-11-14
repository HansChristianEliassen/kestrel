# HDOpti comparison

Sources: hdopti.com (home page, App Details, full patch notes from v1.1 to v4.7.11, and the articles index), checked 2026-11-14. HDOpti publishes feature names but not how they work, so wherever it says "hidden settings" or "network pack", I couldn't inspect the actual changes.

## Implemented in Kestrel

| HDOpti feature | Kestrel version | Why it's safe and worth having |
|---|---|---|
| Create restore point (dashboard) | "Create restore point" button, plus an opt-in step before Optimize PC | Uses `Checkpoint-Computer`. Kestrel reports when Windows skips one because of its documented one-per-24-hours limit. |
| Windows optimization: power plan | High performance plan (built-in, not a custom one) | Built-in Windows plan; Kestrel remembers your previous plan and can switch back. |
| Disable USB power-saving states | Turn off USB selective suspend | Documented power setting changed through `powercfg`. Applies to the active plan, while plugged in. |
| Adjust Windows appearance | Transparency and window animations off | Already in Kestrel before this. |
| GPU optimization | Optimizations for windowed games; per-game high-performance GPU | Windows' own per-user settings, the same ones Settings > Graphics writes. Fully reversible. |
| XMP/DOCP detection | Memory speed health check | Read-only. Compares the configured RAM speed with the speed Windows reports for the modules. |
| Debloater / app management | App Uninstaller with optional-preinstalled filter | Already in Kestrel. Now also warns before removing developer tools such as Git or Python. |
| Disable startup apps | Startup Manager | Already in Kestrel; uses the same switch as Task Manager. |
| Revert Windows optimization | Per-tweak Revert and Restore all | Original values are saved before the first change. |
| Junk cleanup | System Cleaner | Already in Kestrel. |
| Heat indicators on tweaks | A drawback line on every tweak | Each tweak states its cost, not just its benefit. |

Also added, though HDOpti doesn't offer them: a refresh-rate check (it found a 165 Hz display running at 60 Hz on this PC), a read-only status for hardware GPU scheduling, and memory-integrity status.

## Rejected

| HDOpti feature | Reason |
|---|---|
| Network pack ("reset internet configuration and apply tweaks") | Resetting Winsock or IP settings is a troubleshooting step, not a speed-up. The usual TCP tweaks (Nagle, throttling index, autotuning) have no proven effect on modern Windows and can make things worse. |
| IPv6 enable/disable | Microsoft advises against disabling IPv6. Windows is only tested with it on. |
| MSI mode with high interrupt priority | Unsupported per-device registry edit. Modern GPU drivers already use MSI where it's supported. Forcing it on a device that doesn't support it can stop it working or prevent boot. Windows ignores the "priority" value. |
| NVIDIA/AMD "hidden driver settings" (Profile Inspector), alternative Resizable BAR flags | Undocumented, change with each driver version, and can't be verified. The official control-panel settings stay with the vendor apps. |
| Controller/KBM overclocking | Needs patched or test-signed USB drivers. That weakens security and can trigger anti-cheat. |
| Delete prefetch files | Prefetch makes apps launch faster. Deleting it slows the next launches, and Windows rebuilds it anyway. |
| Disable non-essential background processes and services | Your spec rules this out (Defender, Update, essential services). Effects are unpredictable. |
| Live CPU/GPU temperature monitoring | Your spec rules out hardware monitoring and background activity. |
| Custom "HD Optimisation" power plan | Contents aren't published, so it can't be verified. The built-in High performance plan covers the safe part. |

## Couldn't verify, so not implemented

| HDOpti feature | Note |
|---|---|
| HD OPTI Game Mode (per-game list) | How it works isn't published. Windows Game Mode is implemented instead. |
| Game configs (CoD, Fortnite, Valorant …) | Per-game config files that change with every game patch; their contents aren't public. |
| Renderer worker count tool | Specific to Call of Duty; no public evidence it helps. |
| Controller polling tester, USB port tester | Measuring tools rather than optimizations. They could be added later as read-only tools. |
| OBS presets, PC Shop, downloads | Outside what Kestrel is for. |

## Game Mode (researched for 0.6)

**Confirmed** (from the in-app screenshot on hdopti.com/app-details and the patch notes):
- It "automatically detects running games and applies optimized CPU settings to improve performance."
- It must be enabled before you launch the game. Once enabled, "it works in the background — you don't need to keep the HD OPTI application open."
- Once a game is detected, the optimizations stay active until that game closes. Detected games are listed under "Active Detected Processes".
- It only works with a list of supported games, which HDOpti adds to in updates (The Finals, Forza Horizon 6, CS2, Valorant…). Unsupported games are requested through their Discord.
- It launched as a beta in v4.5.0 (April 2026).

**Not published (so not copied):** which "CPU settings" it changes, how it detects games, and whether it changes power plans or closes or suspends anything.

**Assumption, labelled as such:** "works without the app open", "applies to supported games" and "stays until the game closes" all fit Windows' per-executable *Image File Execution Options → PerfOptions* registry keys (CPU/I/O priority applied by Windows itself at launch). That's an inference from the described behaviour, not something HDOpti documents.

**Kestrel's Game Mode, compared:**
| | HDOpti (confirmed) | Kestrel 0.6 |
|---|---|---|
| Game list | Their supported list | Any game: detected Steam/Epic/GOG/Battle.net/EA/Riot/Ubisoft/Rockstar games, or any .exe |
| What changes | "Optimized CPU settings" (unpublished) | Listed, reversible actions per profile: Windows Game Mode, background recording off, power plan, USB selective suspend, closing listed apps |
| Activation | Automatic, background | Manual, or automatic (opt-in) while Kestrel is open |
| Undo | Until the game closes | Restored when the game or Kestrel closes; snapshot plus RunOnce recovery after a crash or restart |
| Process priority | Likely (CPU settings) | Not included: Windows Game Mode already prioritises the foreground game, raised priority can starve audio and input threads, and HDOpti's own article warns that CPU affinity changes can worsen stutter |
| Suspending processes | Unknown | Not included: it relies on undocumented APIs and can hang apps or trip anti-cheat |

## Changes in this pass
- `optimizer/tweaks.rs`: windowed-game optimizations, USB selective suspend, tweak categories, helpers for DirectX preference strings (with tests).
- `optimizer/gaming.rs`: per-game high-performance GPU; Balanced preset now includes windowed-game optimizations.
- `optimizer/health.rs` (new): refresh rate, memory speed, GPU scheduling, memory integrity (read-only).
- `system/windows.rs`: `create_restore_point`.
- `uninstaller`: "Developer tool" chip and warning.
- UI: tweaks grouped by category, Health check section, restore-point button and Optimize PC option, per-game section with both actions.
