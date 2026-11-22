//! Windows > Services: the fixed list of services a gaming PC doesn't need (see `services.rs`). Tick, apply,
//! revert; nothing outside the list can be changed.

use eframe::egui::{Align, Layout, RichText, Ui};

use super::components::*;
use crate::app::state::{Action, Confirm};
use crate::app::App;
use crate::services::{self, Group, Start};
use crate::system::relaunch_as_admin;

impl App {
    pub(crate) fn services_tab(&mut self, ui: &mut Ui) {
        let st = &mut self.services;
        if !st.loaded {
            st.loaded = true;
            st.load.start(ui.ctx(), services::candidates);
        }
        let admin = self.admin;
        let busy = st.apply.running();
        let mut restart = false;
        if !admin {
            info_bar(ui, Status::Caution, "Changing services needs administrator rights.", |ui| {
                restart = row_button(ui, true, "Restart as administrator").clicked();
            });
            ui.add_space(S3);
        }
        let chosen: Vec<services::Candidate> = st.list.iter().filter(|c| st.selected.contains(c.id()) && !c.applied()).cloned().collect();
        let (mut apply, mut pick) = (false, false);
        ui.horizontal(|ui| {
            apply = accent_button(ui, admin && !busy && !chosen.is_empty(), &format!("Apply {} selected", chosen.len())).clicked();
            pick = button(ui, !busy, "Select recommended").clicked();
            if busy || st.load.running() {
                progress(ui, if busy { "Working…" } else { "Reading services…" });
            }
        });
        if pick {
            st.selected = st.list.iter().filter(|c| c.group == Group::Recommended && !c.applied()).map(|c| c.id()).collect();
        }

        let mut revert = None;
        for (kind, title) in [(Group::Recommended, "Recommended"), (Group::Optional, "Optional"), (Group::Apps, "From your other apps")] {
            let rows: Vec<usize> = (0..st.list.len()).filter(|&i| st.list[i].group == kind).collect();
            if rows.is_empty() {
                continue;
            }
            section(ui, title, |_| {});
            group(ui, |ui| {
                for (n, &i) in rows.iter().enumerate() {
                    let c = &st.list[i];
                    row(ui, n, |ui| {
                        ui.horizontal(|ui| {
                            let mut on = st.selected.contains(c.id()) || c.applied();
                            let names = c.services.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ");
                            let r = ui
                                .add_enabled_ui(!c.applied() && !busy, |ui| checkbox(ui, &mut on, RichText::new(c.title).color(TEXT)))
                                .inner
                                .on_hover_text(format!("Service: {names}\nSets it to {}.", c.to.label()));
                            if r.changed() {
                                if on {
                                    st.selected.insert(c.id());
                                } else {
                                    st.selected.remove(c.id());
                                }
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if c.saved
                                    && row_button(ui, admin && !busy, "Revert").on_hover_text("Put back how it started before").clicked()
                                {
                                    revert = Some(i);
                                }
                                match () {
                                    _ if c.changed_back() => status(ui, Status::Caution, "Changed back"),
                                    _ if c.applied() && c.to == Start::Manual => status(ui, Status::Ok, "Manual"),
                                    _ if c.applied() => status(ui, Status::Ok, "Off"),
                                    _ if c.running => status(ui, Status::Neutral, "Running"),
                                    _ => status(ui, Status::Neutral, "On"),
                                };
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(26.0);
                            caption(ui, format!("You lose: {}", c.lose));
                        });
                    });
                }
            });
        }
        if st.list.is_empty() && !st.load.running() {
            group(ui, |ui| empty_state(ui, "Nothing to turn off", "None of the services on Kestrel's list are on this PC."));
        }
        ui.add_space(S2);
        caption(ui, "Frees a little memory and background activity. It doesn't add FPS by itself; measure a game to check.");

        if let Some(i) = revert {
            let names: Vec<String> = st.list[i].services.iter().map(|(n, _)| n.clone()).collect();
            let title = st.list[i].title;
            st.apply.start(ui.ctx(), move || {
                names
                    .iter()
                    .map(|n| match services::revert(n) {
                        Ok(()) => format!("✔ {title}: {n} restored"),
                        Err(e) => format!("✖ {title} ({n}): {e}"),
                    })
                    .collect()
            });
        }
        if apply {
            let body = chosen.iter().map(|c| format!("{}: {}", c.title, c.lose)).collect::<Vec<_>>().join("\n");
            self.confirm = Some(Confirm {
                title: format!("Turn off {} services?", chosen.len()),
                body: format!(
                    "What you lose:\n{body}\n\nEach original is saved first. Undo any time with Revert, or Settings > Restore everything."
                ),
                warning: None,
                ok: "Turn off",
                action: Action::ApplyServices(chosen),
            });
        }
        if restart {
            if let Err(e) = relaunch_as_admin("services") {
                self.show_error("Restart as administrator", e);
            }
        }
    }
}
