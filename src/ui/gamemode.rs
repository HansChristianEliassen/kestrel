//! The Game Mode page, and the Game Mode switch row the Overview reuses.

use eframe::egui::{self, RichText, Ui};

use super::components::*;
use crate::app::App;
use crate::gamemode::{self, watch, Action, GameEntry, Profile};
use crate::system::pick_exe;

impl App {
    /// "Game Mode  [On/Off]" settings row with the current state as its description.
    pub(crate) fn game_mode_row(&mut self, ui: &mut Ui, i: usize) {
        let gm = &self.gamemode;
        let busy = gm.job.running();
        let desc = match &gm.on {
            Some(on) => match &on.game {
                Some(game) => format!("On · {} · {game} is running", on.profile.label()),
                None => format!("On · {} · until you turn it off", on.profile.label()),
            },
            None if busy => "Working…".to_string(),
            None if gm.config.auto => "Off · turns on when one of your games starts".to_string(),
            None => "Off · quiets your PC while you play".to_string(),
        };
        let mut on = gm.on.is_some();
        let mut changed = false;
        setting_row(ui, i, RichText::new("Game Mode").color(TEXT), &desc, |ui| {
            changed = ui.add_enabled_ui(!busy, |ui| toggle(ui, &mut on)).inner.changed();
        });
        if changed {
            let ctx = ui.ctx().clone();
            if on {
                let profile = self.gamemode.config.manual_profile;
                self.game_mode_on(&ctx, profile, None);
            } else {
                // Turned off by hand during a game: stay off until that game closes.
                self.gamemode.suppressed = self.gamemode.on.as_ref().is_some_and(|o| o.game.is_some());
                self.game_mode_off(&ctx);
            }
        }
    }

    pub(crate) fn game_mode_page(&mut self, ui: &mut Ui) {
        secondary(ui, "Quiets your PC while you play; undone when the game closes.");
        ui.add_space(S2);
        let busy = self.gamemode.job.running();
        let mut save = false;

        group(ui, |ui| {
            self.game_mode_row(ui, 0);
            let cfg = &mut self.gamemode.config;
            setting_row(
                ui,
                1,
                RichText::new("Start automatically").color(TEXT),
                "Turns on when one of your games starts and off when it closes. Works while Kestrel is open or minimized, or in the tray with Settings > Minimize to tray when closed.",
                |ui| save |= toggle(ui, &mut cfg.auto).changed(),
            );
            setting_row(ui, 2, RichText::new("Profile").color(TEXT), cfg.manual_profile.desc(), |ui| {
                egui::ComboBox::from_id_salt("gm_profile_manual").selected_text(cfg.manual_profile.label()).show_ui(ui, |ui| {
                    for p in Profile::ALL {
                        save |= ui.selectable_value(&mut cfg.manual_profile, p, p.label()).changed();
                    }
                });
            });
        });
        if self.gamemode.config.auto {
            ui.add_space(S2);
            caption(ui, "Kestrel never opens or inspects the game, so anti-cheat has nothing to object to.");
        }

        let profile = self.gamemode.config.manual_profile;
        section(ui, "What the profile changes", |ui| {
            caption(ui, format!("{} profile", profile.label()));
        });
        let active = profile.actions(&self.gamemode.config);
        group(ui, |ui| {
            for (i, a) in Action::ALL.into_iter().enumerate() {
                let custom = &mut self.gamemode.config.custom;
                setting_row_note(ui, i, None, RichText::new(a.label()).color(TEXT), a.desc(), a.warning(), |ui| {
                    if profile == Profile::Custom {
                        let mut on = custom.contains(&a);
                        if checkbox(ui, &mut on, "").changed() {
                            custom.retain(|x| *x != a);
                            if on {
                                custom.push(a);
                            }
                            save = true;
                        }
                    } else if active.contains(&a) {
                        status(ui, Status::Ok, "Included");
                    } else {
                        status(ui, Status::Neutral, "Not included");
                    }
                });
            }
        });
        if profile != Profile::Custom {
            ui.add_space(S2);
            caption(ui, "Choose the Custom profile to pick the actions yourself.");
        }

        let mut browse = false;
        section(ui, "Games", |ui| browse = link(ui, "Browse for a game…").clicked());
        if browse {
            if let Some(exe) = pick_exe() {
                self.add_game_mode_game(exe);
                save = true;
            }
        }
        let mut remove_game = None;
        let mut add_exe = None;
        group(ui, |ui| {
            let games = &mut self.gamemode.config.games;
            if games.is_empty() {
                empty_state(
                    ui,
                    "No games yet",
                    "Add a detected game below, or browse for a game's .exe. Use the game's own program (for example cod.exe), not its launcher.",
                );
            }
            for (i, g) in games.iter_mut().enumerate() {
                setting_row(ui, i, RichText::new(g.file_name()).color(TEXT), &g.exe, |ui| {
                    if row_button(ui, true, "Remove").clicked() {
                        remove_game = Some(i);
                    }
                    egui::ComboBox::from_id_salt(("gm_profile", i)).selected_text(g.profile.label()).width(120.0).show_ui(ui, |ui| {
                        for p in Profile::ALL {
                            save |= ui.selectable_value(&mut g.profile, p, p.label()).changed();
                        }
                    });
                });
            }
            let known: Vec<String> = games.iter().map(|g| g.exe.to_lowercase()).collect();
            let candidates: Vec<_> =
                self.gaming.games.iter().filter(|g| g.source != "Other" && !known.contains(&g.exe().to_lowercase())).collect();
            row(ui, games.len().max(1), |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("gm_add_detected").selected_text("Add a detected game").width(220.0).show_ui(ui, |ui| {
                        if candidates.is_empty() {
                            ui.label(RichText::new("No other detected games").color(TEXT_3));
                        }
                        for game in &candidates {
                            if ui.selectable_label(false, &game.name).on_hover_text(game.exe()).clicked() {
                                add_exe = Some(game.exe().to_string());
                            }
                        }
                    });
                    if self.gaming.games_job.running() {
                        progress(ui, "Looking for installed games");
                    }
                });
            });
        });
        if let Some(i) = remove_game {
            self.gamemode.config.games.remove(i);
            save = true;
        }
        if let Some(exe) = add_exe {
            self.add_game_mode_game(exe);
            save = true;
        }

        let mut reload_apps = false;
        section(ui, "Background apps", |ui| reload_apps = link(ui, "Refresh list").clicked());
        if reload_apps {
            self.gamemode.apps = self.gamemode.background_apps();
        }
        let mut remove_app = None;
        let mut add_app = None;
        group(ui, |ui| {
            let close = &self.gamemode.config.close;
            for (i, app) in close.iter().enumerate() {
                setting_row(ui, i, RichText::new(app).color(TEXT), "", |ui| {
                    if row_button(ui, true, "Remove").clicked() {
                        remove_app = Some(i);
                    }
                });
            }
            if close.is_empty() {
                row(ui, 0, |ui| {
                    secondary(
                        ui,
                        "No apps yet, so efficiency mode and closing apps do nothing. Add the apps you leave open while you play.",
                    );
                });
            }
            row(ui, close.len().max(1), |ui| {
                egui::ComboBox::from_id_salt("gm_add_app").selected_text("Add an open app").width(260.0).show_ui(ui, |ui| {
                    // Largest first, so the apps worth quieting or closing are at the top.
                    for (app, bytes) in self.gamemode.apps.iter().filter(|(a, _)| !close.contains(&a.to_lowercase())) {
                        let label = if *bytes > 0 { format!("{app}  ·  {}", fmt_bytes(*bytes)) } else { app.clone() };
                        if ui.selectable_label(false, label).clicked() {
                            add_app = Some(app.to_lowercase());
                        }
                    }
                });
            });
        });
        ui.add_space(S2);
        caption(ui, "Windows components, security software and anti-cheat can't be added.");
        if let Some(i) = remove_app {
            self.gamemode.config.close.remove(i);
            save = true;
        }
        if let Some(app) = add_app.filter(|a| watch::protected(a).is_none()) {
            self.gamemode.config.close.push(app);
            save = true;
        }

        section(ui, "Recovery", |_| {});
        let mut restore = false;
        group(ui, |ui| {
            setting_row(
                ui,
                0,
                RichText::new("Restore previous settings").color(TEXT),
                "Puts the saved originals back if Game Mode was cut off. Also runs at your next sign-in.",
                |ui| {
                    restore = button(ui, gamemode::is_active() && !busy, "Restore")
                        .on_disabled_hover_text("Nothing to restore right now.")
                        .clicked()
                },
            );
        });
        if restore {
            self.game_mode_off(ui.ctx());
        }

        if save {
            if let Err(e) = self.gamemode.config.save() {
                self.show_notice("Couldn't save Game Mode settings", e);
            }
        }
    }

    fn add_game_mode_game(&mut self, exe: String) {
        if !self.gamemode.config.games.iter().any(|g| g.exe.eq_ignore_ascii_case(&exe)) {
            self.gamemode.config.games.push(GameEntry { exe, profile: Profile::Balanced });
        }
    }
}
