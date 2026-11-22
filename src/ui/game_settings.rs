//! The "Game settings" section of a game's page: pick Quality, Balanced or Performance and Kestrel writes the
//! game's own settings file (with Undo), or shows the exact menu steps for games whose files aren't documented.

use eframe::egui::{RichText, Ui};

use super::components::*;
use crate::app::state::SettingsDialog;
use crate::app::App;
use crate::optimizer::game_settings::{self as gs, Preset, Source};

impl App {
    pub(crate) fn open_game_settings(&mut self, name: String, exe: String) {
        let source = gs::find(&name, &exe);
        let preset = Preset::Balanced;
        self.gaming.on_hdd = crate::optimizer::health::on_hard_drive(&exe);
        let changes = source.as_ref().map(|s| gs::preview(s, preset).unwrap_or_default()).unwrap_or_default();
        self.gaming.settings = Some(SettingsDialog { name, exe, source, preset, changes });
    }

    pub(crate) fn game_settings_section(&mut self, ui: &mut Ui) {
        let Some(d) = &mut self.gaming.settings else { return };
        let (mut apply, mut undo, mut changed) = (false, false, false);
        group(ui, |ui| {
            row(ui, 0, |ui| {
                let mut p = d.preset;
                if segmented(ui, &mut p, &Preset::ALL.map(|p| (p, p.label()))) {
                    d.preset = p;
                    changed = true;
                }
                caption(ui, d.preset.desc());
                ui.add_space(S2);
                match &d.source {
                    None => {
                        ui.label("Kestrel doesn't know this game's settings file yet.");
                        caption(ui, "Measure the game to see what to lower first.");
                    }
                    Some(Source::Guide(game)) => {
                        ui.label(strong("In the game's menu:"));
                        for (i, step) in gs::guide(game, d.preset).iter().enumerate() {
                            ui.label(format!("{}. {step}", i + 1));
                        }
                    }
                    Some(source) => {
                        if d.changes.is_empty() {
                            ui.label("Nothing to change: already at or below this preset.");
                        } else {
                            ui.label(strong(format!("{} setting{} change:", d.changes.len(), if d.changes.len() == 1 { "" } else { "s" })));
                            for (name, from, to) in &d.changes {
                                ui.label(RichText::new(format!("{name}: {from} \u{2192} {to}")).color(TEXT_2));
                            }
                        }
                        if matches!(source, Source::Rdr2(_)) {
                            caption(ui, "Also turn on DLSS (Quality) in the game for the biggest gain.");
                        }
                        ui.add_space(S2);
                        ui.horizontal(|ui| {
                            apply = accent_button(ui, !d.changes.is_empty(), "Apply").clicked();
                            if gs::has_backup(source) {
                                undo = button(ui, true, "Undo")
                                    .on_hover_text("Put back the settings from before Kestrel changed them")
                                    .clicked();
                            }
                            caption(ui, "Close the game first.");
                        });
                    }
                }
            });
        });
        let refresh = |d: &mut SettingsDialog| {
            d.changes = d.source.as_ref().map(|s| gs::preview(s, d.preset).unwrap_or_default()).unwrap_or_default()
        };
        if changed {
            refresh(d);
        }
        let file = std::path::Path::new(&d.exe).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        let (name, preset, source) = (d.name.clone(), d.preset, d.source.clone());
        let Some(source) = source.filter(|_| apply || undo) else { return };
        let result = if apply {
            gs::apply(&source, preset, &file).map(|done| {
                (
                    format!("{name}: {} settings", preset.label()),
                    format!("Changed {} settings. Start the game to play with them, and measure it to see the difference.", done.len()),
                )
            })
        } else {
            gs::undo(&source, &file)
                .map(|()| (format!("{name}: settings restored"), "Back to how they were before Kestrel changed them.".to_string()))
        };
        match result {
            Ok((title, body)) => self.show_notice(&title, body),
            Err(e) => self.show_error(&format!("{name}: settings not changed"), e),
        }
        if let Some(d) = &mut self.gaming.settings {
            refresh(d);
        }
    }
}
