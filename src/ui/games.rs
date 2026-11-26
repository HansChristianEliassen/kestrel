//! The Games page: your game library and Game Mode, and each game's own page with everything for that game in
//! one place: Play on top, then tabs for Measure, Game settings, and Game Mode with the Windows switches.

use std::path::Path;

use eframe::egui::{self, RichText, Ui};

use super::components::*;
use crate::app::state::GameTab;
use crate::app::App;
use crate::gamemode::{GameEntry, Profile};
use crate::optimizer::gaming::GameSetting;

impl App {
    /// Opens a game's page (index into the game list).
    pub(crate) fn open_game(&mut self, i: usize) {
        let Some(game) = self.gaming.games.get(i) else { return };
        let (name, exe) = (game.name.clone(), game.exe().to_string());
        self.gaming.selected_game = Some(i);
        self.gaming.game_tab = GameTab::Measure;
        self.gaming.running_checked = None;
        self.open_game_settings(name, exe);
    }

    pub(crate) fn games_page(&mut self, ui: &mut Ui) {
        if let Some(i) = self.gaming.selected_game.filter(|i| *i < self.gaming.games.len()) {
            return self.game_page(ui, i);
        }
        page_header(ui, "Games", "Open a game to measure and tune it.");
        self.games_tab(ui);
    }

    fn game_page(&mut self, ui: &mut Ui, i: usize) {
        if link(ui, "\u{2190} All games").clicked() {
            self.gaming.selected_game = None;
            self.gaming.settings = None;
            return;
        }
        ui.add_space(S2);
        let game = &mut self.gaming.games[i];
        let file = |p: &str| Path::new(p).file_name().map_or(p.to_string(), |f| f.to_string_lossy().into_owned());
        page_header(ui, &game.name, &format!("{} \u{00b7} {}", game.source, file(game.exe())));
        let mut play = false;
        let mut exe_changed = false;
        ui.horizontal(|ui| {
            play = button(ui, true, "Play").on_hover_text("Turns on Game Mode with this game's profile, then starts the game.").clicked();
            // Some games have several programs; the right one matters for Measure and the per-game switches.
            if game.exes.len() > 1 {
                let before = game.chosen;
                egui::ComboBox::from_id_salt("game_exe").selected_text(file(game.exe())).show_ui(ui, |ui| {
                    for (n, e) in game.exes.iter().enumerate() {
                        ui.selectable_value(&mut game.chosen, n, file(e)).on_hover_text(e);
                    }
                });
                exe_changed = game.chosen != before;
            }
        });
        if exe_changed {
            game.refresh();
        }
        let (name, exe) = (game.name.clone(), game.exe().to_string());
        if exe_changed || self.gaming.settings.as_ref().is_none_or(|d| d.exe != exe) {
            self.open_game_settings(name.clone(), exe.clone());
        }
        ui.add_space(S3);
        pivot(
            ui,
            &mut self.gaming.game_tab,
            &[(GameTab::Measure, "Measure"), (GameTab::Settings, "Game settings"), (GameTab::More, "Game Mode")],
        );
        match self.gaming.game_tab {
            GameTab::Measure => self.measure_tab(ui),
            GameTab::Settings => self.game_settings_section(ui),
            GameTab::More => self.game_more_tab(ui, i, &exe),
        }
        if play {
            self.play(ui.ctx(), &exe);
            self.gaming.running_checked = None;
        }
    }

    /// Game Mode for this game (its profile, or adding it), and Windows' own per-program switches.
    fn game_more_tab(&mut self, ui: &mut Ui, i: usize, exe: &str) {
        let build = self.build;
        let file = |p: &str| Path::new(p).file_name().map_or(p.to_string(), |f| f.to_string_lossy().into_owned());
        let file_lc = file(exe).to_lowercase();
        let cfg = &mut self.gamemode.config;
        let mut save = false;
        group(ui, |ui| match cfg.games.iter_mut().find(|g| g.file_name() == file_lc) {
            Some(entry) => {
                let when = if cfg.auto { "Turns on when this game starts." } else { "Turns on when you press Play." };
                setting_row(ui, 0, RichText::new("Profile while playing").color(TEXT), when, |ui| {
                    egui::ComboBox::from_id_salt("game_profile").selected_text(entry.profile.label()).show_ui(ui, |ui| {
                        for p in Profile::ALL {
                            save |= ui.selectable_value(&mut entry.profile, p, p.label()).changed();
                        }
                    });
                });
            }
            None => {
                setting_row(ui, 0, RichText::new("Not in Game Mode").color(TEXT), "Add it to turn Game Mode on when it starts.", |ui| {
                    if button(ui, true, "Add").clicked() {
                        cfg.games.push(GameEntry { exe: exe.to_string(), profile: Profile::Performance });
                        cfg.auto = true;
                        save = true;
                    }
                });
            }
        });
        if save {
            if let Err(e) = self.gamemode.config.save() {
                self.show_error("Game Mode", e);
            }
        }

        section(ui, "Switches for this game", |_| {});
        let mut error = None;
        let game = &mut self.gaming.games[i];
        group(ui, |ui| {
            for (n, s) in GameSetting::ALL.into_iter().enumerate() {
                let (on, by_kestrel) = game.status[n];
                let mut want = on;
                let why = s.unsupported(build);
                let mut desc = why.unwrap_or(s.desc()).to_string();
                if on && !by_kestrel {
                    desc.push_str(" Set outside Kestrel (in Windows, the NVIDIA Control Panel or another app); turning it off returns this game to the default.");
                }
                setting_row(ui, n, RichText::new(s.label()).color(TEXT), &desc, |ui| {
                    if ui.add_enabled_ui(why.is_none(), |ui| toggle(ui, &mut want)).inner.changed() {
                        let exe = game.exe().to_string();
                        if let Err(e) = if want { s.apply(&exe) } else { s.undo(&exe) } {
                            error = Some(e);
                        }
                        game.refresh();
                    }
                });
            }
        });
        ui.add_space(S2);
        caption(ui, "Takes effect the next time the game starts.");
        if let Some(e) = error {
            self.show_error("Couldn't change the game setting", e);
        }
    }
}
