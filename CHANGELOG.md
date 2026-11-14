# Changelog

## 0.9.0: Bulk uninstall and tray
- **Bulk uninstall:** tick several apps on the Apps page and choose Uninstall selected. One confirmation lists every app (and warns about developer tools); then each app's own uninstaller runs, one after another. "Stop waiting" moves on to the next app, "Cancel the rest" stops the queue after the current one. Results come in one notification, and leftovers are listed per app. Protected apps and apps without an uninstaller can't be ticked.
- **Minimize to tray (opt-in):** Settings > Window has a new switch, "Minimize to tray when closed". Closing the window then tucks Kestrel into the notification area (minimized, with no taskbar button; a fully hidden window made the UI library repaint constantly) so automatic Game Mode keeps working at 0 ms idle CPU. Click the icon to reopen; its menu has Exit, which ends Game Mode and restores everything as before. Off by default, and Kestrel still never starts with Windows.

## 0.8.0: Usefulness
- **Cleanup preview:** each category has "View files", listing what would be deleted (first 200 files).
- **Large files:** a new Cleanup tab lists files of 250 MB or more in Downloads, Desktop, Documents, Videos, Pictures and Music, with "Show in folder". Read-only; links are never followed.
- **Leftovers after uninstall:** once a desktop app is gone, Kestrel lists its install folder and folders named after it in AppData and ProgramData, with sizes and an Open button. It never deletes them, and never suggests shared folders like Program Files.
- **Overview:** the status list shows only what needs attention; the checks that are fine are one click away. New check: a restart Windows Update is waiting for.
- **Clearer wording:** Gaming says its settings stay on until reverted, Game Mode says its changes only last while you play. Windowed-game and per-game settings say they take effect the next time the game starts; the fullscreen-optimizations switch warns that Auto HDR or variable refresh rate may stop working for that game.

## 0.7.0: Reliability
Fixes from the audit in [AUDIT.md](AUDIT.md).
- **Backups:** `backup.txt` is now written to a temporary file and renamed into place, under one lock, so a crash or two changes at once can't lose saved originals.
- **Restore while Game Mode is on:** Restore everything and Revert are refused while Game Mode is on (ending Game Mode would have undone them).
- **Background recording:** the tweak now turns off only "Record what happened" (`HistoricalCaptureEnabled`). Older versions also turned off all Game Bar capture; Revert puts those values back too.
- **Uninstalling:** "Stop waiting" now also works while the uninstaller itself is still open. When Kestrel runs as administrator, it refuses to run uninstallers registered for your account only (any program can change those), and says to open Kestrel normally instead.
- **Windows Update cache:** skipped while the Windows Update service is running, instead of deleting files it may be using.
- **Health check:** finds game drives from launcher manifests instead of searching every game folder.
- **Startup apps:** scheduled tasks that run under another account are marked as needing administrator rights instead of failing when switched. Switching runs in the background.
- **Presets:** Performance now adds only the High performance power plan, and leaves it out on PCs with a battery. USB selective suspend, transparency, animations and mouse acceleration are Custom only. "Limit background Store apps" is no longer offered (Windows 10 only); Revert still works for it.
- **Errors** now always stay until dismissed. The window and taskbar show a Kestrel icon.
- **Build:** `.cargo/config.toml` holds the GNU linker flag, so `cargo build --release` works without setting `RUSTFLAGS`.

## 0.6.1: Interface redesign
A new design system ([DESIGN.md](DESIGN.md)) and every screen rebuilt on it.
- **Fonts and icons:** Segoe UI with real Semibold, Windows' own Segoe Fluent Icons in the navigation, a neutral Windows 11 dark palette with one accent color, and a 4 px spacing grid.
- **Lists:** list groups with dividers replace a card per item; tables with columns for Cleanup and Apps; Windows-style toggles and checkboxes.
- **Status:** status is a dot and a word instead of colored badges, and emoji are no longer used as icons.
- **Navigation:** pages are now Overview, Cleanup, Apps, Startup apps, Gaming, Game Mode, with Settings at the bottom. The health check moved to the Overview.
- **Notifications:** results show as non-blocking notifications instead of modal dialogs; errors stay until dismissed.
- **Keyboard and states:** every control shows a keyboard focus outline, and loading, empty and disabled states are explained.
- **Settings:** a conventional grouped layout (Permissions, Recovery, Activity, About).

## 0.6.0
**Game Mode** (new page): temporary, user-approved changes while you play, undone afterwards.
- **Profiles:** Balanced (makes sure Windows Game Mode is on, pauses background recording), Performance (adds the High performance power plan, keeps USB devices awake, closes the apps in your list) and Custom (pick the actions; saved). Each configured game can use its own profile.
- **Turning it on:** by hand, or automatically when a listed game starts (optional, off by default). Detection listens for Windows' foreground-window event, so it uses no CPU between window switches. It reads a process snapshot and never opens the game's process. While a detected game runs, it checks every 5 seconds whether the game has closed.
- **Closing apps:** only apps you list, closed with a normal close request (no force-closing). Windows components, security software and anti-cheat can't be listed. Closed apps aren't reopened.
- **Recovery:** each original value is saved to `gamemode-session.txt` before it changes, and a RunOnce entry is registered. A crash, power cut or restart therefore gets everything restored at the next sign-in or the next Kestrel launch. Closing Kestrel ends Game Mode and restores everything. A restore that fails keeps its entry for another try and is reported as failed.
- Version set to 0.6.0. Added `rustfmt.toml` (`max_width = 140`, matching the existing style); the code is now `cargo fmt` and `cargo clippy` clean.

## 0.5.0
Health checks for graphics driver age, storage (games on a hard drive, nearly full drives), network connection and running overlays or other optimizers. Optimize PC now includes the refresh-rate fix, shows each setting's benefit and how many are already active, and lists other findings.

## 0.4.0
Parallel cleaner scan. Store apps through Windows' PackageManager API (display names, sizes, native removal). RAM speed from SMBIOS. Startup Manager covers sign-in/boot scheduled tasks and resolves shortcut targets. Single instance.

## 0.3.0
Per-game hub: detects Steam, Epic, GOG, Battle.net, EA, Riot, Ubisoft and Rockstar games, plus a native file picker. Per-game switches for high-performance GPU, fullscreen optimizations and windowed-game optimizations. VRR/Auto HDR status. Gaming page split into tabs.

## 0.2.0
Handle-based safe deletion (junction redirects refused). Version-aware tweak labels and "changed outside Kestrel" detection. Uninstall waits until the app is really gone. Restore everything covers startup, per-game and display changes.

## 0.1.0
First release: system cleaner, app uninstaller, startup manager, gaming optimizer with reversible tweaks, Optimize PC, refresh-rate fix, activity history.
