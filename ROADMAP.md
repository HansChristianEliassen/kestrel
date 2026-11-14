# Kestrel review and roadmap (research only, nothing implemented)

Effort scale: **S** ≈ under a day, **M** ≈ 1–3 days, **L** ≈ a week.
Evidence scale: **Documented** (Microsoft documents it), **Measured** (independent, repeatable results), **Plausible** (the mechanism is sound but the gain varies), **Weak** (vendor claims only).

## 1. Where Kestrel stands

**Strengths**
- Every change is opt-in: a preview and confirmation come first, and the original value is saved before the first change, with per-tweak Revert and Restore all.
- The tweak set is small and honest. Each tweak shows a benefit and a drawback, and the status detection reads the real setting.
- The cleaner never follows links, skips locked files and each app's cache while that app is running, and deletes only the files from the scan.
- The uninstaller always runs the vendor's own uninstaller, and drivers, anti-cheat and runtimes are protected.
- One ~4 MB exe with no service, telemetry or network access, and 0% CPU when idle.

**Weaknesses found in the code**
1. **Security (high).** When run as administrator, the cleaner deletes by path in folders that standard users can write to (`C:\Windows\Temp`, `ProgramData\…\WER`). The reparse check happens right before `remove_file`, but a parent folder can be swapped for a junction in between. That is the same bug class as the published junction/link-following privilege-escalation CVEs in CCleaner, Trend Micro Cleaner One, Malwarebytes AdwCleaner and Windows' own Disk Cleanup.
2. **Misleading "Active" status.** "Limit background Store apps" shows *Active* on your Windows 11 build 26200 because the registry value is set, but Microsoft removed the global toggle from Windows 11, and current guides only cite the value without confirming it still works. "Optimizations for windowed games" does nothing on Windows 10. Status should mean "takes effect", not just "value is written".
3. **Restore all is incomplete.** It doesn't cover startup changes, per-game settings or a kept refresh-rate change.
4. **Uninstall result is often wrong.** NSIS and Inno uninstallers copy themselves to a temp folder and exit straight away, so Kestrel reports "still installed" while the real uninstall is still running.
5. **Heavy use of PowerShell.** The Store app list, RAM check, Delivery Optimization cleanup and restore point each start `powershell.exe` (0.5–1.5 s each). These calls fail if PowerShell is blocked or locked down by policy, and the error text is whatever PowerShell prints.
6. **The startup list misses scheduled tasks.** Many updaters and launchers (Adobe, Google, Edge, OEM tools) start through logon tasks. Shortcut entries show "Unknown publisher" because the shortcut target isn't resolved.
7. **Per-game settings need a pasted path.** There's no file picker and no detection of installed games.
8. **UI.** The Gaming page is one very long scroll. The sidebar has no visible keyboard focus. The Optimize PC dialog doesn't include the health-check fixes.
9. **Build.** Four crates are pinned to old versions only to make the minimal GNU toolchain link, and the `winreg` crate duplicates what `windows` (already a dependency, with its Registry feature on) provides.

## 2. Prioritized new features

### High priority (must have)
| # | Feature | Why | Evidence | Effort |
|---|---|---|---|---|
| H1 | **Handle-based safe delete** | Fixes weakness 1. Open each file with `FILE_FLAG_OPEN_REPARSE_POINT`, check `GetFinalPathNameByHandleW` is still inside the scanned folder, then delete through the open handle (`SetFileInformationByHandle` with `FILE_DISPOSITION_INFO_EX`). | Documented APIs; the bug class is proven by the CVEs below | M |
| H2 | **Version-aware tweaks** | Read the Windows build with `RtlGetVersion`. Each tweak declares where it works, and the UI shows "Not supported on this Windows version" instead of a false *Active*. Re-verify or retire "Limit background Store apps". | Documented | S |
| H3 | **"Reverted by Windows" detection** | If a backup exists but the setting is no longer active, Windows Update or another app changed it back. Show this with a one-click reapply. | Documented (Kestrel reads the live value) | S |
| H4 | **Per-game hub** | Detect installed games from Steam (`libraryfolders.vdf` + `appmanifest_*.acf`), Epic (`ProgramData\Epic\…\Manifests\*.item`) and GOG (registry), plus a native file picker (`IFileOpenDialog`). One card per game: high-performance GPU, fullscreen optimizations, opting the game out of windowed-game optimizations, and a "used by Kestrel" summary. This replaces pasting paths. | Documented per-game Windows settings | M–L |
| H5 | **Reliable uninstall result** | After the uninstaller exits, keep checking every 2 s for the app's registry entry to disappear, with a "Still uninstalling… / Stop waiting" state. | Documented registry behavior | S |
| H6 | **Startup tasks and shortcut targets** | List logon-triggered scheduled tasks through Task Scheduler's COM interface (`ITaskService`) and allow enabling/disabling them, which is reversible. Resolve `.lnk` targets with `IShellLinkW` to show publishers. | Documented | M |
| H7 | **Replace PowerShell with native APIs** | Store apps through the WinRT `PackageManager` (`FindPackagesForUser`, `RemovePackageAsync`), which also gives display names and install folders for sizes. RAM speed from SMBIOS (`GetSystemFirmwareTable('RSMB')`, type 17). Keep PowerShell only for `Checkpoint-Computer`. | Documented | M |
| H8 | **GPU driver age check** | Read-only: GPU name, driver version and date (SetupAPI `DEVPKEY_Device_DriverDate`, or WMI over COM), with a link to the vendor. An old driver is one of the most common real causes of poor game performance. | Plausible to measured, depending on the game | S–M |

### Medium priority (worth adding)
| # | Feature | Why | Evidence | Effort |
|---|---|---|---|---|
| M1 | **Overlay and conflict check** (read-only) | Shows which overlays and boosters are active (Discord, Steam, NVIDIA, Game Bar, RTSS/Afterburner, Overwolf, other "boosters"), with a note that overlays hook into games and that two optimizers can undo each other. Report only. | Plausible; anti-cheat conflicts are documented by game publishers | S–M |
| M2 | **Storage checks** | Flags game libraries on a hard drive (`IOCTL_STORAGE_QUERY_PROPERTY` seek-penalty check) and drives with under ~10% free space. Optional DirectStorage readiness check (NVMe + Windows 11). | Measured (HDD load and stream stutter) | S–M |
| M3 | **Better Optimize PC** | Include health-check fixes (refresh rate, with the 15-second keep-or-revert), restore point ticked when running as admin, the benefit and drawback shown inline, and a count of settings already active. | n/a (UX) | S |
| M4 | **Restore all that's complete, plus export/import** | Restore all also covers startup, per-game and display changes. Export and import a plain-text profile of tweaks and per-game settings. | n/a (safety) | S–M |
| M5 | **Bulk optional-app removal** | Tick several preinstalled Store apps and remove them in one previewed batch. Optionally (as admin, with a warning) also remove the provisioned copy so new user accounts don't get it. | Documented (`Remove-AppxProvisionedPackage` / `PackageManager.DeprovisionPackageForAllUsersAsync`) | M |
| M6 | **Graphics status: VRR and Auto HDR** | Show the state of the global variable-refresh-rate and Auto HDR settings, which live in the same `DirectXUserGlobalSettings` value Kestrel already handles, with a Settings link. Check the exact value names on a Windows 11 test machine before adding toggles. | Documented feature; the registry names are community-verified | S |
| M7 | **Delivery Optimization upload off** | "Allow downloads from other PCs" uploads update data to other PCs, using your upload bandwidth during play. Settings-backed and reversible; needs admin. | Documented setting | S |
| M8 | **Cleaner: supported deep cleanup** | Component store analysis and cleanup through `DISM /Online /Cleanup-Image /AnalyzeComponentStore` and `/StartComponentCleanup` (Microsoft's supported method). GPU driver installer leftovers (`C:\NVIDIA`, `C:\AMD`), only when older than 7 days and no installer is running. | Documented (DISM); plausible (leftovers) | M |
| M9 | **Faster scan, less memory** | Scan categories in parallel with `std::thread::scope`, which adds no dependency. | n/a | S |
| M10 | **Fewer dependencies, cleaner build** | Replace `winreg` with `windows` registry calls; recommend the MSVC toolchain and drop the GNU-only version pins. Add a manifest, version info and icon (`.rc` via a build script), and a single-instance mutex so two Kestrel windows can't fight over the backup file. | n/a | S–M |

### Low priority (optional)
| # | Feature | Notes | Effort |
|---|---|---|---|
| L1 | "Before you play" | Closes apps you pick (browser, launchers) politely with `WM_CLOSE`, as a one-off action with no background watcher. Helps mainly on 8–16 GB RAM PCs. | S–M |
| L2 | Memory integrity and Virtual Machine Platform guidance | Microsoft's own gaming article says turning these off can help, while warning about the security cost. Keep it to status, explanation and links; never toggle them. | S |
| L3 | Network status check | Ethernet vs. Wi-Fi, Wi-Fi band and link speed, read-only. No "tweaks". | S |
| L4 | Notifications while gaming | Check that Windows' "Do not disturb when playing a game" rule is on. | S |
| L5 | Undo from the history page | Each history entry gets its own undo where possible. | M |
| L6 | Input polling tester | Measures mouse or controller report rate with Raw Input, started by the user. A useful diagnostic, not an optimization. | M |

## 3. Improvements to existing features
- **Interface:** split the Gaming page into *Settings / Health / Per-game* tabs; add evidence labels ("Documented", "Cosmetic", "Varies"); show "Admin needed" and "Restart needed" labels; paint a keyboard-focus outline on the sidebar items.
- **Scan speed:** parallel scanning (M9); drop the PowerShell calls (H7). The first Apps load should go from about 1.5 s to under 0.3 s.
- **Exe size and memory:** 4.3 MB is fine. Most of the ~85 MB of memory is the OpenGL driver rather than Kestrel. Switching to wgpu would be heavier, so leave it as is.
- **Error handling:** turn HRESULT and `DISP_CHANGE` codes into plain sentences, and tell "UAC prompt declined" apart from a real failure.
- **Cleanup safety:** H1; also never delete files with both hidden and system attributes outside known cache folders, and store a per-category file limit in the scan result.
- **Compatibility:** H2; test on Windows 10 22H2 and Windows 11 23H2 and later; the USB suspend setting only applies to the active power plan (document this, or apply it to the High performance plan too when both are chosen).
- **Restore:** M4; save the Windows build number and date with each backup, and warn if a backup is from an older Windows build.

## 4. Ideas taken from HDOpti and others
| Source | Idea | What Kestrel should do |
|---|---|---|
| HDOpti | Restore point on the dashboard; heat icons on tweaks | Done; a drawback line already covers heat. |
| HDOpti | Game configs and a per-game "Game Mode" | Use the per-game hub (H4) with Windows' own per-app settings, not in-game config files that break with every patch. |
| HDOpti | USB tester and polling tester | L6, as a diagnostic only. |
| WinUtil (Chris Titus) | Restore point before tweaks; tweak groups with an "Advanced – CAUTION" level; feature installs | The caution level is a good idea, so use evidence labels. Skip the service-disabling and Edge-removal tweaks. |
| Razer Cortex | Booster that restores the PC after a game session | L1 without a resident watcher. Its own reviews describe the gains as small. |
| Hone | Persistent tweaks with a paid tier; network/TCP "optimization" | Persistence plus *revert detection* (H3) is the useful part; the network claims are vendor-only (see section 5). |
| Windows Settings | Per-game graphics, VRR, Auto HDR | H4 and M6 bring these together in one place. |

## 5. Features to avoid
| Feature | Why |
|---|---|
| TCP and Nagle tweaks (`TcpAckFrequency`, `TCPNoDelay`), `NetworkThrottlingIndex`, `SystemResponsiveness`, "network packs" | Games set their own socket options and mostly use UDP. No independent evidence, and changing these can hurt throughput. |
| Turning off IPv6 | Microsoft advises against it; Windows is only tested with IPv6 on. |
| Forcing MSI mode, interrupt affinity or priority | Unsupported per-device registry changes that can break a device or prevent boot; modern drivers already use MSI where supported. |
| Timer resolution, HPET or `bcdedit` tweaks | Windows 10 2004 and later changed how the global timer works. Results are mixed and machine-specific, and the changes are hard to undo. |
| Turning off Spectre/Meltdown mitigations or Defender, services or Windows Update | A security cost for small gains, and outside your spec. |
| Core-parking hacks, CPU affinity or priority changes | HDOpti's own article warns that affinity can make stutter worse; the Windows scheduler handles hybrid CPUs. |
| RAM cleaners, standby-list purging, prefetch or SysMain off | They undo caches Windows uses to make things faster. |
| Registry cleaners | No measurable benefit, and they can break things. |
| NVIDIA Profile Inspector "hidden" flags, Resizable BAR overrides | Undocumented and different in every driver version. |
| Controller or mouse "overclock" drivers | Need test-signed drivers, which weakens security and can trigger anti-cheat. |
| Live monitoring, FPS overlays, a resident auto-booster | Outside your spec, and they cost resources while gaming. |
| "Ultimate Performance" plan as a default | Built for workstations; no gaming gain over High performance on typical PCs and more idle power use. Fine as an informational note only. |

## 6. How to build these in Rust
- **Safe delete (H1):** `CreateFileW(path, DELETE | FILE_READ_ATTRIBUTES, FILE_SHARE_*, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)`, then `GetFileInformationByHandleEx(FileAttributeTagInfo)` to reject reparse points, then `GetFinalPathNameByHandleW` and check the result starts with the scanned root, then `SetFileInformationByHandle(FileDispositionInfoEx, FILE_DISPOSITION_FLAG_DELETE | POSIX_SEMANTICS)`. All of these are `windows` crate features (`Win32_Storage_FileSystem`). Add a test that creates a junction inside a temp folder and checks that nothing outside it is deleted.
- **Windows version (H2):** `RtlGetVersion` (`Wdk_System_SystemServices`), or `GetVersionExW` with a manifest that declares Windows 10/11 support. Add a `min_build`/`max_build` to each tweak.
- **Game libraries (H4):** parse VDF by hand (a few lines: quoted key/value pairs) rather than adding a crate. Epic manifests are JSON, but only `InstallLocation` and `LaunchExecutable` are needed, so a targeted string search avoids a new dependency. File picker: `CoCreateInstance(FileOpenDialog)` with `Win32_UI_Shell_Common`.
- **Task Scheduler (H6):** `ITaskService::Connect`, then `GetFolder("\\")` and a recursive walk; keep tasks whose triggers include `TASK_TRIGGER_LOGON` or `TASK_TRIGGER_BOOT`; toggle with `IRegisteredTask::put_Enabled`, and record the original value in `backup.txt`.
- **WinRT (H7):** `windows` crate features `Management_Deployment` and `ApplicationModel`. `PackageManager::FindPackagesByUserSecurityId("")`, `Package::DisplayName`, `InstalledLocation`, and `RemovePackageAsync(..).get()` on the existing worker thread.
- **SMBIOS RAM (H7):** `GetSystemFirmwareTable(u32::from_be_bytes(*b"RSMB"), 0, ..)`, then walk the structures for type 17 and read Speed (offset 0x15) and Configured Memory Speed (offset 0x20, SMBIOS 2.7 and later). Add a unit test with a captured byte blob.
- **Keep the structure:** each item goes into the existing modules (`optimizer/health.rs`, `startup/manager.rs`, `uninstaller/`) with a small `#[test]` beside each parser. No new background threads beyond the existing one-shot `Job`.

## 7. Roadmap
| Release | Contents | Goal |
|---|---|---|
| **0.2: Safety and honesty** ✔ done | H1, H2, H3, H5, M4 (complete Restore all; export/import still open) | Nothing can be tricked into deleting the wrong files, and every status is true. |
| **0.3: Per-game hub** ✔ done | H4 (plus Battle.net, EA, Riot, Ubisoft, Rockstar), M6 as status only, file picker, Gaming page tabs | The most-requested gamer workflow, done with Windows' own settings. |
| **0.4: Faster and native** ✔ done | H7, H6, M9, single instance from M10. Still open from M10: replacing `winreg` (about 40 call sites for one small crate) and an embedded manifest, icon and version info (needs a resource compiler, which the GNU toolchain here doesn't have) | No PowerShell on hot paths, complete startup coverage. |
| **0.5: Health and conflicts** ✔ done | H8, M1, M2, M3, L3 | Find the real causes of bad performance (refresh rate, driver, disk, overlays) and fix them from Optimize PC. |
| **0.6: Game Mode** ✔ done | HDOpti-inspired Game Mode (replaces L1, "before you play"): profiles, per-game profiles, auto-start on game launch, closing listed apps, crash/restart recovery | Temporary gaming changes that always come back. |
| **0.7: Polish** | M5, M7, M8, export/import (M4), L2, L4–L6; Game Mode follow-ups: optional "reopen closed apps", tray/minimize-to-tray so detection keeps running with the window closed, match games by full path | Nice-to-haves once the core is solid. |

## Bottom line
- **What's missing:** a per-game hub with game detection, scheduled-task startups, driver/overlay/disk health checks, and version-aware status.
- **What to fix first:** privileged deletion (a real security bug), false *Active* labels, uninstall-result tracking, and Restore all coverage.
- **What helps gamers most:** the refresh-rate fix (done), per-game GPU and windowed settings with game detection, driver-age and HDD checks, and an overlay/conflict report. These address common, measurable problems rather than placebo tweaks.
