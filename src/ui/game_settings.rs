//! Gaming > Per-game > Game settings: pick Quality, Balanced or Performance and Kestrel writes the game's own
//! settings file (with Undo), or shows the exact menu steps for games whose files aren't documented.

use eframe::egui::{self, RichText};

use super::components::*;
use crate::app::state::SettingsDialog;
use crate::app::App;
use crate::optimizer::game_settings::{self as gs, Preset, Source};

impl App {
    pub(crate) fn open_game_settings(&mut self, name: String, exe: String) {
        let source = gs::find(&name, &exe);
        let preset = Preset::Balanced;
        let changes = source.as_ref().map(|s| gs::preview(s, preset).unwrap_or_default()).unwrap_or_default();
        self.gaming.settings = Some(SettingsDialog { name, exe, source, preset, changes });
    }

    pub(crate) fn game_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.gaming.settings else { return };
        let (mut close, mut apply, mut undo, mut changed) = (false, false, false, false);
        let (_, x) = dialog(ctx, "game_settings", 520.0, |ui| {
            dialog_title(ui, &format!("{}: game settings", d.name));
            let mut p = d.preset;
            if segmented(ui, &mut p, &Preset::ALL.map(|p| (p, p.label()))) {
                d.preset = p;
                changed = true;
            }
            secondary(ui, d.preset.desc());
            ui.add_space(S3);
            match &d.source {
                None => {
                    ui.label("Kestrel doesn't know this game's settings file yet.");
                    caption(ui, "Measure the game on Gaming > Measure: it tells you which of its settings to lower first.");
                }
                Some(Source::Guide(game)) => {
                    ui.label(strong("In the game's menu:"));
                    for (i, step) in gs::guide(game, d.preset).iter().enumerate() {
                        ui.label(format!("{}. {step}", i + 1));
                    }
                    ui.add_space(S1);
                    caption(ui, "This game's settings file isn't documented, so Kestrel doesn't edit it. These are the same changes, done in the game.");
                }
                Some(source) => {
                    if d.changes.is_empty() {
                        ui.label("Nothing to change: every setting Kestrel knows is already at or below this preset.");
                    } else {
                        ui.label(strong(format!("{} setting{} change:", d.changes.len(), if d.changes.len() == 1 { "" } else { "s" })));
                        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                            for (name, from, to) in &d.changes {
                                ui.label(RichText::new(format!("{name}: {from} \u{2192} {to}")).color(TEXT_2));
                            }
                        });
                    }
                    if matches!(source, Source::Rdr2(_)) {
                        ui.add_space(S1);
                        caption(ui, "Also turn on DLSS in the game (Settings > Graphics > NVIDIA DLSS: Quality, or Balanced for more FPS). It usually adds the most FPS of all.");
                    }
                    ui.add_space(S1);
                    caption(ui, "Only settings above this preset are lowered; nothing is raised. Close the game first. The original file is saved, so Undo puts everything back.");
                    dialog_buttons(ui, |ui| {
                        if button(ui, true, "Close").clicked() {
                            close = true;
                        }
                        apply = accent_button(ui, !d.changes.is_empty(), "Apply").clicked();
                        if gs::has_backup(source)
                            && button(ui, true, "Undo").on_hover_text("Put back the settings from before Kestrel changed them").clicked()
                        {
                            undo = true;
                        }
                    });
                    return;
                }
            }
            dialog_buttons(ui, |ui| close = accent_button(ui, true, "Close").clicked());
        });
        if changed {
            d.changes = d.source.as_ref().map(|s| gs::preview(s, d.preset).unwrap_or_default()).unwrap_or_default();
        }
        let file = std::path::Path::new(&d.exe).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        if apply {
            if let Some(source) = d.source.clone() {
                let (name, preset) = (d.name.clone(), d.preset);
                match gs::apply(&source, preset, &file) {
                    Ok(done) => {
                        self.show_notice(
                            &format!("{name}: {} settings", preset.label()),
                            format!(
                                "Changed {} settings. Start the game to play with them, and measure it to see the difference.",
                                done.len()
                            ),
                        );
                        self.gaming.settings = None;
                    }
                    Err(e) => self.show_error(&format!("{name}: settings not changed"), e),
                }
            }
            return;
        }
        if undo {
            if let Some(source) = d.source.clone() {
                let name = d.name.clone();
                match gs::undo(&source, &file) {
                    Ok(()) => {
                        self.show_notice(
                            &format!("{name}: settings restored"),
                            "The game's settings are back to how they were before Kestrel changed them.",
                        );
                        self.gaming.settings = None;
                    }
                    Err(e) => self.show_error(&format!("{name}: not restored"), e),
                }
            }
            return;
        }
        if close || x {
            self.gaming.settings = None;
        }
    }
}
