//! The Profile panel at the top of Windows > Performance: pick Balanced, Competitive or Extreme, review what it
//! changes on this PC, apply it in one go, and undo it as a whole.

use eframe::egui::{Align, Layout, RichText, Ui};

use super::components::*;
use crate::app::state::{Action, Confirm};
use crate::app::App;
use crate::optimizer::profiles::{self, Kind, Profile};
use crate::system::relaunch_as_admin;

impl App {
    pub(crate) fn profile_panel(&mut self, ui: &mut Ui) {
        let (build, admin) = (self.build, self.admin);
        let p = &mut self.profile;
        if p.loaded != Some(p.profile) && !p.plan.running() {
            p.loaded = Some(p.profile);
            let profile = p.profile;
            p.plan.start(ui.ctx(), move || profiles::plan(profile, build, admin));
        }
        section(ui, "Profile", |_| {});
        let mut chosen = p.profile;
        ui.horizontal(|ui| {
            if segmented(ui, &mut chosen, &Profile::ALL.map(|x| (x, x.label()))) {
                p.profile = chosen;
            }
        });
        ui.add_space(S1);
        caption(ui, p.profile.desc());
        if let Some(hw) = &p.hw {
            let mut notes =
                vec![if hw.battery { "Has a battery: settings that cost battery life are left out" } else { "Desktop PC" }.to_string()];
            if !hw.anticheat.is_empty() {
                notes.push(format!("{} installed: memory integrity stays on", hw.anticheat.join(" and ")));
            }
            caption(ui, notes.join(" · "));
        }
        ui.add_space(S2);
        let mut restart = false;
        if !admin {
            info_bar(ui, Status::Caution, "Profiles change machine-wide settings, which needs administrator rights.", |ui| {
                restart = row_button(ui, true, "Restart as administrator").clicked();
            });
            ui.add_space(S2);
        }
        let busy = p.apply.running() || p.plan.running();
        let ticked: Vec<profiles::Item> = p.items.iter().filter(|i| i.offered() && p.ticked.contains(&i.id())).cloned().collect();
        let (mut apply, mut undo) = (false, false);
        ui.horizontal(|ui| {
            apply = accent_button(ui, admin && !busy && !ticked.is_empty(), &format!("Apply {} ({})", p.profile.label(), ticked.len()))
                .clicked();
            if profiles::has_record() {
                undo = button(ui, admin && !busy, "Undo profile").on_hover_text("Put back everything a profile changed").clicked();
            }
            if busy {
                progress(ui, if p.apply.running() { "Working…" } else { "Checking this PC…" });
            }
        });
        ui.add_space(S3);
        group(ui, |ui| {
            if p.items.is_empty() {
                row(ui, 0, |ui| progress(ui, "Checking this PC…"));
            }
            for (n, item) in p.items.iter().enumerate() {
                row(ui, n, |ui| {
                    ui.horizontal(|ui| {
                        let id = item.id();
                        let mut on = item.done || p.ticked.contains(&id);
                        let label = RichText::new(&item.title).color(TEXT);
                        let r =
                            ui.add_enabled_ui(item.offered() && !busy, |ui| checkbox(ui, &mut on, label)).inner.on_hover_text(&item.detail);
                        if r.changed() {
                            if on {
                                p.ticked.insert(id);
                            } else {
                                p.ticked.remove(&id);
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            match () {
                                _ if item.done => status(ui, Status::Ok, "On"),
                                _ if item.blocked.is_some() => status(ui, Status::Neutral, "Not offered"),
                                _ if item.sensitive => status(ui, Status::Caution, "Your choice"),
                                _ => status(ui, Status::Neutral, item.profile.label()),
                            };
                        });
                    });
                    // Trade-offs that need a decision, and why something is held back, are shown in full.
                    let note = match (&item.blocked, item.sensitive && !item.done) {
                        (Some(why), _) if !item.done => Some(why.clone()),
                        (_, true) => Some(item.detail.clone()),
                        _ => None,
                    };
                    if let Some(note) = note {
                        ui.horizontal(|ui| {
                            ui.add_space(26.0);
                            ui.vertical(|ui| {
                                ui.set_max_width((ui.available_width() - 60.0).max(240.0));
                                ui.add(eframe::egui::Label::new(RichText::new(note).size(CAPTION).color(TEXT_2)).wrap());
                            });
                        });
                    }
                });
            }
        });
        let done = p.items.iter().filter(|i| i.done).count();
        if !p.items.is_empty() {
            ui.add_space(S2);
            caption(ui, format!("{done} already on. Everything here is saved first; Undo profile or Settings > Restore everything puts it back, except removed apps, which you reinstall from the Microsoft Store."));
        }

        if restart {
            if let Err(e) = relaunch_as_admin("profile") {
                self.show_error("Restart as administrator", e);
            }
        }
        if undo {
            self.profile.apply.start(ui.ctx(), profiles::undo);
        }
        if apply {
            let body = ticked.iter().map(|i| format!("• {}", i.title)).collect::<Vec<_>>().join("\n");
            let risky: Vec<String> = ticked
                .iter()
                .filter(|i| i.sensitive)
                .map(|i| match &i.kind {
                    Kind::Tweak(t) => format!("{}: {}", i.title, t.info().drawback),
                    _ => format!("{}: {}", i.title, i.detail),
                })
                .collect();
            self.confirm = Some(Confirm {
                title: format!("Apply the {} profile?", self.profile.profile.label()),
                body: format!("{body}\n\nEach original is saved first. Some changes take effect after a restart."),
                warning: (!risky.is_empty())
                    .then(|| format!("You chose changes that cost security or remove things:\n{}", risky.join("\n"))),
                ok: "Apply",
                action: Action::ApplyProfile(ticked),
            });
        }
    }
}
