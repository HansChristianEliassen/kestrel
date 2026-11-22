#[cfg(debug_assertions)]
pub(crate) mod devshots;
pub mod state;

use eframe::egui;

use crate::cleaner::{clean_items, scan_all};
use crate::gamemode;
use crate::optimizer::{games, gaming, health, Preset, Tweak};
use crate::system::{create_restore_point, is_admin, windows_build};
use crate::ui::components::{accent_button, button, confirm, dialog, dialog_buttons, dialog_title, secondary, toasts, Toast, BG, SIDEBAR};
use crate::utils::log;
use crate::utils::logging::read_history;
use crate::{startup, uninstaller, update, upgrade};
use state::*;

pub struct App {
    pub(crate) page: Page,
    pub(crate) admin: bool,
    pub(crate) build: u32,
    pub(crate) confirm: Option<Confirm>,
    pub(crate) toasts: Vec<Toast>,
    /// The Overview's status checks have been requested once.
    pub(crate) overview_requested: bool,
    /// The Overview also lists status checks that need no attention.
    pub(crate) show_ok_checks: bool,
    pub(crate) cleaner: CleanerState,
    pub(crate) apps: AppsState,
    pub(crate) upgrades: UpgradeState,
    pub(crate) startup: StartupState,
    pub(crate) gaming: GamingState,
    pub(crate) opt: OptState,
    pub(crate) gamemode: GameModeState,
    pub(crate) history: String,
    /// Settings > About this PC, read when Settings opens.
    pub(crate) about: Vec<(&'static str, String)>,
    /// Notification-area icon, present only while "Keep watching after closing the window" is on.
    tray: Option<crate::system::tray::Tray>,
    /// Exit was chosen from the tray menu, so the next close request really closes.
    exiting: bool,
    /// The window was closed to the tray: minimized, without a taskbar button.
    tucked: bool,
    tucked_seen: bool,
    pub(crate) updates: UpdateState,
    /// When the last automatic update check started (this run).
    update_tried: Option<std::time::Instant>,
    /// Start this exe (the freshly installed update) once Kestrel has closed.
    relaunch: Option<std::path::PathBuf>,
    #[cfg(debug_assertions)]
    shots: Option<devshots::Shots>,
    /// Page to open on the first frame, after a "Restart as administrator" from that page.
    start_page: Option<String>,
    /// The app icon as a texture, for the sidebar (made on first draw).
    pub(crate) brand: Option<egui::TextureHandle>,
}

impl App {
    pub fn new() -> Self {
        let build = windows_build();
        let mut app = App {
            page: Page::Dashboard,
            admin: is_admin(),
            build,
            confirm: None,
            toasts: Vec::new(),
            overview_requested: false,
            show_ok_checks: false,
            cleaner: CleanerState::default(),
            apps: AppsState::default(),
            upgrades: UpgradeState::default(),
            startup: StartupState::default(),
            gaming: GamingState { selected: preset_selection(Preset::Balanced, build), ..Default::default() },
            opt: OptState::default(),
            gamemode: GameModeState::default(),
            history: String::new(),
            about: Vec::new(),
            tray: None,
            exiting: false,
            tucked: false,
            tucked_seen: false,
            updates: UpdateState::default(),
            update_tried: None,
            relaunch: None,
            #[cfg(debug_assertions)]
            shots: devshots::Shots::from_env(),
            brand: None,
            start_page: std::env::args().find_map(|a| a.strip_prefix(crate::system::PAGE_ARG).map(String::from)),
        };
        // This launch is the new version (or the update was never installed): the previous exe is no longer needed.
        update::clean_up();
        if build < 22000 {
            app.show_error(
                "Windows 11 required",
                "Kestrel supports Windows 11 only. On this version of Windows some settings won't exist or won't do anything.",
            );
        }
        // Debug screenshot runs sit next to a real Kestrel: they must not touch its Game Mode session or go online.
        #[cfg(debug_assertions)]
        if app.shots.is_some() {
            app.gamemode.config.auto = false;
            app.updates.prefs.enabled = false;
            return app;
        }
        // Game Mode was still on when Kestrel last stopped (crash, power cut): put everything back now.
        if gamemode::is_active() {
            let (lines, ok) = gamemode::restore();
            let title = if ok { "Game Mode settings restored" } else { "Game Mode settings partly restored" };
            app.show_notice(title, format!("Game Mode was still on when Kestrel last closed unexpectedly.\n\n{}", lines.join("\n")));
        }
        app
    }

    pub(crate) fn game_mode_on(&mut self, ctx: &egui::Context, profile: gamemode::Profile, game: Option<String>) {
        let gm = &mut self.gamemode;
        let actions = profile.actions(&gm.config);
        let close = gm.config.close.clone();
        let trigger = game.clone().unwrap_or_else(|| "turned on by hand".into());
        // Turned on automatically, the game is already running; `play` clears this until the game has started.
        gm.on = Some(GameModeOn { profile, seen: game.is_some(), game });
        gm.job.start(ctx, move || {
            let lines = gamemode::activate(profile, &actions, &close, &trigger);
            (lines, gamemode::is_active())
        });
    }

    pub(crate) fn game_mode_off(&mut self, ctx: &egui::Context) {
        self.gamemode.job.start(ctx, || {
            let (lines, ok) = gamemode::restore();
            (lines, !ok)
        });
    }

    /// Automatic Game Mode: reacts to foreground-window changes (no polling while no game runs),
    /// and while a detected game runs, rechecks every 5 seconds whether it has closed (a game doesn't always
    /// trigger a focus change when it exits).
    fn game_mode_tick(&mut self, ctx: &egui::Context) {
        const RECHECK: std::time::Duration = std::time::Duration::from_secs(5);
        let gm = &mut self.gamemode;
        match (gm.config.auto, gm.hook) {
            (true, None) => gm.hook = gamemode::watch::start(ctx),
            (false, Some(h)) => {
                gamemode::watch::stop(h);
                gm.hook = None;
            }
            _ => {}
        }
        // A session tied to a game (automatic, or started with Play) is watched even when automatic mode is off.
        let auto_session = gm.on.as_ref().is_some_and(|o| o.game.is_some());
        if (!gm.config.auto && !auto_session) || gm.job.running() {
            return;
        }
        let due = auto_session && gm.last_check.elapsed() >= RECHECK;
        if !(gamemode::watch::take_change() || due) {
            if auto_session {
                ctx.request_repaint_after(RECHECK.saturating_sub(gm.last_check.elapsed()));
            }
            return;
        }
        gm.last_check = std::time::Instant::now();
        // Wakes the next check exactly one interval from now; a no-op frame if no game is running.
        ctx.request_repaint_after(RECHECK);
        let running = crate::system::running_processes();
        if let Some(on) = gm.on.as_mut().filter(|o| o.game.is_some()) {
            // Ends when its game closes, but only once the game was seen running (a launch takes a while).
            if on.game.as_ref().is_some_and(|g| running.contains(g)) {
                on.seen = true;
            } else if on.seen {
                self.game_mode_off(ctx);
            }
            return;
        }
        let game = gm.config.running_game(&running).map(|g| (g.profile, g.file_name()));
        match (game, gm.on.is_some()) {
            (Some((profile, name)), false) if gm.config.auto && !gm.suppressed => self.game_mode_on(ctx, profile, Some(name)),
            (None, _) => gm.suppressed = false,
            _ => {}
        }
    }

    /// Play: Game Mode with the game's profile (its Game Mode entry, or the manual profile), then the game itself,
    /// started through Explorer so it never inherits Kestrel's administrator rights.
    pub(crate) fn play(&mut self, ctx: &egui::Context, exe: &str) {
        let file = std::path::Path::new(exe).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        if let Err(e) = crate::system::shell_execute("explorer.exe", &format!("\"{exe}\""), false, None) {
            return self.show_error("Couldn't start the game", e);
        }
        log(&format!("Play: started {exe}"));
        if self.gamemode.on.is_some() {
            return; // already on; leave the running session alone
        }
        let gm = &self.gamemode;
        let profile = gm.config.games.iter().find(|g| g.file_name() == file).map_or(gm.config.manual_profile, |g| g.profile);
        self.game_mode_on(ctx, profile, Some(file));
        if let Some(on) = &mut self.gamemode.on {
            on.seen = false;
        }
        self.gamemode.last_check = std::time::Instant::now();
    }

    /// Measures a running game for `measure::SECONDS` in the background.
    pub(crate) fn start_measure(&mut self, ctx: &egui::Context, name: String, exe: &str) {
        use crate::optimizer::measure;
        let file = std::path::Path::new(exe).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        if measure::presentmon().is_none() {
            let (pm_name, pm_id) = measure::PRESENTMON;
            let tool =
                upgrade::apps::AppUpdate { name: pm_name.into(), id: pm_id.into(), current: String::new(), available: String::new() };
            self.confirm = Some(Confirm {
                title: "Install PresentMon to measure games?".into(),
                body: format!(
                    "Kestrel measures frame rates with PresentMon, Intel's free, open-source (MIT) frame-time tool used by reviewers. It's a single small program installed through winget ({pm_id}), which checks it against the catalog's hash.\n\nMeasuring starts by itself once it's installed."
                ),
                warning: None,
                ok: "Install",
                action: Action::InstallApps(vec![tool]),
            });
            self.gaming.pending_measure = Some((name, exe.to_string()));
            return;
        }
        if !self.admin {
            return self.show_error(
                "Measure",
                "Measuring needs administrator rights, because Windows protects the event tracing PresentMon uses (Settings > Restart as administrator).",
            );
        }
        if !crate::system::running_processes().contains(&file) {
            return self.show_error("Measure", format!("Start {name} first and get into gameplay, then click Measure."));
        }
        self.gaming.measuring = Some((name.clone(), std::time::Instant::now()));
        let (path, build) = (exe.to_string(), self.build);
        self.gaming.measure.start(ctx, move || {
            // The advice is gathered right away, while the game is still open, so it matches what was measured.
            let result = measure::capture(&file).map(|s| (s, crate::optimizer::advice::advise(&path, build)));
            (name, file, result)
        });
    }

    /// Adds or removes the tray icon to match the setting, handles its clicks, and turns closing the window
    /// into tucking it away (minimized, no taskbar button) while the icon is there.
    fn tray_tick(&mut self, ctx: &egui::Context) {
        use crate::system::tray::{taskbar_button, Request, Tray};
        let want = self.gamemode.config.tray;
        if want && self.tray.is_none() {
            // Drawn at the tray's own size (16 px at 100% scaling) so the small-size details survive.
            let size =
                unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSMICON) };
            self.tray = Tray::add(ctx, &crate::icon_sized(size.clamp(16, 64) as usize));
            if self.tray.is_none() {
                self.gamemode.config.tray = false;
                let _ = self.gamemode.config.save();
                self.show_error(
                    "Notification area",
                    "Windows didn't accept the notification-area icon, so closing the window still exits Kestrel.",
                );
            }
        } else if !want {
            self.tray = None;
        }
        match self.tray.as_ref().and_then(|t| t.take()) {
            Some(Request::Open) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Some(Request::Exit) => {
                self.exiting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
        if self.tray.is_some() && !self.exiting && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            taskbar_button(false);
            self.tucked = true;
        }
        // Restored by the icon, a second launch or Alt+Tab: give the taskbar button back.
        // (The minimize takes effect after this frame, so wait until it has been seen.)
        match ctx.input(|i| i.viewport().minimized) {
            Some(true) if self.tucked => self.tucked_seen = true,
            Some(false) if self.tucked && self.tucked_seen => {
                taskbar_button(true);
                (self.tucked, self.tucked_seen) = (false, false);
            }
            _ => {}
        }
    }

    /// Asks GitHub for a newer release in the background. `manual` reports "up to date" and errors too.
    pub(crate) fn start_update_check(&mut self, ctx: &egui::Context, manual: bool) {
        if !self.updates.check.running() {
            self.updates.manual = manual;
            self.updates.check.start(ctx, update::latest);
        }
    }

    pub(crate) fn start_update_download(&mut self, ctx: &egui::Context) {
        let Some(release) = self.updates.available.clone() else { return };
        if self.updates.download.running() {
            return;
        }
        let progress = self.updates.progress.clone();
        self.updates.download.start(ctx, move || update::download(&release, &progress.0, &progress.1));
    }

    /// Asks before closing Kestrel to install the verified update ("Later" keeps it for Settings > Updates).
    pub(crate) fn run_install_update(&mut self, _ctx: &egui::Context) {
        let version = self.updates.available.as_ref().map(|r| r.version.clone()).unwrap_or_default();
        let game_mode = if gamemode::is_active() { " Game Mode ends first and puts your settings back." } else { "" };
        self.confirm = Some(Confirm {
            title: format!("Restart to install Kestrel {version}?"),
            body: format!(
                "The update was downloaded and its signature checked. Kestrel will close and start the new version.{game_mode}\n\nYour settings, Game Mode setup and saved originals are kept."
            ),
            warning: None,
            ok: "Restart now",
            action: Action::InstallUpdate,
        });
    }

    /// The new version is put in place, then Kestrel closes normally (ending Game Mode first) and starts it.
    fn install_update(&mut self, ctx: &egui::Context) {
        let Some(staged) = self.updates.staged.take() else { return };
        let target = match std::env::current_exe() {
            Ok(t) => t,
            Err(e) => return self.show_error("Update", e.to_string()),
        };
        match update::install(&staged, &target) {
            Ok(()) => {
                self.relaunch = Some(target);
                self.exiting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => self.show_error("Update not installed", e),
        }
    }

    /// Non-blocking notification; errors stay until dismissed.
    pub(crate) fn show_notice(&mut self, title: &str, body: impl Into<String>) {
        self.toasts.push(Toast::new(title, body));
    }

    pub(crate) fn show_error(&mut self, title: &str, body: impl Into<String>) {
        self.toasts.push(Toast::error(title, body));
    }

    /// Switches page and loads its data on first visit (never in the background otherwise).
    pub(crate) fn go(&mut self, page: Page, ctx: &egui::Context) {
        self.page = page;
        match page {
            Page::Apps if !self.apps.loaded => self.load_apps(ctx),
            Page::Upgrades => {
                self.upgrades.gpus = health::gpu_drivers();
                self.upgrades.wu_paused = upgrade::pause::paused_until();
            }
            Page::Startup if !self.startup.loaded => self.load_startup(ctx),
            Page::Gaming => self.refresh_gaming(ctx),
            Page::Privacy if !self.gaming.status_job.running() => self.gaming.status_job.start(ctx, tweak_status),
            Page::GameMode => {
                if self.gaming.games.is_empty() {
                    self.load_games(ctx);
                }
                self.gamemode.apps = self.gamemode.background_apps();
            }
            Page::Settings => {
                self.history = read_history();
                self.about = health::system_summary();
            }
            _ => {}
        }
    }

    pub(crate) fn start_scan(&mut self, ctx: &egui::Context) {
        if !self.cleaner.scan.running() && !self.cleaner.clean.running() {
            self.cleaner.scan.start(ctx, scan_all);
        }
    }

    pub(crate) fn load_apps(&mut self, ctx: &egui::Context) {
        self.apps.loaded = true;
        self.apps.load.start(ctx, uninstaller::list);
    }

    pub(crate) fn check_app_updates(&mut self, ctx: &egui::Context) {
        if !self.upgrades.apps_job.running() && !self.upgrades.install.running() {
            self.upgrades.apps_job.start(ctx, upgrade::apps::available);
        }
    }

    pub(crate) fn check_driver_updates(&mut self, ctx: &egui::Context) {
        if !self.upgrades.drivers_job.running() && !self.upgrades.driver_install.running() {
            self.upgrades.drivers_job.start(ctx, upgrade::drivers::available);
        }
    }

    pub(crate) fn load_startup(&mut self, ctx: &egui::Context) {
        self.startup.loaded = true;
        self.startup.load.start(ctx, startup::list);
    }

    pub(crate) fn refresh_gaming(&mut self, ctx: &egui::Context) {
        if !self.gaming.status_job.running() {
            self.gaming.status_job.start(ctx, tweak_status);
        }
        if !self.gaming.checks_job.running() {
            self.gaming.checks_job.start(ctx, health::run);
        }
        if self.gaming.games.is_empty() {
            self.load_games(ctx);
        }
    }

    pub(crate) fn load_games(&mut self, ctx: &egui::Context) {
        if !self.gaming.games_job.running() {
            self.gaming.games_job.start(ctx, games::find_all);
        }
    }

    /// Raises each display to its maximum, then waits for the user to keep it (see `refresh_trial_dialog`).
    pub(crate) fn try_refresh_fix(&mut self, fixes: Vec<(String, u32, u32)>) {
        let mut applied = Vec::new();
        let mut errors = Vec::new();
        for (device, old, new) in fixes {
            match health::set_refresh_rate(&device, new) {
                Ok(()) => applied.push((device, old, new)),
                Err(e) => errors.push(e),
            }
        }
        if !applied.is_empty() {
            self.gaming.refresh_trial = Some((std::time::Instant::now(), applied));
        }
        if !errors.is_empty() {
            self.show_error("Refresh rate", errors.join("\n"));
        }
    }

    fn refresh_trial_dialog(&mut self, ctx: &egui::Context) {
        const SECONDS: u64 = 15;
        let Some((started, list)) = &self.gaming.refresh_trial else { return };
        let left = SECONDS.saturating_sub(started.elapsed().as_secs());
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let (mut keep, mut revert) = (false, left == 0);
        let (_, close) = dialog(ctx, "refresh_trial", 400.0, |ui| {
            dialog_title(ui, "Keep this refresh rate?");
            for (device, old, new) in list {
                ui.label(format!("{}: {old} Hz to {new} Hz", device.trim_start_matches(r"\\.\")));
            }
            ui.add_space(4.0);
            secondary(ui, format!("If the screen went black or flickers, just wait. It switches back in {left} seconds."));
            dialog_buttons(ui, |ui| {
                revert |= button(ui, true, "Revert").clicked();
                keep = accent_button(ui, true, "Keep").clicked();
            });
        });
        revert |= close && !keep;
        if !(keep || revert) {
            return;
        }
        let (_, list) = self.gaming.refresh_trial.take().unwrap();
        for (device, old, new) in &list {
            if revert {
                if let Err(e) = health::set_refresh_rate(device, *old) {
                    self.show_error("Refresh rate", format!("Couldn't switch back: {e}"));
                }
            } else if let Err(e) = health::keep_refresh_rate(device, *old) {
                self.show_error(
                    "Refresh rate",
                    format!("{new} Hz is active, but Kestrel couldn't save the old value for Restore all: {e}"),
                );
            }
        }
        self.gaming.checks_job.start(ctx, health::run);
    }

    pub(crate) fn start_restore_point(&mut self, ctx: &egui::Context) {
        self.gaming.apply.start(ctx, || vec![create_restore_point().unwrap_or_else(|e| format!("Restore point failed: {e}"))]);
    }

    pub(crate) fn start_optimize(&mut self, ctx: &egui::Context) {
        self.opt.stage = OptStage::Scanning;
        let build = self.build;
        self.opt.scan.start(ctx, move || {
            let recommended: Vec<Tweak> =
                Tweak::ALL.into_iter().filter(|t| Preset::Performance.includes(*t) && t.unsupported(build).is_none()).collect();
            // A correctly configured PC shouldn't be offered changes: settings already on or without effect here are left out.
            let (active, tweaks): (Vec<Tweak>, Vec<Tweak>) =
                recommended.into_iter().partition(|t| t.is_applied() || t.not_needed().is_some());
            let mut refresh = Vec::new();
            let mut attention = Vec::new();
            for c in health::run() {
                if !c.refresh_fix.is_empty() {
                    refresh = c.refresh_fix;
                } else if c.attention {
                    attention.push(c.title);
                }
            }
            OptScan {
                tweaks,
                already_active: active.len(),
                items: scan_all().into_iter().filter(|i| i.found.count > 0).collect(),
                startup_enabled: startup::list().iter().filter(|e| e.enabled).count(),
                refresh,
                attention,
            }
        });
    }

    /// Restores the given tweaks; with `everything`, also startup, per-game and display changes.
    pub(crate) fn start_restore(&mut self, ctx: &egui::Context, tweaks: Vec<Tweak>, everything: bool) {
        // Ending Game Mode writes its own snapshot back, which would undo this restore.
        if gamemode::is_active() {
            self.show_error("Turn off Game Mode first","Game Mode is on and puts its settings back when it ends, which would undo this restore. Turn off Game Mode, then try again.");
            return;
        }
        if everything {
            self.startup.loaded = false; // reload the Startup page on next visit
        }
        self.gaming.apply.start(ctx, move || {
            let mut lines: Vec<String> = tweaks
                .iter()
                .filter_map(|t| match t.restore() {
                    Ok(true) => Some(format!("✔ Restored: {}", t.info().title)),
                    Ok(false) => None,
                    Err(e) => Some(format!("✖ {}: {e}", t.info().title)),
                })
                .collect();
            if everything {
                lines.extend(startup::restore_all());
                lines.extend(gaming::restore_all_games());
                lines.extend(health::restore_displays());
            }
            if lines.is_empty() {
                lines.push("There were no saved settings to restore.".into());
            }
            lines
        });
    }

    fn run(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Clean => {
                let items: Vec<_> = std::mem::take(&mut self.cleaner.items).into_iter().filter(|i| i.selected).collect();
                self.cleaner.clean.start(ctx, move || clean_items(&items));
            }
            Action::Uninstall(apps) => {
                use std::sync::atomic::Ordering::Relaxed;
                let (stop, cancel, pos) = (self.apps.stop_waiting.clone(), self.apps.cancel_queue.clone(), self.apps.queue_pos.clone());
                cancel.store(false, Relaxed);
                self.apps.leftovers.clear();
                self.apps.selected.clear();
                self.apps.queue = apps.iter().map(|a| a.name.clone()).collect();
                let ctx2 = ctx.clone();
                self.apps.uninstall.start(ctx, move || {
                    let (mut lines, mut leftovers) = (Vec::new(), Vec::new());
                    for (i, app) in apps.iter().enumerate() {
                        if cancel.load(Relaxed) {
                            lines.push(format!("Skipped {} (you cancelled the rest).", app.name));
                            continue;
                        }
                        pos.store(i, Relaxed);
                        stop.store(false, Relaxed);
                        ctx2.request_repaint();
                        match uninstaller::uninstall(app, &stop) {
                            Ok(m) => lines.push(m),
                            Err(e) => lines.push(format!("✖ {}: {e}", app.name)),
                        }
                        let left = uninstaller::leftovers(app);
                        if !left.is_empty() {
                            leftovers.push((app.name.clone(), left));
                        }
                    }
                    (lines, leftovers)
                });
            }
            Action::ApplyTweaks(tweaks) => self.gaming.apply.start(ctx, move || apply_tweaks(&tweaks)),
            // Reverse order: per-plan power settings are restored before the power plan itself switches back.
            Action::RestoreAll => self.start_restore(ctx, Tweak::ALL.into_iter().rev().collect(), true),
            Action::InstallUpdate => self.install_update(ctx),
            Action::ApplyFixes(fixes) => {
                use crate::optimizer::advice::Fix;
                let mut tweaks = Vec::new();
                let mut gpu = Vec::new();
                for fix in fixes {
                    match fix {
                        Fix::Tweak(t) => tweaks.push(t),
                        Fix::GameGpu(exe) => gpu.push(exe),
                        Fix::Refresh(list) => self.try_refresh_fix(list),
                    }
                }
                // Fixes are applied once; the next measurement shows what they did.
                if let Some(m) = &mut self.gaming.last {
                    m.advice.auto.clear();
                    m.applied = true;
                }
                self.gaming.apply.start(ctx, move || {
                    let mut lines = apply_tweaks(&tweaks);
                    for exe in gpu {
                        lines.push(match crate::optimizer::gaming::GameSetting::HighPerfGpu.apply(&exe) {
                            Ok(()) => "\u{2714} The game now uses the fast graphics card (next time it starts)".into(),
                            Err(e) => format!("\u{2716} Fast graphics card: {e}"),
                        });
                    }
                    lines
                });
            }
            Action::UpdateApps(apps) => self.start_app_queue(ctx, apps, false),
            Action::InstallApps(apps) => self.start_app_queue(ctx, apps, true),
            Action::InstallDrivers(drivers) => {
                let ids: Vec<String> = drivers.iter().map(|d| d.id.clone()).collect();
                self.upgrades.drivers_selected.clear();
                self.upgrades.driver_install.start(ctx, move || {
                    // The restore point is the safety net, but a PC with System Protection off can still install.
                    let point = create_restore_point()
                        .unwrap_or_else(|e| format!("No restore point: {e} Device Manager can still roll back each driver."));
                    let (mut lines, reboot) = upgrade::drivers::install(&ids)?;
                    lines.iter().for_each(|l| log(&format!("Driver update: {l}")));
                    lines.insert(0, point);
                    Ok((lines, reboot))
                });
            }
        }
    }

    /// Runs winget for each app, one after another: updates, or with `new`, fresh installs from "Get apps".
    fn start_app_queue(&mut self, ctx: &egui::Context, apps: Vec<upgrade::apps::AppUpdate>, new: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        let up = &mut self.upgrades;
        let (cancel, pos) = (up.cancel_queue.clone(), up.queue_pos.clone());
        cancel.store(false, Relaxed);
        up.new_installs = new;
        if new {
            self.apps.get_selected.clear();
        } else {
            up.apps_selected.clear();
        }
        up.queue = apps.iter().map(|a| a.name.clone()).collect();
        let ctx2 = ctx.clone();
        up.install.start(ctx, move || {
            let mut lines = Vec::new();
            for (i, app) in apps.iter().enumerate() {
                if cancel.load(Relaxed) {
                    lines.push(format!("Skipped {} (you cancelled the rest).", app.name));
                    continue;
                }
                pos.store(i, Relaxed);
                ctx2.request_repaint();
                let result = if new { upgrade::apps::install_new(app) } else { upgrade::apps::install(app) };
                let line = result.unwrap_or_else(|e| format!("\u{2716} {e}"));
                log(&format!("App {}: {line} ({} {})", if new { "install" } else { "update" }, app.id, app.available));
                lines.push(line);
            }
            lines
        });
    }

    pub(crate) fn apply_optimize(&mut self, ctx: &egui::Context) {
        let tweaks: Vec<Tweak> = self.opt.tweaks.iter().filter(|(_, on)| *on).map(|(t, _)| *t).collect();
        let items: Vec<_> = std::mem::take(&mut self.opt.items).into_iter().filter(|i| i.selected).collect();
        let restore_point = self.opt.restore_point;
        self.opt.stage = OptStage::Applying;
        self.opt.apply.start(ctx, move || {
            let mut lines = Vec::new();
            if restore_point {
                match create_restore_point() {
                    Ok(msg) => lines.push(msg),
                    Err(e) => return vec![format!("Nothing was changed because the restore point failed: {e}")],
                }
            }
            lines.extend(apply_tweaks(&tweaks));
            lines.extend(clean_items(&items));
            lines
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some((lines, still_on)) = self.gamemode.job.poll() {
            self.show_notice(if still_on { "Game Mode is on" } else { "Game Mode is off" }, lines.join("\n"));
            if !still_on {
                self.gamemode.on = None;
            }
            self.refresh_gaming(ctx); // statuses on the Gaming page may have changed
        }
        if let Some(items) = self.cleaner.scan.poll() {
            self.cleaner.items = items;
        }
        if let Some(entries) = self.cleaner.usage_job.poll() {
            self.cleaner.usage = entries;
        }
        if let Some(files) = self.cleaner.large_job.poll() {
            self.cleaner.large = Some(files);
        }
        if let Some(lines) = self.cleaner.clean.poll() {
            self.show_notice("Cleanup complete", lines.join("\n"));
        }
        if let Some((apps, note)) = self.apps.load.poll() {
            self.apps.apps = apps;
            self.apps.note = note;
        }
        if let Some(result) = self.updates.check.poll() {
            let manual = std::mem::take(&mut self.updates.manual);
            match result {
                Ok(found) => {
                    self.updates.last_error = None;
                    self.updates.prefs.last_check = update::now();
                    if let Err(e) = self.updates.prefs.save() {
                        self.show_error("Updates", e);
                    }
                    if found.is_none() && manual {
                        self.show_notice("No update available", format!("Kestrel {} is the latest version.", update::CURRENT));
                    }
                    self.updates.available = found;
                }
                // Automatic checks fail quietly (offline is normal); the reason shows in Settings > Updates.
                Err(e) => {
                    if manual {
                        self.show_error("Couldn't check for updates", e.clone());
                    }
                    self.updates.last_error = Some(e);
                }
            }
        }
        if let Some(result) = self.updates.download.poll() {
            match result {
                Ok(path) => {
                    self.updates.staged = Some(path);
                    self.run_install_update(ctx);
                }
                Err(e) => self.show_error("Update not installed", e),
            }
        }
        if let Some((lines, leftovers)) = self.apps.uninstall.poll() {
            self.apps.leftovers = leftovers;
            self.apps.queue.clear();
            self.show_notice("Uninstall", lines.join("\n"));
            self.load_apps(ctx);
        }
        if let Some(result) = self.upgrades.apps_job.poll() {
            match result {
                Ok(list) => {
                    let (list, hidden) = upgrade::apps::without_skipped(list);
                    self.upgrades.skipped = hidden;
                    // Ticks for apps that are no longer listed would update nothing.
                    self.upgrades.apps_selected.retain(|id| list.iter().any(|a| &a.id == id));
                    self.upgrades.apps = Some(list);
                }
                Err(e) => self.show_error("Couldn't check for app updates", e),
            }
        }
        if let Some(lines) = self.upgrades.install.poll() {
            self.upgrades.queue.clear();
            if self.upgrades.new_installs {
                self.show_notice("Get apps", lines.join("\n"));
                self.load_apps(ctx); // the new apps appear under Installed apps
                                     // PresentMon was installed for a measurement: start it now instead of asking for another click.
                if let Some((name, exe)) = self.gaming.pending_measure.take() {
                    if crate::optimizer::measure::presentmon().is_some() {
                        self.start_measure(ctx, name, &exe);
                    }
                }
            } else {
                self.show_notice("App updates", lines.join("\n"));
                self.check_app_updates(ctx);
            }
        }
        if let Some(result) = self.apps.get_job.poll() {
            match result {
                Ok(list) => self.apps.get_results = Some(list),
                Err(e) => self.show_error("Couldn't search for apps", e),
            }
        }
        if let Some(result) = self.upgrades.drivers_job.poll() {
            match result {
                Ok(list) => {
                    self.upgrades.drivers_selected.retain(|id| list.iter().any(|d| &d.id == id));
                    self.upgrades.drivers = Some(list);
                }
                Err(e) => self.show_error("Couldn't check for driver updates", e),
            }
        }
        if let Some(result) = self.upgrades.driver_install.poll() {
            match result {
                Ok((mut lines, reboot)) => {
                    if reboot {
                        lines.push("Restart Windows to finish installing. Kestrel doesn't restart for you.".into());
                    }
                    self.upgrades.restart_needed |= reboot;
                    self.show_notice("Driver updates", lines.join("\n"));
                }
                Err(e) => self.show_error("Drivers not installed", e),
            }
            self.upgrades.gpus = health::gpu_drivers();
            self.check_driver_updates(ctx);
        }
        if let Some((name, file, result)) = self.gaming.measure.poll() {
            self.gaming.measuring = None;
            match result {
                Ok((stats, advice)) => {
                    let before = crate::optimizer::measure::history(&file).last().map(|h| h.1);
                    crate::optimizer::measure::save(&file, &stats);
                    let text = crate::optimizer::measure::describe(&stats, before.as_ref());
                    log(&format!("Measure {file}: {text}"));
                    self.show_notice(&format!("{name}: measured"), format!("{text}. Details and fixes are on Gaming > Measure."));
                    self.gaming.last = Some(Measured { name, stats, before, advice, applied: false });
                }
                Err(e) => self.show_error(&format!("{name}: not measured"), e),
            }
        }
        if let Some(entries) = self.startup.load.poll() {
            self.startup.entries = entries;
        }
        if let Some((i, result)) = self.startup.toggle.poll() {
            match (result, self.startup.entries.get_mut(i)) {
                (Ok(()), Some(e)) => e.enabled = !e.enabled,
                (Ok(()), None) => {}
                (Err(err), _) => self.show_error("Couldn't change startup app", err),
            }
        }
        if let Some(status) = self.gaming.status_job.poll() {
            self.gaming.status = status;
        }
        if let Some(checks) = self.gaming.checks_job.poll() {
            self.gaming.checks = checks;
        }
        if let Some(games) = self.gaming.games_job.poll() {
            self.gaming.games = games;
        }
        if let Some(lines) = self.gaming.apply.poll() {
            self.show_notice("Settings", lines.join("\n"));
            self.refresh_gaming(ctx);
            self.gaming.games.iter_mut().for_each(|g| g.refresh());
        }
        if let Some(found) = self.opt.scan.poll() {
            self.opt.tweaks = found.tweaks.into_iter().map(|t| (t, true)).collect();
            self.opt.already_active = found.already_active;
            self.opt.items = found.items;
            self.opt.startup_enabled = found.startup_enabled;
            self.opt.refresh_on = !found.refresh.is_empty();
            self.opt.refresh = found.refresh;
            self.opt.attention = found.attention;
            self.opt.restore_point = self.admin;
            self.opt.stage = OptStage::Review;
        }
        if let Some(mut lines) = self.opt.apply.poll() {
            // The refresh-rate change needs the user's keep-or-revert answer, so it runs here, after everything else.
            let refresh = std::mem::take(&mut self.opt.refresh);
            if self.opt.refresh_on && !refresh.is_empty() && !lines.iter().any(|l| l.starts_with("Nothing was changed")) {
                lines.push("Display refresh rate raised: press Keep in the dialog, or it switches back in 15 seconds.".into());
                self.try_refresh_fix(refresh);
            }
            if lines.is_empty() {
                lines.push("Nothing was selected, so nothing changed.".into());
            }
            self.opt.stage = OptStage::Done(lines);
            self.cleaner.items.clear();
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(c) = &self.confirm {
            if let Some(ok) = confirm(ctx, &c.title, &c.body, c.warning.as_deref(), c.ok) {
                let c = self.confirm.take().unwrap();
                if ok {
                    self.run(c.action, ctx);
                }
            }
        }
        if self.opt.stage != OptStage::Closed {
            self.optimize_dialog(ctx);
        }
        self.refresh_trial_dialog(ctx);
        // First launch: ask once whether to check for updates. Nothing goes online before the answer.
        if !self.updates.prefs.asked && self.confirm.is_none() && self.opt.stage == OptStage::Closed {
            self.update_opt_in(ctx);
        }
        toasts(ctx, &mut self.toasts);
    }
}

/// Checkbox state for a preset, leaving out tweaks that do nothing on this Windows build.
pub(crate) fn preset_selection(p: Preset, build: u32) -> [bool; Tweak::ALL.len()] {
    Tweak::ALL.map(|t| p.includes(t) && t.unsupported(build).is_none())
}

/// One line per tweak. Settings that are already right or wouldn't do anything on this PC are reported
/// as such and left alone, so the summary shows what really changed.
fn apply_tweaks(tweaks: &[Tweak]) -> Vec<String> {
    tweaks
        .iter()
        .map(|t| {
            let title = t.info().title;
            if let Some(why) = t.not_needed() {
                return format!("{NOT_NEEDED}{title}. {why}");
            }
            if t.is_applied() {
                return format!("{ALREADY}{title}");
            }
            match t.apply() {
                Ok(()) => match t.takes_effect() {
                    Some(when) => format!("✔ {title} ({when})"),
                    None => format!("✔ {title}"),
                },
                Err(e) => format!("✖ {title}: {e}"),
            }
        })
        .collect()
}

/// Line prefixes the result summary groups by.
pub(crate) const ALREADY: &str = "Already set: ";
pub(crate) const NOT_NEEDED: &str = "Not needed: ";

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(debug_assertions)]
        self.shots_tick(ctx);
        self.tray_tick(ctx);
        if let Some(page) = self.start_page.take() {
            match page.as_str() {
                "measure" => {
                    self.gaming.tab = GamingTab::Measure;
                    self.go(Page::Gaming, ctx);
                }
                "updates" => self.go(Page::Upgrades, ctx),
                "settings" => self.go(Page::Settings, ctx),
                _ => {}
            }
        }
        self.poll(ctx);
        self.game_mode_tick(ctx);
        // After the window is up (never delays startup), then whenever the daily check falls due while Kestrel keeps
        // running (in the tray, for example). A failed check (offline) is retried at most hourly.
        const RETRY: std::time::Duration = std::time::Duration::from_secs(60 * 60);
        if self.updates.prefs.enabled {
            let tried_recently = self.update_tried.is_some_and(|t| t.elapsed() < RETRY);
            if !tried_recently && self.updates.prefs.check_due(update::now()) {
                self.update_tried = Some(std::time::Instant::now());
                self.start_update_check(ctx, false);
            }
            // One frame an hour is enough to notice the check is due; nothing runs in between.
            ctx.request_repaint_after(RETRY);
        }
        if self.updates.download.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if self.page == Page::Dashboard && !self.overview_requested {
            self.overview_requested = true;
            self.refresh_gaming(ctx);
        }
        egui::SidePanel::left("nav")
            .exact_width(212.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::default().fill(SIDEBAR).inner_margin(egui::Margin { left: 8, right: 8, top: 16, bottom: 12 }))
            .show(ctx, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin { left: 32, right: 24, top: 24, bottom: 0 }))
            .show(ctx, |ui| {
                // Readable line length on wide windows: content stops growing at 920 px.
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    // 12 px gutter so the scrollbar never sits on the lists' edge.
                    ui.set_max_width((ui.available_width() - 12.0).min(920.0));
                    self.update_banner(ui);
                    match self.page {
                        Page::Dashboard => self.dashboard(ui),
                        Page::Cleaner => self.cleaner_page(ui),
                        Page::Apps => self.apps_page(ui),
                        Page::Upgrades => self.upgrades_page(ui),
                        Page::Privacy => self.privacy_page(ui),
                        Page::Startup => self.startup_page(ui),
                        Page::Gaming => self.gaming_page(ui),
                        Page::GameMode => self.game_mode_page(ui),
                        Page::Settings => self.settings_page(ui),
                    }
                    ui.add_space(24.0);
                });
            });
        self.dialogs(ctx);
    }

    /// Kestrel leaves nothing running, so closing it ends Game Mode and puts every setting back.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tray = None;
        if let Some(h) = self.gamemode.hook.take() {
            gamemode::watch::stop(h);
        }
        if gamemode::is_active() {
            gamemode::restore();
        }
        // The new instance waits for this one's single-instance lock, so it starts once this process is gone.
        if let Some(exe) = self.relaunch.take() {
            if std::process::Command::new(&exe).arg(crate::system::RELAUNCHED).spawn().is_err() {
                log("Update: installed, but the new version didn't start; open Kestrel again");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_use_power_mode_not_plan() {
        let perf = preset_selection(Preset::Performance, 26200);
        assert!(perf[Tweak::PowerMode.index()] && perf[Tweak::GameMode.index()]);
        let off = [Tweak::HighPerfPower, Tweak::NoMouseAccel, Tweak::UsbSuspendOff, Tweak::NoTransparency, Tweak::NoAnimations];
        for t in off.into_iter().chain([Tweak::NoBackgroundApps]) {
            assert!(!perf[t.index()], "{t:?} should not be in a preset");
        }
        assert!(!preset_selection(Preset::Balanced, 26200)[Tweak::PowerMode.index()]);
    }
}
