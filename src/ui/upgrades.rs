//! Updates page: app updates (winget) and driver updates (Windows Update), plus the graphics driver's age.
//! Kestrel's own updates live in Settings > Updates.

use std::collections::HashSet;
use std::sync::atomic::Ordering::Relaxed;

use eframe::egui::{vec2, Label, RichText, Ui};

use super::components::*;
use crate::app::state::UpgradeState;
use crate::app::state::{Action, Confirm};
use crate::app::App;
use crate::optimizer::health::{driver_age, gpu_vendor};
use crate::system::{open_uri, relaunch_as_admin};
use crate::upgrade::apps::{AppUpdate, POPULAR};
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

/// Progress of the winget queue (updates or Get apps installs), with "Cancel the rest".
fn queue_bar(ui: &mut Ui, up: &UpgradeState) {
    let i = up.queue_pos.load(Relaxed);
    let name = up.queue.get(i).cloned().unwrap_or_default();
    let verb = if up.new_installs { "Installing" } else { "Updating" };
    let text = if up.queue.len() > 1 {
        format!("{verb} {} of {}: {name}. Installers may ask for administrator rights.", i + 1, up.queue.len())
    } else {
        format!("{verb} {name}. Its installer may ask for administrator rights.")
    };
    let more = up.queue.len() > i + 1 && !up.cancel_queue.load(Relaxed);
    info_bar(ui, Status::Neutral, &text, |ui| {
        if more && row_button(ui, true, "Cancel the rest").on_hover_text("No more apps start after this one.").clicked() {
            up.cancel_queue.store(true, Relaxed);
        }
    });
    ui.add_space(S2);
}

/// Some installers (Spotify, for one) refuse to run as administrator. Returns whether the note was shown.
fn admin_note(ui: &mut Ui, admin: bool) -> bool {
    if admin {
        info_bar(
            ui,
            Status::Caution,
            "Kestrel is running as administrator. Some app installers refuse to run that way; if one fails, open Kestrel normally and try again.",
            |_| {},
        );
    }
    admin
}

fn install_confirm(apps: Vec<AppUpdate>) -> Confirm {
    let how = "winget downloads each installer from its publisher, checks it against the hash in the winget catalog and installs it silently. Installing accepts each package's license. If an installer needs administrator rights, Windows asks.";
    let (title, body) = if let [a] = &apps[..] {
        (format!("Install {}?", a.name), format!("{} ({})\n\n{how}", a.name, a.id))
    } else {
        let list: Vec<String> = apps.iter().map(|a| format!("• {} ({})", a.name, a.id)).collect();
        (format!("Install {} apps?", apps.len()), format!("{}\n\nThey install one after another. {how}", list.join("\n")))
    };
    Confirm { title, body, warning: None, ok: "Install", action: Action::InstallApps(apps) }
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
    /// Apps > Get apps: search the winget catalog, or pick from a short list of popular apps, and install.
    pub(crate) fn get_apps_tab(&mut self, ui: &mut Ui) {
        let busy = self.upgrades.install.running();
        let searching = self.apps.get_job.running();
        if busy && self.upgrades.new_installs {
            queue_bar(ui, &self.upgrades);
        }
        admin_note(ui, self.admin);
        let mut search = false;
        ui.add_space(S2);
        ui.horizontal(|ui| {
            let r = search_box(ui, &mut self.apps.get_query, "Search for an app, like Steam or Discord", 320.0);
            search = r.lost_focus() && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter));
            search |= button(ui, !searching && !self.apps.get_query.trim().is_empty(), "Search").clicked();
            if searching {
                progress(ui, "Searching the winget catalog…");
            }
        });
        if search && !searching {
            let q = self.apps.get_query.clone();
            self.apps.get_job.start(ui.ctx(), move || crate::upgrade::apps::search(&q));
        }
        let popular: Vec<AppUpdate> = POPULAR
            .iter()
            .map(|(name, id)| AppUpdate { name: (*name).into(), id: (*id).into(), current: String::new(), available: String::new() })
            .collect();
        let st = &mut self.apps;
        let mut target: Option<Vec<AppUpdate>> = None;
        let mut toggled = None;
        let mut bulk = false;
        if !st.get_selected.is_empty() && !busy {
            ui.add_space(S2);
            info_bar(ui, Status::Neutral, &format!("{} selected.", plural(st.get_selected.len(), "app")), |ui| {
                bulk = row_button(ui, true, "Install selected").clicked();
                if row_button(ui, true, "Clear").clicked() {
                    st.get_selected.clear();
                }
            });
        }
        let lists: Vec<(String, &Vec<AppUpdate>)> = match &st.get_results {
            Some(r) => vec![(format!("Results for \u{201c}{}\u{201d}", st.get_query.trim()), r), ("Popular".into(), &popular)],
            None => vec![("Popular".into(), &popular)],
        };
        for (title, list) in lists {
            section(ui, &title, |_| {});
            group(ui, |ui| {
                if list.is_empty() {
                    empty_state(ui, "No apps found", "Try another name. Only apps in the winget catalog can be installed from here.");
                }
                for (i, app) in list.iter().enumerate() {
                    row(ui, i, |ui| {
                        ui.horizontal(|ui| {
                            tick(ui, &st.get_selected, &app.id, !busy, &mut toggled);
                            ui.allocate_ui(vec2((ui.available_width() - 120.0).max(80.0), 20.0), |ui| {
                                ui.add(Label::new(RichText::new(&app.name).color(TEXT)).truncate());
                                caption(
                                    ui,
                                    if app.available.is_empty() { app.id.clone() } else { format!("{} · {}", app.id, app.available) },
                                );
                            });
                            ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                                if row_button(ui, !busy, "Install")
                                    .on_disabled_hover_text("Wait for the current installs to finish.")
                                    .clicked()
                                {
                                    target = Some(vec![app.clone()]);
                                }
                            });
                        });
                    });
                }
            });
        }
        ui.add_space(S2);
        caption(ui, "Apps come from the winget catalog, Windows' own package manager. Each installer is checked against the catalog's hash before it runs.");
        if bulk {
            let all = st.get_results.iter().flatten().chain(popular.iter());
            let mut picked: Vec<AppUpdate> = Vec::new();
            for a in all {
                if st.get_selected.contains(&a.id) && !picked.iter().any(|p| p.id == a.id) {
                    picked.push(a.clone());
                }
            }
            target = (!picked.is_empty()).then_some(picked);
        }
        apply_tick(&mut st.get_selected, toggled);
        if let Some(apps) = target {
            self.confirm = Some(install_confirm(apps));
        }
    }

    pub(crate) fn upgrades_page(&mut self, ui: &mut Ui) {
        self.app_updates(ui);
        self.driver_updates(ui);
        self.windows_update(ui);
        self.graphics_driver(ui);
    }

    /// Pause / resume, exactly as Settings > Windows Update does it.
    fn windows_update(&mut self, ui: &mut Ui) {
        use crate::upgrade::pause;
        section(ui, "Windows Update", |_| {});
        let admin = self.admin;
        let up = &mut self.upgrades;
        if up.pause_weeks == 0 {
            up.pause_weeks = 1;
        }
        let desc = match &up.wu_paused {
            Some(until) => format!("Paused until {until}. Windows doesn't download or install updates, including security updates, until then."),
            None => "Stop updates from downloading or restarting the PC before a match or a busy week. Security updates wait too, so resume when you're done. Windows allows up to 5 weeks.".into(),
        };
        let mut result = None;
        group(ui, |ui| {
            let title = if up.wu_paused.is_some() { "Updates are paused" } else { "Pause updates" };
            setting_row(ui, 0, RichText::new(title).color(TEXT), &desc, |ui| {
                let hover = "Needs administrator rights (Restart as administrator).";
                if up.wu_paused.is_some() {
                    if row_button(ui, admin, "Resume").on_disabled_hover_text(hover).clicked() {
                        result = Some(pause::resume());
                    }
                } else {
                    if row_button(ui, admin, "Pause").on_disabled_hover_text(hover).clicked() {
                        result = Some(pause::pause(up.pause_weeks * 7));
                    }
                    eframe::egui::ComboBox::from_id_salt("pause_weeks")
                        .width(110.0)
                        .selected_text(plural(up.pause_weeks as usize, "week"))
                        .show_ui(ui, |ui| {
                            for w in [1u64, 2, 5] {
                                ui.selectable_value(&mut up.pause_weeks, w, plural(w as usize, "week"));
                            }
                        });
                }
            });
        });
        ui.add_space(S2);
        ui.horizontal_wrapped(|ui| {
            caption(ui, "The same pause as in Settings, so you can also resume it there.");
            if link(ui, "Open Windows Update").clicked() {
                open_uri("ms-settings:windowsupdate");
            }
        });
        match result {
            Some(Ok(msg)) => {
                up.wu_paused = pause::paused_until();
                self.show_notice("Windows Update", msg);
            }
            Some(Err(e)) => self.show_error("Windows Update", e),
            None => {}
        }
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
            queue_bar(ui, up);
        }
        if admin_note(ui, self.admin) {
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
        let mut skip: Option<AppUpdate> = None;
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
                const ACTION: f32 = 140.0;
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
                                if row_button(ui, !busy, "Skip").on_hover_text("Hide this version. A newer one shows again.").clicked() {
                                    skip = Some(app.clone());
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
        let mut unskip = false;
        if up.skipped > 0 {
            ui.horizontal(|ui| {
                caption(ui, format!("{} skipped.", plural(up.skipped, "update")));
                unskip = link(ui, "Show again").clicked();
            });
        }
        if let Some(app) = skip {
            match crate::upgrade::apps::skip(&app) {
                Ok(()) => {
                    up.apps_selected.remove(&app.id);
                    if let Some(list) = &mut up.apps {
                        list.retain(|a| a.id != app.id);
                    }
                    up.skipped += 1;
                }
                Err(e) => self.show_error("Skip", e),
            }
            return;
        }
        if unskip {
            crate::upgrade::apps::unskip_all();
            up.skipped = 0;
            check = true;
        }
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
            if let Err(e) = relaunch_as_admin("updates") {
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
