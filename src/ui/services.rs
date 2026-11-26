//! Windows > Services: the fixed list of services a gaming PC doesn't need (see `services.rs`). Each switch turns a
//! service off or back on at once; nothing outside the list can be changed.

use eframe::egui::{Align, Layout, RichText, Ui};

use super::components::*;
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
        if busy || st.load.running() {
            progress(ui, if busy { "Working…" } else { "Reading services…" });
        }

        let (mut revert, mut turn_off) = (None, None);
        for (kind, title) in [
            (Group::Recommended, "Recommended"),
            (Group::Optional, "Optional"),
            (Group::Apps, "From your other apps"),
            (Group::Extreme, "Extreme (costs a Windows feature)"),
        ] {
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
                            let names = c.services.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ");
                            ui.label(RichText::new(c.title).color(TEXT))
                                .on_hover_text(format!("Service: {names}\nOff sets it to {}.", c.to.label()));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                // On = Windows' setting; Off = Kestrel's. Turned off outside Kestrel: nothing saved to go back to.
                                let mut on = !c.applied();
                                let r = ui.add_enabled_ui(admin && !busy && (on || c.saved), |ui| toggle(ui, &mut on)).inner;
                                if r.changed() {
                                    if on {
                                        revert = Some(i);
                                    } else {
                                        turn_off = Some(c.clone());
                                    }
                                }
                                if c.changed_back() {
                                    ui.add_space(S3);
                                    status(ui, Status::Caution, "Changed back");
                                } else if c.applied() && c.to == Start::Manual {
                                    ui.add_space(S3);
                                    status(ui, Status::Ok, "Starts when needed");
                                }
                            });
                        });
                        caption(ui, format!("Off, you lose: {}", c.lose));
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
        if let Some(c) = turn_off {
            st.apply.start(ui.ctx(), move || services::apply(&c));
        }
        if restart {
            if let Err(e) = relaunch_as_admin("services") {
                self.show_error("Restart as administrator", e);
            }
        }
    }
}
