pub mod state;

use eframe::egui;

use crate::cleaner::{clean_items, scan_all};
use crate::gamemode;
use crate::optimizer::{games, gaming, health, Preset, Tweak};
use crate::system::{create_restore_point, has_battery, is_admin, windows_build};
use crate::ui::components::{accent_button, button, confirm, dialog, dialog_buttons, dialog_title, secondary, toasts, Toast, BG, SIDEBAR};
use crate::utils::logging::read_history;
use crate::{startup, uninstaller};
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
    pub(crate) startup: StartupState,
    pub(crate) gaming: GamingState,
    pub(crate) opt: OptState,
    pub(crate) gamemode: GameModeState,
    pub(crate) history: String,
    /// Notification-area icon, present only while "Keep watching after closing the window" is on.
    tray: Option<crate::system::tray::Tray>,
    /// Exit was chosen from the tray menu, so the next close request really closes.
    exiting: bool,
    /// The window was closed to the tray: minimized, without a taskbar button.
    tucked: bool,
    tucked_seen: bool,
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
            startup: StartupState::default(),
            gaming: GamingState { selected: preset_selection(Preset::Balanced, build), ..Default::default() },
            opt: OptState::default(),
            gamemode: GameModeState::default(),
            history: String::new(),
            tray: None,
            exiting: false,
            tucked: false,
            tucked_seen: false,
        };
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
        gm.on = Some(GameModeOn { profile, game });
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
        if !gm.config.auto || gm.job.running() {
            return;
        }
        let auto_session = gm.on.as_ref().is_some_and(|o| o.game.is_some());
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
        let game = gm.config.running_game(&running).map(|g| (g.profile, g.file_name()));
        match (game, gm.on.is_some()) {
            (Some((profile, name)), false) if !gm.suppressed => self.game_mode_on(ctx, profile, Some(name)),
            (None, true) if auto_session => self.game_mode_off(ctx),
            (None, _) => gm.suppressed = false,
            _ => {}
        }
    }

    /// Adds or removes the tray icon to match the setting, handles its clicks, and turns closing the window
    /// into tucking it away (minimized, no taskbar button) while the icon is there.
    fn tray_tick(&mut self, ctx: &egui::Context) {
        use crate::system::tray::{taskbar_button, Request, Tray};
        let want = self.gamemode.config.tray;
        if want && self.tray.is_none() {
            self.tray = Tray::add(ctx, &crate::icon());
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
            Page::Startup if !self.startup.loaded => self.load_startup(ctx),
            Page::Gaming => self.refresh_gaming(ctx),
            Page::GameMode => {
                if self.gaming.games.is_empty() {
                    self.load_games(ctx);
                }
                self.gamemode.apps = gamemode::watch::windowed_apps();
            }
            Page::Settings => self.history = read_history(),
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
            let battery = has_battery();
            let recommended: Vec<Tweak> = Tweak::ALL.into_iter().filter(|t| recommended(Preset::Performance, *t, build, battery)).collect();
            let (active, tweaks): (Vec<Tweak>, Vec<Tweak>) = recommended.into_iter().partition(|t| t.is_applied());
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
        }
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
        if let Some((lines, leftovers)) = self.apps.uninstall.poll() {
            self.apps.leftovers = leftovers;
            self.apps.queue.clear();
            self.show_notice("Uninstall", lines.join("\n"));
            self.load_apps(ctx);
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
            self.show_notice("Gaming settings", lines.join("\n"));
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
        toasts(ctx, &mut self.toasts);
    }
}

/// Checkbox state for a preset, leaving out tweaks that do nothing on this Windows build.
pub(crate) fn preset_selection(p: Preset, build: u32) -> [bool; Tweak::ALL.len()] {
    let battery = has_battery();
    Tweak::ALL.map(|t| recommended(p, t, build, battery))
}

/// Presets never pick the High performance plan on a laptop or tablet: it drains the battery for little gain.
fn recommended(p: Preset, t: Tweak, build: u32, battery: bool) -> bool {
    p.includes(t) && t.unsupported(build).is_none() && !(battery && t == Tweak::HighPerfPower)
}

fn apply_tweaks(tweaks: &[Tweak]) -> Vec<String> {
    tweaks
        .iter()
        .map(|t| match t.apply() {
            Ok(()) => format!("✔ {}", t.info().title),
            Err(e) => format!("✖ {}: {e}", t.info().title),
        })
        .collect()
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tray_tick(ctx);
        self.poll(ctx);
        self.game_mode_tick(ctx);
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
                    ui.set_max_width(ui.available_width().min(920.0));
                    match self.page {
                        Page::Dashboard => self.dashboard(ui),
                        Page::Cleaner => self.cleaner_page(ui),
                        Page::Apps => self.apps_page(ui),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_leave_out_power_plan_on_battery() {
        assert!(recommended(Preset::Performance, Tweak::HighPerfPower, 26200, false));
        assert!(!recommended(Preset::Performance, Tweak::HighPerfPower, 26200, true));
        assert!(recommended(Preset::Performance, Tweak::GameMode, 26200, true));
        for t in [Tweak::NoMouseAccel, Tweak::UsbSuspendOff, Tweak::NoTransparency, Tweak::NoAnimations, Tweak::NoBackgroundApps] {
            assert!(!recommended(Preset::Performance, t, 19045, false), "{t:?} should not be in a preset");
        }
    }
}
