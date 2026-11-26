# Changelog

## 0.28.2: NVIDIA gaming profile fix
- **GPU > Settings no longer says "This PC has no NVIDIA graphics card" when there is one.** The check read the driver's global shader cache size, which a clean driver installation leaves unset. Kestrel now only checks that NVIDIA's driver responds, and says "Needs an NVIDIA graphics card and driver" when it really doesn't.

## 0.28.1: Safer driver installs
A driver install from the Driver Optimizer looked like a crash: Kestrel disappeared, and with clean installation on, Windows showed no graphics driver for a few minutes while NVIDIA's installer kept working out of sight. Restarting at that moment could really leave the PC without a driver.
- **A visible install window:** NVIDIA's installer now runs in a window that says what's happening, that a missing driver for a while is normal, and not to restart or turn off the PC until it closes. Kestrel opens again with the result, as before.
- **Kestrel closes itself first:** NVIDIA's OpenGL driver ends OpenGL programs like Kestrel while the display driver restarts. Kestrel now closes cleanly right after starting the installer instead of being killed. The confirmation says this before you start.
- **Opened mid-install:** Kestrel says NVIDIA's installer is still running, and updates when it's done.
- **Interrupted installs:** if the PC restarted or the installer stopped before finishing, Kestrel says so. If Windows has no NVIDIA driver left, the Driver Optimizer offers to install the driver it kept again, instead of saying there's no NVIDIA card.
- The clean installation option now warns that the PC is without a driver for longer.

## 0.28.0: One NVIDIA gaming profile, no telemetry
- **NVIDIA gaming profile (GPU > Settings):** one profile in the driver's global settings that covers every game, instead of settings per game:
  - Prefer maximum performance;
  - Low Latency Mode On;
  - a 10 GB shader cache (a bigger one you set is kept);
  - on RTX 40 and 50 cards, the latest DLSS model, the same setting as the NVIDIA App's DLSS Override. RTX 20 and 30 cards keep their DLSS setting, since the newest model costs FPS there.

  It replaces the "Bigger NVIDIA shader cache" setting and keeps its backups, so Revert still restores your earlier values. The trade-off is said plainly: the card may stay at higher clocks on the desktop too. Profiles and Set up this PC leave it out on laptops.
- **Per-game NVIDIA switches from 0.26 are gone from game pages.** One that's still on is shown until you turn it off; Restore everything still puts them back.
- **Block NVIDIA telemetry (GPU > Driver Optimizer):** the display driver itself carries NVIDIA's telemetry (NvTelemetry inside the NVIDIA Display Container, which reports to `events.telemetry.data.nvidia.com`, `feedbacks.telemetry.data.nvidia.com` and `activation.gfe.nvidia.com`, as its own log shows). It's part of the signed driver, so no package-level strip can remove it. Kestrel blocks those three hosts in the Windows hosts file instead:
  - precise: game-profile updates and driver downloads keep working;
  - reversible: switch it off, or use Restore everything;
  - it survives driver updates.

  The row shows live whether the servers can still be reached and which internet connections NVIDIA's display container has right now.
- **The Driver Optimizer finishes the job after installing:** it blocks telemetry and applies the gaming profile (both on by default; the profile starts off on laptops), and puts Kestrel's NVIDIA settings back if a clean installation reset them.
- **Rollback:** the stripped driver you install is kept, along with the one installed before it, so "Reinstall" goes back to the previous driver with one click. Older copies are deleted automatically, and Delete removes the kept ones.

## 0.27.0: Driver Optimizer
**GPU > Driver Optimizer** installs NVIDIA's driver without the NVIDIA App, its background services, overlay, recorder and telemetry, and keeps every gaming feature. It's built into Kestrel: no NVCleanstall, no 7-Zip, nothing else to install.
- **Get a driver:** Kestrel finds your card on NVIDIA's own driver lookup and lists recent Game Ready drivers, or Studio drivers, with date and size. Download one (with progress and Cancel), or import a driver file you already have (Import, or drop the .exe on the window). Every file must carry NVIDIA's valid digital signature. Kestrel says plainly that the newest driver isn't always the best one.
- **Read from the driver itself:** Kestrel unpacks the package with Windows' own tar and reads NVIDIA's component manifests, including what each component requires. Dependencies come from NVIDIA's own declarations, so newer drivers work without a Kestrel update. Components Kestrel doesn't know yet are kept. The package's device list confirms your card is supported before anything installs.
- **Presets:**
  - **Optimized Gaming** (default) leaves out the NVIDIA App, its container services, message bus and backend, ShadowPlay (overlay and recording), NVIDIA Virtual Audio, FrameView SDK and NVIDIA telemetry.
  - **Competitive** also leaves out the deep-learning scaling model (DLDSR). It's honest that nothing more runs in the background than with Optimized Gaming.
  - **Minimal** also leaves out PhysX and HDMI/DisplayPort audio.
  - **Custom** lets you switch each component, with requirements kept automatically.
  - Kept in every preset: the driver itself (NVIDIA Control Panel, DirectX, Vulkan, OpenGL, DLSS with Frame Generation and Ray Reconstruction, Reflex, Low Latency Mode, G-SYNC, NVENC/NVDEC, shader cache, per-game profiles), the Visual C++ runtime, and the hardware-specific USB-C and laptop power (NVPCF) drivers, which install only where that hardware exists.
  - Each component shows its size, what it runs in the background, and what you lose without it. On driver 621.40, Optimized Gaming cuts the package from 3.9 GB to 3.3 GB.
- **Nothing signed is changed:** left-out components are whole folders. setup.cfg loses only the three file references that pointed into the NVIDIA App folder (EULA, privacy policy, consent text), as NVCleanstall does. The driver's INF, catalog and binaries stay as NVIDIA signed them, so there are no test-signing or Secure Boot workarounds and anti-cheat sees a normal driver.
- **Install or export:** install runs NVIDIA's own installer silently, with an optional clean installation (it resets Control Panel settings, so it's off by default). NVIDIA's OpenGL driver closes OpenGL programs such as Kestrel while the display driver restarts, so the installer runs from a small script that records the result and reopens Kestrel on the Driver Optimizer. Export saves the stripped package to a folder of your choice. "Delete Kestrel's temporary driver files" removes the download and unpacked files.
- **After installing**, Kestrel checks and shows only measured results:
  - the driver version and the driver file's signature;
  - PhysX;
  - telemetry services and tasks still present by name;
  - NVIDIA services, scheduled tasks, processes, their memory and disk use, before and after.
  - It also says the display driver's own container still reports some usage data. That's part of NVIDIA's signed driver and can't be removed without breaking the signature.
- No FPS claims: leaving out the App removes background programs, not frame-time magic. Kestrel's only network traffic here is NVIDIA's driver lookup and download.
- Tested end to end on an RTX 2070 SUPER with driver 621.40: lookup, signature check, unpacking, Optimized Gaming build, silent install, automatic reopen and verification.

## 0.26.0: NVIDIA per-game settings
- **Two NVIDIA switches on each game's page (Games > the game > Game Mode):**
  - **Prefer maximum performance:** NVIDIA's Power management mode for just this game, so light or processor-heavy games don't stutter when the card clocks down. A small effect, and the card uses more power while you play; per game, so it doesn't stay at full speed on the desktop as the global setting would.
  - **Low latency mode:** Low Latency Mode = On (at most one frame prepared ahead), which cuts input lag when the graphics card is the limit. DirectX 11 and older games only; for games with NVIDIA Reflex, Reflex is the better choice.
  - Written to the game's own NVIDIA driver profile, the same one the NVIDIA Control Panel shows under Program settings (NVIDIA's profile for the game when it has one; otherwise Kestrel creates one). The value before is saved first; turning a switch off, or Settings > Restore everything, puts it back and removes a profile Kestrel created.
- **Not added, with reasons:** Low Latency Mode "Ultra" (NVIDIA doesn't publish its setting), texture filtering "High performance" (no measurable gain on current cards), threaded optimization (OpenGL only), and forced V-Sync or frame limits (they depend on your G-SYNC setup and preference).
- The GPU page points to the per-game settings.
- **Switches instead of tick boxes:** every setting on Windows, GPU, Network, Controller & input, Privacy and Services, and the per-game switches, is now an On/Off switch that takes effect when you flip it, instead of ticking settings and pressing Apply. Turning a switch off puts back the value saved before Kestrel changed it (what the Revert button did). A setting that was already on before Kestrel shows On and can only be changed in Windows, since Kestrel doesn't know its earlier value. A message appears only when something fails or needs a restart. Lists you pick from before confirming (Set up this PC, profiles, app updates, clean test, files to delete) keep tick boxes.

## 0.25.0: New layout and network test
A flat sidebar with one page per area, modeled on HD OPTI's layout, and a connection test.
- **Sidebar:** Home, Game Mode, Windows, GPU, Network, Controller & input, Games, Clean up & health, Apps and Tools, with Settings at the bottom. The menu button next to the logo collapses it to icons (names show on hover).
- **Game Mode switch in the sidebar:** always at hand, with what it's doing (on for a game, waiting for your games, or off). Game Mode also has its own page now instead of a tab under Games.
- **Status bar:** Kestrel's version, what's going on (Clean test on, Game Mode on, profile applied, or ready) and a Report a problem link, above every page.
- **GPU page:** GPU scheduling, the NVIDIA shader cache and the MPO fix, with the graphics findings (driver, Resizable BAR, refresh rate, VRR and Auto HDR, overlays).
- **Network page:** a connection test that pings your router and Cloudflare's 1.1.1.1 25 times each (about 4 seconds) and reports ping, jitter and packet loss, and says whether a problem is inside your home network (Wi-Fi, cable) or beyond your router (your provider). Read-only. Also the update-sharing setting and the connection finding.
- **Controller & input page:** mouse acceleration and USB selective suspend.
- **Tools page:** System analysis and Clean test (moved from Windows and Home).
- **Clean up & health:** a Health tab with everything Kestrel found on this PC.
- Settings moved to GPU, Network and Controller & input are the same settings with the same backups and Revert; profiles still include them.

## 0.24.0: Resizable BAR and MPO
From a comparison with WinUtil, Win11Debloat, Sophia Script and Optimizer: the two performance-related additions with real evidence behind them.
- **Resizable BAR check (Home > Status):** tells you when Resizable BAR (AMD: Smart Access Memory) is off on a graphics card that supports it (NVIDIA RTX 30 and newer, AMD RX 6000 and newer, Intel Arc), and how to turn it on in the BIOS. Reviews measured about 1 to 3 percent more FPS on average and much better lows in some games. Detection reads the capability Microsoft's DirectX runtime reports (GPU upload heaps need Resizable BAR on a dedicated card); older cards that can't use it get no warning, and nothing is reported when the driver doesn't say.
- **Turn off multi-plane overlay (Windows > Performance > Individual settings, Advanced):** NVIDIA's own fix for flicker, black flashes or stutter with G-SYNC, FreeSync or several monitors (`Dwm\OverlayTestMode` = 5). A fix for that problem, not a speed-up; reports differ on whether Windows 11 24H2 and later still honor it. Saved first, Revert puts it back, needs a restart.
- **Not added, with reasons:** the Ultimate Performance plan (on a plugged-in PC it differs from High performance only in never turning off hard disks), network adapter power and interrupt settings (no solid gaming measurements; interrupt moderation off raises CPU use), "fastest DNS" (doesn't change in-game ping), turning off IPv6 or Teredo (Teredo is used by Xbox party chat), and visual effects for best performance (no FPS effect).

## 0.23.1: Protected tasks
- **Extreme no longer offers the Windows AI scheduled tasks** (Recall, Click to Do). Windows lets only itself change them, even for administrators (xOS gets around this by running as TrustedInstaller). Removing Recall still turns Recall off.
- **Clearer error:** a task Windows protects now says so, instead of asking you to restart as administrator when you already are.
- **No leftover backups after a failed change:** when a scheduled task can't be changed, the original Kestrel saved a moment before is removed again, and Restore everything and Undo profile skip tasks that are already as they were, instead of failing on them every time.

## 0.23.0: Profiles and System analysis
Built from a teardown of KernelOS, xOS, AtlasOS and ReviOS: optimization profiles up to an Extreme level close to custom gaming builds, and a System analysis that shows what Windows is running, what changed, and what other optimizers already did.
- **Profiles (Windows > Performance > Profile):** Balanced, Competitive and Extreme, each building on the one before, across Windows settings, services and scheduled tasks. Kestrel checks this PC first: what's already on is shown as On and skipped, settings that cost battery life are left out on PCs with a battery, and memory integrity stays on when FACEIT or Riot Vanguard is installed. Apply shows the full list first; **Undo profile** (or Settings > Restore everything) puts it all back.
  - **Balanced:** Game Mode, background recording off, windowed-game optimizations, the recommended services, and AtlasOS's five telemetry tasks.
  - **Competitive:** adds Best performance power mode, GPU scheduling, no update sharing, service grouping, the NVIDIA shader cache, USB selective suspend off (desktops), the optional and app services, and 16 more telemetry and diagnostic tasks from xOS's list (update, backup, disk-integrity and memory-diagnostic tasks are left alone).
  - **Extreme:** adds animations and transparency off and the High performance plan (desktops), plus choices that start unticked and are spelled out before you confirm: memory integrity off, automatic updates off (Microsoft's "Configure Automatic Updates" policy), Search indexing off, removing Recall (its Windows feature, reversible), and removing Widgets, Phone Link, Copilot and Teams (reinstall from the Microsoft Store).
  - **Not offered:** turning Defender off. Windows only lets another antivirus do that; KernelOS and xOS force it with TrustedInstaller workarounds Microsoft doesn't support. CPU vulnerability protections also stay on (AtlasOS notes turning them off slows some recent CPUs).
- The old Balanced/Performance/Custom preset row is replaced by the profile; individual settings stay below it.
- **System analysis (Windows > System analysis):** processes, service hosts (svchost), running services, threads, handles, memory in use, background CPU, startup apps and scheduled tasks, against a saved baseline (your first measurement is kept automatically; Save as baseline replaces it). It tells genuine reductions apart from service grouping: fewer svchost processes with the same services running is the same work in fewer processes.
- **Found on this PC:** recognizes AtlasOS and xOS playbooks, service grouping (by threshold or service by service), Microsoft Defender not running, memory integrity off (with what FACEIT and Riot Vanguard need), Windows Update turned off by policy, CPU vulnerability protections turned off, and a disabled telemetry service.
- **Group Windows services into fewer processes (Windows > Performance > Advanced, opt-in):** raises the svchost split threshold above this PC's memory, as AtlasOS and xOS do. Labeled honestly: fewer processes and a little less memory, the same work, no FPS gain; a crashing service stops the others in its process. Saved first, Revert puts it back, needs a restart. AtlasOS's service-by-service grouping counts as already on.
- **Services:** Diagnostics Hub collector and Program Compatibility Assistant (recommended), and troubleshooters/problem detection, Problem Reports and Inventory and Compatibility Appraisal (optional), each with what you lose.
- **Not added, on purpose:** `Win32PrioritySeparation` 0x26 and `NetworkThrottlingIndex` 10 equal Windows' own defaults; xOS's svchost threshold of 3670016 is also the default; `SystemResponsiveness`, `DisablePagingExecutive`, `DisablePageCombining` and `QueuedPresentLimit` were reverted by KernelOS itself; global timer resolution has been per-process since Windows 10 2004.
- The memory integrity check no longer says Kestrel never turns security features off: Set up this PC for gaming offers it as an opt-in.

## 0.22.0: Clean test
- **Clean test (Home > This PC, or "Rule out other software" on a game's Measure tab):** a guided, temporary version of Microsoft's clean boot, to find out whether other software is what slows your games. Kestrel lists the other programs' services that start with Windows and your startup apps, you untick anything you want kept, and **Start test** turns them off and offers a restart. Kestrel opens by itself after the restart with a banner on every page; measure your game (Measure compares it with your earlier run), then **End test** puts everything back. Settings > Restore everything also ends a test.
- **What stays on:** Windows' own services and drivers are never touched (told apart by the program's company, as msconfig does), and neither are anti-cheat, game launchers' services, security software, or hardware software for graphics, audio, network, Bluetooth, controllers and input devices. Programs Kestrel can't identify are left alone too.
- **Honest result when there's nothing to turn off:** if nothing else starts with Windows, the dialog says so: other software isn't what slows your games, and a driver may be (LatencyMon shows which).
- The test keeps its own record of what it changed, so ending it never undoes choices you made on the Services or Startup apps tabs.
- **Tray fixes:** clicking Kestrel's tray icon now brings the window to the front with focus, so the tray flyout closes and your next click works (before, the first click only activated the window). Starting with Windows with the tray on now really starts in the tray, without a taskbar button. The Settings text says where to find the icon: Windows 11 may hide it under ^ in the taskbar corner.

## 0.21.0: Know more
- **More games with menu-step guides** (Game settings tab of a game's page; no files are edited): **Valorant** (Riot's own list: Improve Clarity, Experimental Sharpening, Bloom, Distortion and Cast Shadows off, the four quality settings lowered gradually), **Apex Legends** (EA's own values: V-Sync off unless you see tearing, Ambient Occlusion and Volumetric Lighting off, Texture Streaming Budget Low or Medium, Model Detail Low, Bilinear filtering) and **Minecraft** (Graphics Fast or Fancy, Render and Simulation Distance). Steps use only what the game's publisher documents; Counter-Strike 2 is left out because Valve publishes no performance guidance and the third-party guides disagree.
- **Hard drive warning** on a game's Measure tab when the game is installed on a hard drive instead of an SSD.
- **Updates on Home:** how many app and driver updates the last check found, with a View button. Kestrel still checks only when you ask.
- **Copy diagnostics** (Settings > About this PC) copies Kestrel's version, the Windows build, the PC's details and your latest Kestrel activity, to paste into a bug report; **Report a problem** opens GitHub issues. Nothing is sent from Kestrel.
- **Start with Windows (opt-in, Settings > Window):** off by default. When you turn it on, Kestrel starts minimized in the notification area when you sign in, so automatic Game Mode works without opening it first. It adds a per-user Run entry (the kind Task Manager > Startup apps lists; no administrator rights) and turns the tray on; turning it off deletes the entry. Honest cost: Kestrel then stays running, using about 90 MB of memory and no CPU while it waits.
- **Startup apps show what they cost now:** each running startup app shows the memory it uses right now, and you can sort by it. Windows' own per-app startup timings (what Task Manager's "Startup impact" shows) are in administrator-only files with an undocumented format, so Kestrel doesn't guess from them.

## 0.20.0: Simpler layout
Kestrel's nine pages are now five sections plus Settings, after comparing how Microsoft PC Manager, the NVIDIA app and Razer Cortex are organized.
- **Home:** Set up this PC for gaming, **Your games** (most recently measured first, with the last result, Play and Open), **This PC** (Clean up, Game Mode, how many Windows settings Kestrel changed) and status.
- **Games:** your game library and Game Mode. **Open** a game for its own page: Play on top, then three tabs: **Measure** (result, what limits it, fixes, earlier results), **Game settings** (presets) and **Game Mode** (its profile, or Add, plus Windows' per-game switches). Before, this was spread over Gaming > Per-game, Gaming > Measure and the Game Mode page.
- **Windows:** Performance (the Windows settings), Startup apps, **Services (new)** and Privacy, as tabs of one page.
- **Services (new):** a short, researched list of services a gaming PC doesn't need, from Microsoft's own "OK to disable" guidance: telemetry, device-management messages, offline maps, location, Phone Link calls, retail demo, fax, link tracking, mobile hotspot, payments/NFC, Windows Insider (unless you're an Insider) and printing (only when no real printer is installed), plus optional SysMain and other apps' updaters and remote-access tools (set to Manual). Each row says what you lose. Services Windows, games, anti-cheat, Xbox, controllers or sound need are never listed and can't be changed. The original is saved first, the service stops right away, and Revert or Settings > Restore everything starts it again. Honest note: this frees a little memory and background activity; it doesn't add FPS by itself.
- **Clean up:** unchanged (Junk files, Large files, Disk usage).
- **Apps:** Installed, Get apps and Updates (apps, drivers, Windows Update) as tabs of one page.
- Restart as administrator from a game's page comes back to that game; texts that pointed to old page names now point to the new ones; one Play button per game page instead of two.
- **Less text:** shorter page subtitles and captions throughout; each Windows setting's trade-off moved into its hover text.

## 0.19.0: Game settings
- **Game settings (Gaming > Per-game > Game settings):** three presets for a game's own graphics settings: **Quality** (trims only the most expensive effects), **Balanced** (lowers what costs a lot of FPS for little visible difference) and **Performance** (as many FPS as possible). You see every change first. Presets only lower settings that are above their target; nothing is ever raised. The game must be closed, the original settings file is saved first, and **Undo** puts it back.
  - **Unreal Engine games** are supported automatically: Kestrel finds the game's GameUserSettings.ini and adjusts its quality groups (shadows, global illumination, reflections, effects, post-processing, foliage, shading, view distance, anti-aliasing) using the levels Epic documents. Textures are left alone; they cost memory, not FPS.
  - **Red Dead Redemption 2:** volumetrics, soft shadows, trees, particles, ambient occlusion, tessellation, reflections, shadows, parallax occlusion, water physics and motion blur, written only where the file's values name themselves (or, for water physics, are the menu slider's position). Plus a reminder to turn on DLSS, which usually adds the most.
  - **Overwatch and Call of Duty:** their settings files aren't documented, so Kestrel shows the exact menu steps for each preset instead.
- **Measure** offers "Open game settings" when the graphics card is the limit and Kestrel has presets for the game.

## 0.18.0: Find the bottleneck
- **Measure says what limits the game,** in plain words: the graphics card (busy nearly the whole frame), the processor and what runs next to the game (the graphics card waits part of each frame), or a frame limit (held at, say, 165 FPS by V-Sync or a limiter). It uses PresentMon's GPU-busy time, the measure Intel designed for this.
- **Advice matched to the bottleneck,** most helpful first. Graphics card: turn on DLSS (RTX cards) or FSR/XeSS, lower the heaviest settings, with a link to the NVIDIA app or AMD Software. Processor: the settings that load the processor (crowds, view distance), memory integrity, background apps. Frame limit: how to raise or remove it. Suggestions are also shown when a game already runs well, under "Want even more FPS?".
- **New checks for setup mistakes** (Overview > Status, the gaming setup and Measure):
  - **Screen plugged into the motherboard** while a graphics card is installed: the most common reason a new PC runs games badly.
  - **Memory in single channel:** one stick, or all sticks on the same channel (read from the memory's own labels; only reported when certain).
  - **Laptop on battery or Energy saver.**
  - **Ryzen X3D with two core groups** (7900X3D, 7950X3D, 9900X3D, 9950X3D): checks that Xbox Game Bar and the AMD chipset driver are installed, which Windows needs to put games on the cache cores.
- **NVIDIA shader cache (new, reversible):** raises the driver's shader cache to 10 GB, so the driver doesn't throw away compiled shaders and games don't stutter rebuilding them. A bigger size you already set is kept. Uses NVIDIA's driver settings interface (nvapi64.dll, part of every NVIDIA driver); in the gaming setup and Measure's fixes on PCs with NVIDIA graphics.

## 0.17.0: Set up this PC for gaming
Optimize PC is now **Set up this PC for gaming** (Overview > Start): one scan finds everything on this PC that slows games down, shows it as one list in plain words, and applies it with one click. Everything can be undone.
- **Windows settings:** the recommended ones as before, plus two new ones (as administrator): **hardware-accelerated GPU scheduling** (needed for frame generation; after a restart) and **stop sharing updates with other PCs** (frees your upload for online games; a Delivery Optimization policy already set on the PC counts as done).
- **Apps that don't need to start with Windows:** common background apps such as Spotify, Discord, Teams and the Edge preloader are turned off at startup with the same reversible switch as the Startup apps page. Game launchers and OneDrive are listed unticked. Device software (mouse, headset, RGB) and security software are never listed.
- **Game Mode for your games:** adds your detected games to Game Mode with the Performance profile and turns on automatic Game Mode, so background apps are quieted only while you play.
- **Optional, unticked:** remove preinstalled extras (Clipchamp, News, Solitaire…; Xbox apps and the Store are kept). It frees space, not FPS, and says so.
- **Optional, unticked, behind a warning: turn off memory integrity.** Independent tests measured about 4 to 10 percent more FPS in processor-limited games, but it lowers protection against malicious drivers and some anti-cheat systems require it. Never automatic or part of a preset; needs a restart; reversible.
- After applying, Kestrel offers **Restart now** when a change needs it, and suggests measuring a game to see the difference.
- **Measure:** "Kestrel can fix" now also covers GPU scheduling, apps that start with Windows, and Game Mode for the measured game. Memory integrity is shown as information with a button to Windows Security. **Undo these fixes** reverts exactly what was applied.
- **Gaming > Windows settings** ends with "What Kestrel doesn't do, and why": debloating, turning off services or Defender, timer and boot tweaks, RAM cleaners, ping tweaks and deleting shader caches, each with the reason.

## 0.16.0: Proof
- **Measure (new tab on Gaming):** start your game (Kestrel notices it, or press Play), get into gameplay and click Measure. After 60 seconds Kestrel says in plain words how smooth the game runs, from average FPS, 1% lows (the slowest moments) and stutter, and compares with the last time. It uses Intel PresentMon, the free, open-source frame-time tool reviewers use; Kestrel installs it through winget when you first measure, then starts measuring by itself.
- **Fixes after measuring:** when the result is low or uneven, Kestrel lists what would help. The fixes it can make itself (Windows Game Mode, background recording, optimizations for windowed games, the Best performance power mode, the screen's full refresh rate, and the fast graphics card for this game on PCs with two) are applied together with "Apply fixes", each one reversible. The rest (an old graphics driver, slow memory settings, a game on a hard drive, overlays, too little free memory) are explained, with a button to the right place where there is one. Measure again to see the difference.
- **Restart as administrator** brings you back to the page you were on (Measure, Updates or Settings), instead of the Overview.
- **Play (Gaming > Per-game and Measure):** turns on Game Mode with the game's profile (from your Game Mode list, or the manual profile), then starts the game. Game Mode ends by itself when the game closes, also when automatic Game Mode is off. Games are started through Explorer, so they never inherit Kestrel's administrator rights.
- **Get apps as administrator:** installs are now done for all users when Kestrel runs as administrator, because winget refuses per-user installs from an administrator session (this blocked installing PresentMon). Apps that can only be installed per user say to open Kestrel normally.
- **Disk usage (Cleanup):** pick a drive to see its largest folders and files, open a folder to see what's inside, and show any of them in File Explorer. Read-only. Links are never followed, so nothing is counted twice.

## 0.15.0: Control
- **Privacy (new page):** seven switches for Windows features that share your data or show promotions: the advertising ID, tailored experiences, your language list for websites, tips and suggestions (in Settings, while using Windows, setup prompts and lock-screen tips), Start recommendations, inking and typing personalization, and online speech recognition. They're the same per-user values as the switches in Settings, so no administrator rights are needed. Each original value is saved first; Revert and Restore everything put it back. Privacy settings are never part of a preset or Optimize PC.
- **Get apps (Apps page):** search the winget catalog, or pick from popular gaming and everyday apps (Steam, Epic, EA app, Ubisoft Connect, Battle.net, GOG GALAXY, Discord, OBS Studio, and more). Install one or tick several; they install one after another with one confirmation, and winget checks each installer's hash.
- **App updates:** Skip hides one version of an app (a newer one shows again; "Show again" brings skipped ones back). When Kestrel runs as administrator, a note explains that some installers refuse to run that way.
- Fixed: failed app updates showed a garbled symbol instead of ✖ in their summary.

## 0.14.0: Health and space
Based on new research into all-in-one tools (see ROADMAP.md).
- **Drive health (new status check):** each drive's own health report, without administrator rights. NVMe drives show temperature, how much of their rated life is used, hours powered on, and any critical warning or data errors (the same data as Settings > Storage > Disks & volumes); SATA drives show their built-in failure prediction. A drive in trouble is flagged with a reminder to back up.
- **Deep cleanup (Cleanup, administrator):** two new categories, both off by default and both using Windows' own tools:
  - **Previous Windows installation:** Windows.old, often 10–30 GB after a feature update, removed through Disk Cleanup's own handler. After this you can't go back to the previous Windows version.
  - **Old Windows update components:** superseded components, cleaned with DISM's component cleanup; the space freed is measured afterwards. Installed updates can no longer be uninstalled.
- **Pause Windows Update (Updates page, administrator):** pause for 1, 2 or 5 weeks before a match or a busy week, and resume when you like. It's the same pause as Settings > Windows Update, so it shows there and can be resumed there too. Kestrel never disables updates.
- **About this PC (Settings):** Windows version and build, processor, memory and its speed, graphics and driver, displays and refresh rates, and drives, with Copy for support forums.
- **Fixed: the tray icon ignored clicks when Kestrel ran as administrator.** Windows blocks messages from the taskbar to an elevated app unless the app allows them, so Open and Exit did nothing and Kestrel could only be ended in Task Manager.

## 0.13.0: App and driver updates
- **Updates (new page):** keep your apps and drivers current from one place. Nothing is checked until you click, and nothing installs without a confirmation.
  - **Apps:** lists installed apps with a newer version in the winget catalog (Windows' own package manager), with the installed and available version. Update one, tick several, or Update all; they update silently one after another, with progress and "Cancel the rest", and one summary at the end. winget checks each installer's hash; installers that need administrator rights get Windows' normal prompt. A link opens the Microsoft Store's updates for Store apps.
  - **Drivers:** asks Windows Update for driver updates matched to this PC's hardware (certified by Microsoft; always Microsoft's servers, also on PCs that use a company update server). Install one or several as administrator: Kestrel creates a restore point first, accepts the driver's license on your confirmation, and reports each result and whether Windows needs a restart. It never restarts for you. Rollback: Device Manager's Roll Back Driver or System Restore.
  - **Graphics driver:** your GPU's driver version and age, with a link to NVIDIA, AMD or Intel's official driver page. GPU makers offer no update API, and Kestrel doesn't use third-party driver sources.
- Kestrel's own updates stay in Settings > Updates.
- **New logo:** a kestrel's head with its dark "tear mark" stripe under the eye. It's the icon of kestrel.exe itself (File Explorer, Downloads, shortcuts), the window, taskbar and tray; the sidebar shows the bird on its own, larger. It's drawn at each size it's shown, with the eye, stripe and beak slightly thickened at 16 and 24 px so they stay visible.
- Fixed: the tray icon could read past the end of its mask at 150% display scaling.

## 0.12.2: Banner fix
- **Banners and dialog warnings** no longer fill the whole page. 0.12.0's fix for text running under the buttons made banners (like "Update available") and the warning box in confirmation dialogs stretch to all the space they were given. They're back to the height of their text, and the test now also checks their height.

## 0.12.1: Fixes from the 0.12 audit
- **Laptop power mode:** Windows keeps one power mode for plugged-in and one for battery. Kestrel now remembers which one it changed, and if the PC was unplugged (or plugged in) since, it waits until it's back on that power source instead of overwriting the other one. Values saved by older versions restore as before.
- **Safer saves:** your Game Mode setup (games, background apps) and update settings are written atomically, like Kestrel's backups, so a crash or power cut mid-save can't wipe them.
- **Update checks while Kestrel stays open:** the daily check now also runs when Kestrel has been running for a day (for example in the tray), not only at launch. A failed check is retried at most hourly.
- **Activity log:** trimmed to its newest 2,000 lines once it passes 1 MB, instead of growing forever.
- **Empty background-app list:** Game Mode skips efficiency mode and closing apps with a clear note when the list is empty, and the Game Mode page says so.
- **Overlay check:** now also detects Xbox Game Bar and the Steam overlay.

## 0.12.0: Interface polish
- **Interface polish** (same structure, refined throughout; see DESIGN.md):
  - **Overview:** Optimize PC is now the one primary panel; Clean up and Game Mode are compact Quick actions, and Clean up shows the space found once you've scanned; Status leads with a summary line ("All 11 checks look fine") with its dot.
  - **Sidebar:** the Kestrel mark next to the name, pages in three quiet groups, and the current page in semibold.
  - **Compact, aligned rows:** rows take their text's natural height (two-line rows are about 20 px shorter); on Gaming, each setting's state and Revert sit on its title line; table text centers on its buttons.
  - **Controls:** one 32 px height for buttons, drop-downs and search boxes; softer filled buttons, a semibold primary button, hairline borders, quieter section labels, and 8 px corners on lists.
  - **Cleanup:** before the first scan, Scan sits inside the empty panel.
  - The scrollbar no longer touches the lists' right edge.
- **Banners:** text no longer runs under the buttons. Banners with several buttons (like "Update available") reserved a fixed 200 px for them; the text now gets exactly the room the buttons leave and wraps. A test renders the update banner off-screen to keep it that way.
- Settings > About links to the license and the third-party notices, which are now built into the app and attached to each release.
- **License:** Kestrel is now source-available under the PolyForm Noncommercial License 1.0.0 (free for personal and other noncommercial use). Security reports: see SECURITY.md.

## 0.11.0: Background apps, done reversibly
Based on the competitor research in [COMPETITORS.md](COMPETITORS.md).
- **Efficiency mode for your background apps (Game Mode):** a new action puts the apps in your list on Windows 11's power-saving scheduling (EcoQoS, the basis of Task Manager's Efficiency mode) and below-normal priority while you play. They keep running, nothing is closed, and the original priority comes back when Game Mode ends (or at the next start after a crash). It replaces closing apps in the Performance profile; closing is still available under Custom.
- **Memory use in the app picker:** Game Mode's "Background apps" list shows open apps sorted by memory, with sizes, so the heavy ones are easy to spot.
- Researched and rejected: timer resolution changes, killing processes and "RAM cleaning", stopping services, automatic debloating, editing game config files, and local "ping" tweaks. See COMPETITORS.md for the evidence.

## 0.10.0: Windows 11 performance and signed updates
**Windows 11 only from now on.** On older Windows, Kestrel says so at start.

Why users noticed little before: most of what presets changed is already Windows 11's default (Game Mode on, "Record what happened" off, windowed-game optimizations on recent builds), transparency and animation changes are cosmetic, and Game Mode's Balanced profile repeated those defaults. The changes that matter are the ones that fix a real bottleneck on a particular PC. So this release focuses on finding those and reporting honestly.
- **Best performance power mode (new):** Windows 11's own power mode (Settings > System > Power), for the power source you're on now. It's the supported way to stop Windows from holding back CPU boost, and it also works on Modern Standby laptops that have no High performance plan. It's now in the Performance preset and in Game Mode's Balanced and Performance profiles, instead of the High performance plan (still available under Custom). When the active plan isn't Balanced, power modes don't apply, and Kestrel says "Not needed" instead of changing anything.
- **Memory in use (new check):** how full memory is and which open apps use the most, so you know what to close before playing. A single reading when checks run, not monitoring. Games in your Game Mode list are never opened.
- **Honest results:** settings that are already right or wouldn't do anything are skipped and reported as "Already set" or "Not needed", not re-applied. Optimize PC's summary groups Changed, Failed, Already set and Not needed, and says when a change takes effect. No scores or FPS claims.
- **Updates (opt-in):** on first launch Kestrel asks once whether to check for updates. When it's on, Kestrel checks GitHub Releases at most once a day after starting, shows a banner (Update now, Release notes, Remind me later), downloads with progress, and installs only after you confirm the restart. Every update must carry a signature from the Kestrel release key: the signature covers the version and the exe's SHA-256, and is checked with Windows' own cryptography before anything is saved. The new exe replaces the old one by renaming (the running exe is never overwritten, and nothing is elevated); the old one is kept until the new version starts. Settings > Updates has the switch, Check now, the installed version, last check, release notes and the channel (Stable).
- **Release tooling:** `kestrel-sign` (a separate tool, not part of kestrel.exe) and `scripts/release.ps1`, which builds, tests, signs and prepares `kestrel.exe` + `kestrel-update.sig` for a release.
- No new dependencies: HTTPS uses WinHTTP and signatures use Windows CNG.

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
