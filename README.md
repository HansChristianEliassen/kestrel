# Kestrel

A small native Windows 11 cleaner and gaming optimizer written in Rust (egui).
No services, no telemetry, nothing runs until you click. The only network access is the optional update check
(asked on first launch, at most once a day, to github.com).

## Download
Get `kestrel.exe` from the [latest release](https://github.com/HansChristianEliassen/kestrel/releases/latest) and run it; there's no installer. It needs **Windows 11** (64-bit). The exe isn't signed with a Windows code-signing certificate yet, so SmartScreen may warn the first time ("More info" > "Run anyway"). Each release lists the exe's SHA-256 so you can check your download. Releases from 0.10.0 can update themselves, if you turn on update checks.

## Features

The interface follows a small Windows 11-style design system described in [DESIGN.md](DESIGN.md). The Overview shows the main actions, the Game Mode switch and a health-check status list.

| Page | What it does | How |
|---|---|---|
| Cleanup | **Junk files** (each category has "View files") and **Large files** (read-only list of files of 250 MB+ in your personal folders, with "Show in folder"). Scans temp files, Recycle Bin, browser/app caches, thumbnails, Windows Update cache, error reports, crash dumps, old Windows logs, shader caches (opt-in, with a stutter warning), Delivery Optimization files | Read-only scan of a fixed list of known locations; deletes only the scanned files after confirmation. Each file is opened, checked and deleted through one handle, and only if it still resolves inside a scanned folder, so a folder swapped for a junction can't redirect the delete (important when running as administrator). Skips links, files in use, read-only files, Temp files newer than 24 h, and any app's cache while that app is running. Recycle Bin uses `SHEmptyRecycleBinW`; Delivery Optimization uses `Delete-DeliveryOptimizationCache`. |
| Apps | Desktop apps (Uninstall registry keys) and Store apps (Windows' `PackageManager` API, with display names and sizes), with search, sort by name/size and an "optional preinstalled" filter | Runs the app's own registered uninstaller (`MsiExec /X` for MSI) or `PackageManager.RemovePackageAsync`, only after a confirmation that shows name, publisher, version and consequences. Drivers, anti-cheat, runtimes, Windows components and security software can't be uninstalled from here. After an uninstall, folders the app may have left behind are listed (never deleted). Tick several apps to uninstall them one after another with a single confirmation. |
| Startup apps | Run keys (user, machine, 32-bit), both Startup folders (shortcut targets resolved for the publisher) and scheduled tasks that run at sign-in or boot (Windows' own `\Microsoft` tasks are left out) | Run keys and folders use the same `StartupApproved` flag Task Manager uses; tasks use Task Scheduler's own Enabled switch. The original entry is never touched. Security software can't be disabled. |
| Gaming | Grouped by category: Windows (Game Mode, "Record what happened" off), Appearance (transparency, animations), Graphics (optimizations for windowed games), Power & USB (Best performance power mode, High performance plan, USB selective suspend off), Input (mouse acceleration off). Balanced / Performance (adds the Best performance power mode) / Custom presets; settings that are already right or have no effect on this PC show as Not needed, live "Active" detection, per-tweak Revert, System Restore point button. Three tabs. **Per-game:** finds installed games (Steam, Epic, GOG, Battle.net, EA, Riot, Ubisoft, Rockstar) or any exe via the Windows file picker, with per-game switches for high-performance GPU, fullscreen optimizations off, and leaving the game out of windowed-game optimizations. **Health check:** refresh rate vs. monitor max (with a 15-second keep-or-revert fix), graphics driver age, RAM speed vs. rated (XMP/EXPO, from SMBIOS), storage (games on a hard drive, nearly full drives), network connection type and speed, running overlays and other optimizers, GPU scheduling, VRR/Auto HDR status, memory integrity | Per-user settings, `powercfg` and `SystemParametersInfo` only. The original value is saved to `%LOCALAPPDATA%\Kestrel\backup.txt` before the first change; Revert or Restore all puts it back. Health checks never change anything. |
| Game Mode | Temporary, user-approved changes while you play: Balanced / Performance / Custom profiles (per game if you like), turned on by hand or automatically when a listed game starts | Only reversible settings (Windows Game Mode, background recording, power plan, USB selective suspend) plus, for apps you list, Windows' efficiency mode (EcoQoS and below-normal priority, undone afterwards) or a normal close request. The app picker shows memory use. Originals go to `gamemode-session.txt` first, with a RunOnce entry for crash/restart recovery. Detection uses the foreground-window event and a process snapshot (no polling while idle; checks every 5 s while a game runs; never opens the game's process). Ends, and restores everything, when Kestrel closes. With Settings > Minimize to tray when closed (off by default), it keeps watching from the notification area after the window is closed; Kestrel never starts with Windows. |
| Optimize PC | Scan, review and untick, confirm, apply, summary | Combines the Performance preset, the cleaner and the refresh-rate fix (with its keep-or-revert dialog), shows each setting's benefit and how many are already active, and lists other health findings for review. Optional restore point first. Startup apps are listed for review, never disabled automatically. |
| Settings | Restart as administrator, Restore everything, activity history | Restore everything puts back every original Kestrel saved: gaming settings, startup flags, per-game settings and display refresh rates (all kept in `%LOCALAPPDATA%\Kestrel\backup.txt`). History is written to `%LOCALAPPDATA%\Kestrel\history.log`. |

### Deliberately not included
- **"Unused app" detection.** Windows doesn't keep reliable last-used data for desktop apps, so Kestrel doesn't guess.
- **Windows.old and component store (WinSxS) cleanup.** Use Settings > System > Storage > Cleanup recommendations, which uses the supported servicing stack.
- **Defender, Windows Update, services, network "tweaks" and HPET/timer hacks.** These are unsafe or have no proven benefit.
- **Store app sizes.** Windows doesn't report them reliably, so they're left blank.

## Build

You need Rust (stable) from <https://rustup.rs>. The default **MSVC** toolchain (with the Visual Studio C++ Build Tools) needs no extra steps:

```bash
cargo build --release
```

The executable is `target\release\kestrel.exe`. It's a single standalone exe of about 4 MB with LTO, stripped symbols and `panic = "abort"`.

Run the tests:

```bash
cargo test
```

To run a read-only pass over the real system (it prints scan results, apps, startup entries and tweak states, and changes nothing):

```bash
cargo test -- --ignored --nocapture
```

### Building with the GNU toolchain (no Visual Studio)
The minimal `x86_64-pc-windows-gnu` toolchain has no `libshlwapi.a`, so `.cargo/config.toml` lets the linker read the system DLL directly; plain `cargo build --release` works. `Cargo.lock` also pins a few transitive crates (`parking_lot`, `libloading`, `arboard`, `webbrowser`) to versions that don't need `dlltool`.

## Updates
Opt-in (asked once on first launch; Settings > Updates). Kestrel follows `<repository>/releases/latest` (from `repository` in Cargo.toml), so drafts and prereleases are never offered. An update installs only if its `kestrel-update.sig` carries a valid ECDSA P-256 signature from the release key built into `src/update/mod.rs`. The signature covers the version and the exe's SHA-256, so a checksum from GitHub alone is never trusted, and an older or relabelled build is refused. The verified exe is saved as `kestrel.exe.new`; after you confirm, the running exe is renamed to `kestrel.exe.old`, the new one takes its place, and it starts once Kestrel has closed. The old exe is deleted when the new version starts; if the new one won't start, rename `kestrel.exe.old` back. Kestrel in a folder that needs admin rights (Program Files) is told to update by hand, never elevated.

**The repository (or at least its releases) must be public**, otherwise GitHub hides the releases from the check.

## Releasing
1. Bump `version` in Cargo.toml and add a CHANGELOG entry.
2. `powershell -File scripts\release.ps1`: runs fmt/clippy/tests, builds, signs with `%USERPROFILE%\.kestrel\release-signing.key` and writes `dist\v<version>\kestrel.exe` + `kestrel-update.sig`.
3. Tag `v<version>`, push the tag, create the GitHub release and attach **both** files.

The signing key never goes into the repository or CI. Back it up somewhere safe: without it no further update can be signed, and users would have to install the next version by hand once (with a new key built in). Releases without `kestrel-update.sig` (such as 0.9.0) can't be installed automatically.

## Packaging
- **Portable:** ship `kestrel.exe` on its own. It needs no installer, no runtime and no DLLs beyond what Windows provides.
- **Admin rights:** the exe runs as the current user (`asInvoker`). Machine-wide cleanup locations and all-users startup entries need the in-app "Restart as administrator" button, which shows a normal UAC prompt.
- **Optional installer:** wrap the exe with Inno Setup or WiX, or `winget`/`scoop` manifests pointing at a release zip. Sign the exe with `signtool sign /fd SHA256 /a kestrel.exe` to avoid SmartScreen warnings.

## Layout
```
src/
├── main.rs              window setup
├── app/                 App state, background jobs, actions
├── ui/                  components.rs (design system, see DESIGN.md), mod.rs (theme, navigation, screens), gamemode.rs
├── cleaner/             scanner.rs (categories, read-only scan), cleanup.rs (deletion)
├── uninstaller/         installed_apps.rs
├── startup/             manager.rs
├── optimizer/           tweaks.rs (reversible settings), gaming.rs (presets, per-game settings), games.rs (game detection), health.rs (health checks)
├── gamemode/            mod.rs (profiles, snapshot, restore), watch.rs (foreground-event detection, closing apps)
├── system/              windows.rs (Win32 wrappers: ShellExecuteEx, Toolhelp, version info, Recycle Bin, ...), tray.rs
├── update/              mod.rs (release check, download, verify, install, preferences), crypto.rs (CNG), http.rs (WinHTTP)
├── bin/kestrel-sign.rs  release signing tool (not shipped)
└── utils/logging.rs     activity history
```

## License
Licensed under the [MIT license](LICENSE-MIT).
