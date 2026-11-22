//! Clean test: the review dialog, and the banner shown on every page while a test is on.

use eframe::egui::{self, RichText, Ui};

use super::components::*;
use crate::app::App;
use crate::cleantest::{self, Item, Kind};
use crate::system::relaunch_as_admin;

impl App {
    /// Opens the dialog and starts reading what would be turned off.
    pub(crate) fn open_clean_test(&mut self, ctx: &egui::Context) {
        let c = &mut self.clean;
        c.open = true;
        (c.loaded, c.started) = (None, None);
        c.unticked.clear();
        if !c.plan.running() {
            c.plan.start(ctx, cleantest::plan);
        }
    }

    pub(crate) fn clean_test_dialog(&mut self, ctx: &egui::Context) {
        if !self.clean.open {
            return;
        }
        let admin = self.admin;
        let c = &mut self.clean;
        let busy = c.run.running();
        let (mut start, mut restart_now, mut elevate, mut done) = (false, false, false, false);
        let (_, close) = dialog(ctx, "clean_test", 560.0, |ui| {
            dialog_title(ui, "Clean test");
            if let Some(lines) = &c.started {
                secondary(ui, "Other software is off. Restart, then measure your game. Kestrel opens by itself after the restart, and End test puts everything back.");
                ui.add_space(S2);
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for l in lines {
                        ui.label(RichText::new(l).color(TEXT_2));
                    }
                });
                dialog_buttons(ui, |ui| {
                    restart_now = accent_button(ui, true, "Restart now").clicked();
                    done = button(ui, true, "Restart later").clicked();
                });
                return;
            }
            secondary(ui, "Turns off other programs that start with Windows for one restart, so you can measure your game without them, then puts them back. Windows' own services, anti-cheat, launchers and hardware software stay on.");
            ui.add_space(S3);
            if !admin {
                ui.label("Changing services needs administrator rights.");
                dialog_buttons(ui, |ui| {
                    elevate = accent_button(ui, true, "Restart as administrator").clicked();
                    done = button(ui, true, "Cancel").clicked();
                });
                return;
            }
            let Some(p) = &c.loaded else {
                progress(ui, "Checking what starts with Windows…");
                dialog_buttons(ui, |ui| done = button(ui, true, "Cancel").clicked());
                return;
            };
            if p.services.is_empty() && p.startup.is_empty() {
                ui.label(strong("Nothing else starts with Windows on this PC."));
                secondary(ui, "So other software isn't what slows your games. If a game still stutters, a driver may be the cause: LatencyMon shows which one.");
                dialog_buttons(ui, |ui| done = accent_button(ui, true, "Close").clicked());
                return;
            }
            ui.label(strong("Measure your game first if you haven't, so there's a result to compare with."));
            ui.add_space(S2);
            egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                for (title, items) in [("Services", &p.services), ("Startup apps", &p.startup)] {
                    if items.is_empty() {
                        continue;
                    }
                    ui.add_space(S2);
                    ui.label(strong(format!("{title} ({})", items.len())));
                    for i in items {
                        let id = i.id();
                        let mut on = !c.unticked.contains(&id);
                        let label = match i.kind {
                            Kind::Service(_) => i.detail.clone(),
                            Kind::Startup(..) => format!("{} · {}", i.name, i.detail),
                        };
                        if checkbox(ui, &mut on, RichText::new(label).color(TEXT)).changed() {
                            if on {
                                c.unticked.remove(&id);
                            } else {
                                c.unticked.insert(id);
                            }
                        }
                    }
                }
                if !p.kept.is_empty() {
                    ui.add_space(S2);
                    egui::CollapsingHeader::new(RichText::new(format!("Kept running ({})", p.kept.len())).color(TEXT_2)).show(ui, |ui| {
                        for (name, why) in &p.kept {
                            caption(ui, format!("{name}: {why}"));
                        }
                    });
                }
            });
            let n = p.services.len() + p.startup.len() - c.unticked.len();
            dialog_buttons(ui, |ui| {
                if busy {
                    progress(ui, "Turning things off…");
                }
                start = accent_button(ui, !busy && n > 0, &format!("Start test ({n})")).clicked();
                done = button(ui, !busy, "Cancel").clicked();
            });
        });
        if (close && !busy) || done {
            c.open = false;
        }
        if start {
            if let Some(p) = &c.loaded {
                let items: Vec<Item> = p.services.iter().chain(&p.startup).filter(|i| !c.unticked.contains(&i.id())).cloned().collect();
                c.run.start(ctx, move || cleantest::start(&items));
            }
        }
        if elevate {
            if let Err(e) = relaunch_as_admin("cleantest") {
                self.show_error("Restart as administrator", e);
            }
        }
        if restart_now {
            self.clean.open = false;
            if let Err(e) = crate::system::run_hidden("shutdown.exe", &["/r", "/t", "5", "/c", "Kestrel: restarting for the clean test."]) {
                self.show_error("Restart", e);
            }
        }
    }

    /// Shown above every page while a clean test is on, so it's never forgotten.
    pub(crate) fn clean_test_banner(&mut self, ui: &mut Ui) {
        let Some((services, apps)) = self.clean.active else { return };
        let busy = self.clean.run.running();
        let mut end = false;
        let text = format!("Clean test is on: {services} services and {apps} startup apps are off. Measure your game, then end the test.");
        info_bar(ui, Status::Caution, &text, |ui| {
            end = accent_button(ui, !busy, "End test").clicked();
        });
        ui.add_space(S3);
        if end {
            self.clean.run.start(ui.ctx(), cleantest::end);
        }
    }
}
