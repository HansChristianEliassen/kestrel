//! Updates page: app updates (winget) and driver updates (Windows Update), plus the graphics driver's age.
//! Kestrel's own updates live in Settings > Updates.

use std::collections::HashSet;
use std::sync::atomic::Ordering::Relaxed;

use eframe::egui::{vec2, Label, RichText, Ui};

use super::components::*;
use crate::app::state::{Action, Confirm};
use crate::app::App;
use crate::optimizer::health::{driver_age, gpu_vendor};
use crate::system::{open_uri, relaunch_as_admin};
use crate::upgrade::apps::AppUpdate;
use crate::upgrade::drivers::DriverUpdate;

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

fn apps_confirm(apps: Vec<AppUpdate>) -> Confirm {
    let how = "winget downloads each installer from its publisher, checks it against the hash in the winget catalog and runs it silently. An app may close while it updates, so save your work in it first. Updating accepts each package's license. If an installer needs administrator rights, Windows asks.";
    let (title, body) = if let [a] = &apps[..] {
        (format!("Update {}?", a.name), format!("{} to {}\n\n{how}", a.current, a.available))
    } else {
        let list: Vec<String> = apps.iter().map(|a| format!("• {} ({} to {})", a.name, a.current, a.available)).collect();
        (format!("Update {} apps?", apps.len()), format!("{}\n\nThey update one after another. {how}", list.join("\n")))
    };
    Confirm { title, body, warning: None, ok: "Update", action: Action::UpdateApps(apps) }
}

fn drivers_confirm(drivers: Vec<DriverUpdate>) -> Confirm {
    let list: Vec<String> = drivers.iter().map(|d| format!("• {}", d.title)).collect();
    Confirm {
        title: if drivers.len() == 1 { "Install this driver?".into() } else { format!("Install {} drivers?", drivers.len()) },
        body: format!(
            "{}\n\nKestrel creates a restore point, then Windows Update downloads and installs the driver{}. Installing accepts each driver's license. \
             This can take several minutes; the screen may flicker while a display driver installs.\n\n\
             If a device misbehaves afterwards, roll its driver back in Device Manager (the device's Properties > Driver > Roll Back Driver) or use System Restore.",
            list.join("\n"),
            if drivers.len() == 1 { "" } else { "s" }
        ),
        warning: Some("Windows may need a restart to finish. Kestrel never restarts the PC for you.".into()),
        ok: "Install",
        action: Action::InstallDrivers(drivers),
    }
}

/// A ticked/unticked change from a row's checkbox.
fn tick(ui: &mut Ui, selected: &HashSet<String>, id: &str, enabled: bool, toggled: &mut Option<(String, bool)>) {
    let mut on = selected.contains(id);
    if ui.add_enabled_ui(enabled, |ui| checkbox(ui, &mut on, "")).inner.changed() {
        *toggled = Some((id.to_string(), on));
    }
}

fn apply_tick(set: &mut HashSet<String>, toggled: Option<(String, bool)>) {
    match toggled {
        Some((id, true)) => _ = set.insert(id),
        Some((id, false)) => _ = set.remove(&id),
        None => {}
    }
}

impl App {
    pub(crate) fn upgrades_page(&mut self, ui: &mut Ui) {
        page_header(
            ui,
            "Updates",
            "Update your apps with winget and your drivers through Windows Update. Nothing is checked or installed until you click.",
        );
        self.app_updates(ui);
        self.driver_updates(ui);
        self.graphics_driver(ui);
    }

    fn app_updates(&mut self, ui: &mut Ui) {
        let up = &mut self.upgrades;
        let (checking, installing) = (up.apps_job.running(), up.install.running());
        let busy = checking || installing;
        let (mut check, mut all) = (false, false);
        section(ui, "Apps", |ui| {
            if checking && up.apps.is_some() {
                progress(ui, "Checking");
            } else if up.apps.is_some() {
                check = link(ui, "Check again").clicked();
                if up.apps.as_ref().is_some_and(|l| l.len() > 1) && !busy {
                    all = link(ui, "Update all").clicked();
                }
            }
        });
        if installing {
            let i = up.queue_pos.load(Relaxed);
            let name = up.queue.get(i).cloned().unwrap_or_default();
            let text = if up.queue.len() > 1 {
                format!("Updating {} of {}: {name}. Installers may ask for administrator rights.", i + 1, up.queue.len())
            } else {
                format!("Updating {name}. Its installer may ask for administrator rights.")
            };
            let more = up.queue.len() > i + 1 && !up.cancel_queue.load(Relaxed);
            let mut cancel = false;
            info_bar(ui, Status::Neutral, &text, |ui| {
                if more {
                    cancel = row_button(ui, true, "Cancel the rest").on_hover_text("No more updates start after this one.").clicked();
                }
            });
            if cancel {
                up.cancel_queue.store(true, Relaxed);
            }
            ui.add_space(S2);
        }
        let mut bulk = false;
        if !up.apps_selected.is_empty() && !busy {
            info_bar(ui, Status::Neutral, &format!("{} selected.", plural(up.apps_selected.len(), "app")), |ui| {
                bulk = row_button(ui, true, "Update selected").clicked();
                if row_button(ui, true, "Clear").clicked() {
                    up.apps_selected.clear();
                }
            });
            ui.add_space(S2);
        }
        let mut target: Option<Vec<AppUpdate>> = None;
        let mut toggled = None;
        group(ui, |ui| match &up.apps {
            None if checking => row(ui, 0, |ui| progress(ui, "Asking winget which apps have a newer version…")),
            None => {
                setting_row(
                    ui,
                    0,
                    RichText::new("Not checked yet").color(TEXT),
                    "Kestrel asks winget, Windows' own package manager, which installed apps have a newer version. Checking only looks.",
                    |ui| check = accent_button(ui, true, "Check for app updates").clicked(),
                );
            }
            Some(list) if list.is_empty() => {
                empty_state(ui, "Your apps are up to date", "winget found no newer version of any app it knows.")
            }
            Some(list) => {
                const VERSION: f32 = 150.0;
                const ACTION: f32 = 80.0;
                table_header(ui, &[("Name", 0.0, false), ("Installed", VERSION, false), ("Available", VERSION, false), ("", ACTION, true)]);
                for (i, app) in list.iter().enumerate() {
                    row(ui, i + 1, |ui| {
                        ui.horizontal(|ui| {
                            let fill = ui.available_width() - 2.0 * VERSION - ACTION - 3.0 * S2;
                            cell(ui, fill, false, |ui| {
                                tick(ui, &up.apps_selected, &app.id, !busy, &mut toggled);
                                ui.allocate_ui(vec2((fill - 30.0).max(60.0), 20.0), |ui| {
                                    ui.add(Label::new(RichText::new(&app.name).color(TEXT)).truncate()).on_hover_text(&app.id);
                                });
                            });
                            for v in [&app.current, &app.available] {
                                cell(ui, VERSION, false, |ui| ui.add(Label::new(RichText::new(v).size(CAPTION).color(TEXT_2)).truncate()));
                            }
                            cell(ui, ACTION, true, |ui| {
                                if row_button(ui, !busy, "Update").clicked() {
                                    target = Some(vec![app.clone()]);
                                }
                            });
                        });
                    });
                }
            }
        });
        ui.add_space(S2);
        ui.horizontal_wrapped(|ui| {
            caption(ui, "Apps from the winget catalog. Store apps update through the Microsoft Store.");
            if link(ui, "Open Store updates").clicked() {
                open_uri("ms-windows-store://downloadsandupdates");
            }
        });
        apply_tick(&mut up.apps_selected, toggled);
        if let Some(list) = &up.apps {
            if all {
                target = Some(list.clone());
            } else if bulk {
                let picked: Vec<AppUpdate> = list.iter().filter(|a| up.apps_selected.contains(&a.id)).cloned().collect();
                target = (!picked.is_empty()).then_some(picked);
            }
        }
        if check {
            self.check_app_updates(ui.ctx());
        }
        if let Some(apps) = target {
            self.confirm = Some(apps_confirm(apps));
        }
    }

    fn driver_updates(&mut self, ui: &mut Ui) {
        let admin = self.admin;
        let up = &mut self.upgrades;
        let (checking, installing) = (up.drivers_job.running(), up.driver_install.running());
        let busy = checking || installing;
        let mut check = false;
        section(ui, "Drivers", |ui| {
            if checking && up.drivers.is_some() {
                progress(ui, "Checking");
            } else if up.drivers.is_some() && !installing {
                check = link(ui, "Check again").clicked();
            }
        });
        if up.restart_needed {
            info_bar(ui, Status::Caution, "Restart Windows to finish installing drivers.", |_| {});
            ui.add_space(S2);
        }
        if installing {
            info_bar(
                ui,
                Status::Neutral,
                "Creating a restore point, then downloading and installing from Windows Update. This can take several minutes.",
                |ui| {
                    ui.add(eframe::egui::Spinner::new().size(14.0).color(TEXT_2));
                },
            );
            ui.add_space(S2);
        }
        let can_install = admin && !busy;
        let mut restart = false;
        if !admin && up.drivers.as_ref().is_some_and(|l| !l.is_empty()) {
            info_bar(ui, Status::Neutral, "Installing drivers needs administrator rights.", |ui| {
                restart = row_button(ui, true, "Restart as administrator").clicked();
            });
            ui.add_space(S2);
        }
        let mut bulk = false;
        if !up.drivers_selected.is_empty() && !busy {
            info_bar(ui, Status::Neutral, &format!("{} selected.", plural(up.drivers_selected.len(), "driver")), |ui| {
                bulk = row_button(ui, admin, "Install selected").clicked();
                if row_button(ui, true, "Clear").clicked() {
                    up.drivers_selected.clear();
                }
            });
            ui.add_space(S2);
        }
        let mut target: Option<Vec<DriverUpdate>> = None;
        let mut toggled = None;
        group(ui, |ui| match &up.drivers {
            None if checking => row(ui, 0, |ui| progress(ui, "Asking Windows Update for drivers. This can take a minute…")),
            None => {
                setting_row(
                    ui,
                    0,
                    RichText::new("Not checked yet").color(TEXT),
                    "Windows Update offers drivers that Microsoft has certified and matched to this PC's hardware. Checking only looks.",
                    |ui| check = accent_button(ui, true, "Check Windows Update").clicked(),
                );
            }
            Some(list) if list.is_empty() => {
                empty_state(ui, "No driver updates", "Windows Update has no newer drivers for this PC's hardware.")
            }
            Some(list) => {
                for (i, d) in list.iter().enumerate() {
                    row(ui, i, |ui| {
                        ui.horizontal(|ui| {
                            tick(ui, &up.drivers_selected, &d.id, !busy, &mut toggled);
                            let left = ui.available_width() - 90.0;
                            ui.allocate_ui_with_layout(vec2(left, 0.0), eframe::egui::Layout::top_down(eframe::egui::Align::Min), |ui| {
                                ui.set_width(left);
                                ui.add(Label::new(RichText::new(&d.title).color(TEXT)).truncate());
                                let meta: Vec<String> = [
                                    d.class.clone(),
                                    d.maker.clone(),
                                    d.date.clone(),
                                    if d.size > 0 { fmt_bytes(d.size) } else { String::new() },
                                ]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .collect();
                                caption(ui, meta.join(" · "));
                            });
                            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                let r = row_button(ui, can_install, "Install");
                                if r.on_disabled_hover_text(if admin {
                                    "Wait for the current check or install."
                                } else {
                                    "Needs administrator rights."
                                })
                                .clicked()
                                {
                                    target = Some(vec![d.clone()]);
                                }
                            });
                        });
                    });
                }
            }
        });
        ui.add_space(S2);
        caption(
            ui,
            "Monitors, chipsets and other devices get their drivers here too. A monitor's driver only names the screen and its color profile, so there's no speed to gain from it.",
        );
        apply_tick(&mut up.drivers_selected, toggled);
        if bulk {
            if let Some(list) = &up.drivers {
                let picked: Vec<DriverUpdate> = list.iter().filter(|d| up.drivers_selected.contains(&d.id)).cloned().collect();
                target = (!picked.is_empty()).then_some(picked);
            }
        }
        if check {
            self.check_driver_updates(ui.ctx());
        }
        if restart {
            if let Err(e) = relaunch_as_admin() {
                self.show_error("Restart as administrator", e);
            }
        }
        if let Some(drivers) = target {
            self.confirm = Some(drivers_confirm(drivers));
        }
    }

    fn graphics_driver(&mut self, ui: &mut Ui) {
        section(ui, "Graphics driver", |_| {});
        group(ui, |ui| {
            if self.upgrades.gpus.is_empty() {
                empty_state(ui, "No graphics adapter found", "Kestrel couldn't read the graphics driver details.");
            }
            for (i, (name, version, months)) in self.upgrades.gpus.iter().enumerate() {
                let age = months.map(|m| format!(", {}", driver_age(m))).unwrap_or_default();
                let old = months.is_some_and(|m| m > 12);
                let (label, url) = gpu_vendor(name);
                setting_row_note(
                    ui,
                    i,
                    None,
                    RichText::new(name).color(TEXT),
                    &format!("Driver {version}{age}"),
                    old.then_some("Over a year old. Newer drivers often fix performance in recent games."),
                    |ui| {
                        if row_button(ui, true, label).on_hover_text(url).clicked() {
                            open_uri(url);
                        }
                    },
                );
            }
        });
        ui.add_space(S2);
        caption(
            ui,
            "GPU makers release drivers faster than Windows Update carries them, and publish no way to check for them. For the newest driver, use the maker's page or app (NVIDIA App, AMD Software, Intel Driver & Support Assistant).",
        );
    }
}
