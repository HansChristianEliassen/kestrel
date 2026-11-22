//! Gaming > Measure: measure the game you're playing, see what the numbers mean, and apply the fixes Kestrel can
//! make with one click. Written for people who don't know what a frame time is.

use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use super::components::*;
use crate::app::state::{Action, Confirm};
use crate::app::App;
use crate::optimizer::{advice, measure};
use crate::system::{open_uri, relaunch_as_admin, running_processes};

fn file_of(exe: &str) -> String {
    Path::new(exe).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default()
}

impl App {
    pub(crate) fn measure_tab(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let g = &mut self.gaming;
        // Which of the user's games is running, checked every few seconds while this tab is open.
        if g.running_checked.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
            let running = running_processes();
            g.running = g.games.iter().position(|game| running.contains(&file_of(game.exe())));
            g.running_checked = Some(Instant::now());
        }
        ctx.request_repaint_after(Duration::from_secs(3));
        if g.games.is_empty() {
            group(ui, |ui| {
                if g.games_job.running() {
                    row(ui, 0, |ui| progress(ui, "Looking for your games…"));
                } else {
                    empty_state(ui, "No games found", "Add your game under Games (\"Add a game…\").");
                }
            });
            return;
        }
        let Some(chosen) = g.selected_game.filter(|i| *i < g.games.len()) else { return };
        let (name, exe) = (g.games[chosen].name.clone(), g.games[chosen].exe().to_string());
        let (mut measure_now, mut restart) = (false, false);

        if let Some((what, started)) = &g.measuring {
            let left = u64::from(measure::SECONDS).saturating_sub(started.elapsed().as_secs());
            ctx.request_repaint_after(Duration::from_secs(1));
            info_bar(
                ui,
                Status::Neutral,
                &format!("Measuring {what}: {left} seconds left. Keep playing normally; Kestrel can stay in the background."),
                |ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(TEXT_2));
                },
            );
            ui.add_space(S3);
        } else if !self.admin {
            info_bar(ui, Status::Caution, "Measuring needs administrator rights. Kestrel restarts and comes back to this game.", |ui| {
                restart = row_button(ui, true, "Restart as administrator").clicked();
            });
            ui.add_space(S3);
        }
        if g.on_hdd {
            info_bar(
                ui,
                Status::Caution,
                &format!("{name} is on a hard drive. Loading and texture streaming are slower and can stutter; an SSD helps most."),
                |_| {},
            );
            ui.add_space(S3);
        }
        let busy = g.measuring.is_some();
        group(ui, |ui| {
            let running = g.running == Some(chosen);
            setting_row_note(
                ui,
                0,
                Some(if running { Status::Ok } else { Status::Neutral }),
                RichText::new(if running { format!("{name} is running") } else { "Start your game".to_string() }).color(TEXT),
                if running {
                    "Get into gameplay, not a menu, before measuring."
                } else {
                    "Press Play, or start it the way you usually do."
                },
                None,
                |_| {},
            );
            setting_row(
                ui,
                1,
                RichText::new("Measure while you play").color(TEXT),
                &format!("Play normally for {} seconds.", measure::SECONDS),
                |ui| {
                    let r = accent_button(ui, !busy && running && self.admin, "Measure");
                    measure_now = r.on_disabled_hover_text("Start the game first (and run Kestrel as administrator).").clicked();
                },
            );
        });

        let (mut apply, mut undo) = (None, None);
        // The latest result belongs on this game's page only.
        if let Some(m) = g.last.as_ref().filter(|m| m.name == name) {
            let (good, verdict) = advice::verdict(&m.stats);
            ui.add_space(S3);
            group(ui, |ui| {
                setting_row_note(
                    ui,
                    0,
                    Some(if good { Status::Ok } else { Status::Caution }),
                    RichText::new(verdict).color(TEXT),
                    &measure::describe(&m.stats, m.before.as_ref()),
                    None,
                    |_| {},
                );
                if let Some(text) = advice::limit_text(advice::limit(&m.stats)) {
                    row(ui, 1, |ui| {
                        ui.label(RichText::new(text).color(TEXT_2));
                    });
                }
            });
            if !m.applied.is_empty() {
                ui.add_space(S2);
                info_bar(ui, Status::Ok, "Fixes applied. Measure again in the same spot to compare.", |ui| {
                    if row_button(ui, !busy, "Undo these fixes").clicked() {
                        undo = Some(m.applied.clone());
                    }
                });
            }
            // Shown even when the game runs well: the goal is as many FPS as this PC can give.
            if !good && m.advice.auto.is_empty() && m.advice.manual.is_empty() {
                ui.add_space(S2);
                caption(ui, "Nothing to change. If one spot feels slow, measure there.");
            }
            {
                if good {
                    section(ui, "Want even more FPS?", |_| {});
                }
                if !m.advice.auto.is_empty() {
                    let n = m.advice.auto.len();
                    section(ui, "Kestrel can fix", |ui| {
                        if accent_button(ui, !busy, &format!("Apply {n} fix{}", if n == 1 { "" } else { "es" })).clicked() {
                            apply = Some(m.advice.auto.clone());
                        }
                    });
                    group(ui, |ui| {
                        for (i, (title, why, _)) in m.advice.auto.iter().enumerate() {
                            setting_row_note(ui, i, Some(Status::Caution), RichText::new(title).color(TEXT), why, None, |_| {});
                        }
                    });
                    ui.add_space(S2);
                    caption(ui, "Every fix can be undone.");
                }
                if !good || !m.advice.auto.is_empty() {
                    section(ui, "You can also", |_| {});
                } else {
                    ui.add_space(S2);
                }
                group(ui, |ui| {
                    for (i, (title, detail, link)) in m.advice.manual.iter().enumerate() {
                        setting_row(ui, i, RichText::new(title).color(TEXT), detail, |ui| {
                            if let Some((label, uri)) = link {
                                if row_button(ui, true, label).clicked() {
                                    open_uri(uri);
                                }
                            }
                        });
                    }
                });
            }
        }

        // Earlier results for the chosen game, newest first.
        let history = measure::history(&file_of(&exe));
        if !history.is_empty() {
            section(ui, "Earlier results", |_| {});
            group(ui, |ui| {
                for (i, (when, s)) in history.iter().rev().take(8).enumerate() {
                    row(ui, i, |ui| {
                        ui.horizontal(|ui| {
                            cell(ui, 110.0, false, |ui| caption(ui, crate::upgrade::drivers::date_from_unix_days((*when / 86_400) as i64)));
                            ui.label(RichText::new(measure::describe(s, None)).color(TEXT_2));
                        });
                    });
                }
            });
        }
        ui.add_space(S2);
        caption(ui, "Uses Intel PresentMon (free, open source).");
        let clean = link(ui, "Rule out other software with a clean test").clicked();

        if restart {
            if let Err(e) = relaunch_as_admin(&format!("game:{chosen}")) {
                self.show_error("Restart as administrator", e);
            }
        }
        if clean {
            self.open_clean_test(&ctx);
        }
        if measure_now {
            self.start_measure(&ctx, name, &exe);
        }
        if let Some(fixes) = undo {
            self.confirm = Some(Confirm {
                title: "Undo the fixes?".into(),
                body: "Every setting Kestrel changed from this measurement goes back to how it was.".into(),
                warning: None,
                ok: "Undo",
                action: Action::UndoFixes(fixes),
            });
        }
        if let Some(fixes) = apply {
            let list: Vec<String> = fixes.iter().map(|(title, ..)| format!("• {title}")).collect();
            self.confirm = Some(Confirm {
                title: format!("Apply {} fix{}?", fixes.len(), if fixes.len() == 1 { "" } else { "es" }),
                body: format!(
                    "{}\n\nEach original setting is saved first, so you can undo any of them. If the screen's refresh rate changes, keep it only if the picture looks right.",
                    list.join("\n")
                ),
                warning: None,
                ok: "Apply",
                action: Action::ApplyFixes(fixes.into_iter().map(|(_, _, f)| f).collect()),
            });
        }
    }
}
