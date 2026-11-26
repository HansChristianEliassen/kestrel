//! GPU > Driver Optimizer: pick or import an NVIDIA driver, choose a preset, review what's left out, then install
//! or export. The work itself is in `crate::nvdriver`.

use std::sync::atomic::Ordering;

use eframe::egui::{self, RichText, Ui};

use super::components::*;
use crate::app::state::{Action, Confirm};
use crate::app::App;
use crate::nvdriver::package::{Preset, Role};
use crate::nvdriver::{self, install, telemetry, Prepared};
use crate::system::{open_uri, pick_exe, pick_folder};

impl App {
    fn driver_poll(&mut self, ctx: &egui::Context) {
        let d = &mut self.driver;
        if let Some(r) = d.releases.poll() {
            d.chosen = 0;
            d.list = Some(r);
        }
        if let Some(r) = d.prepare.poll() {
            match r {
                Ok(p) => {
                    d.keep = p.pkg.preset(d.preset);
                    d.ready = Some(p);
                    d.error = None;
                }
                Err(e) => d.error = Some(e),
            }
        }
        if let Some(r) = d.install.poll() {
            match r {
                // NVIDIA's OpenGL driver would close Kestrel mid-install anyway; close cleanly now. The install
                // window explains what's happening, and its script opens Kestrel again when it's done.
                Ok(()) => std::process::exit(0),
                Err(e) => self.driver.outcome = Some(Err(e)),
            }
        }
        if self.driver.waiting {
            match install::install_state() {
                install::InstallState::Running => ctx.request_repaint_after(std::time::Duration::from_secs(2)),
                install::InstallState::Done(p) => {
                    self.driver.waiting = false;
                    self.driver_check(ctx, p);
                }
                install::InstallState::Idle => self.driver.waiting = false,
            }
        }
        let d = &mut self.driver;
        if let Some(r) = d.export.poll() {
            d.outcome = Some(r.map(|dir| {
                crate::system::show_in_folder(&dir);
                format!("Saved to {}. Run setup.exe in it to install.", dir.display())
            }));
        }
        if let Some(c) = d.verify.poll() {
            d.checks = c;
            // The install moved a package into the rollback folder and may have blocked telemetry.
            d.kept_now = None;
            d.tele_now = None;
        }
        if let Some(t) = d.tele.poll() {
            d.tele_now = Some(t);
        }
        if let Some(r) = d.tele_set.poll() {
            if let Err(e) = r {
                d.error = Some(e);
            }
            d.tele_now = None;
        }
        if let Some(k) = d.kept.poll() {
            d.kept_now = Some(k);
        }
        if d.tele_now.is_none() && !d.tele.running() && !d.tele_set.running() {
            d.tele.start(ctx, telemetry::check);
        }
        if d.kept_now.is_none() && !d.kept.running() {
            d.kept.start(ctx, install::kept);
        }
        if let Some(r) = d.cleaning.poll() {
            d.outcome = Some(r.map(|()| "Kestrel's temporary driver files are deleted.".to_string()));
        }
        if d.prepare.running() || d.install.running() || d.export.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// Shows how an install went, finishes it (telemetry block, gaming profile, rollback copy) and checks it.
    pub(crate) fn driver_check(&mut self, ctx: &egui::Context, p: install::Pending) {
        let d = &mut self.driver;
        // Look the card up again: the driver changed (or is missing).
        d.gpu = None;
        d.kept_now = None;
        d.outcome = Some(match p.code {
            Some(0) => Ok(format!("NVIDIA's installer finished: driver {}.", p.version)),
            Some(c) => Err(format!("NVIDIA's installer ended with code {c}. The results below show what changed.")),
            None => Err("The install was interrupted (for example by a restart) before NVIDIA's installer finished. The results below show what's installed now; if the driver is missing, install it again below.".into()),
        });
        d.verify.start(ctx, move || install::after_install(&p));
    }

    fn start_prepare(&mut self, ctx: &egui::Context, source: Source) {
        let Some(Some(gpu)) = self.driver.gpu.clone() else { return };
        let d = &mut self.driver;
        d.progress = Default::default();
        d.ready = None;
        d.error = None;
        d.outcome = None;
        d.checks.clear();
        let (progress, stage) = (d.progress.clone(), d.stage.clone());
        d.prepare.start(ctx, move || {
            let exe = match source {
                Source::Download(r) => {
                    if let Ok(mut s) = stage.lock() {
                        *s = format!("Downloading {}…", r.version);
                    }
                    nvdriver::download(&r, &progress)?
                }
                Source::File(p) => std::path::PathBuf::from(p),
            };
            nvdriver::prepare(&exe, &gpu.device, &stage)
        });
    }

    /// GPU > Driver Optimizer > Install (or a rollback), after the confirmation.
    pub(crate) fn start_driver_install(&mut self, ctx: &egui::Context) {
        let previous = self.driver.gpu.clone().flatten().and_then(|g| g.installed);
        let d = &mut self.driver;
        let (clean, stage) = (d.clean, d.stage.clone());
        let mut job = install::Pending { block: !d.no_block, profile: !d.no_profile, previous, ..Default::default() };
        d.outcome = None;
        d.checks.clear();
        if let Some(version) = d.rollback.take() {
            let Some((_, dir, _)) = d.kept_now.iter().flatten().find(|k| k.0 == version).cloned() else { return };
            job.removed = install::kept_removed(&version);
            job.version = version;
            if let Ok(mut s) = stage.lock() {
                *s = "Installing with NVIDIA's installer (a few minutes; the screen goes dark for a moment)…".into();
            }
            d.install.start(ctx, move || install::install(&dir, false, &job));
            return;
        }
        let Some(p) = d.ready.clone() else { return };
        let keep = d.keep.clone();
        d.install.start(ctx, move || {
            if let Ok(mut s) = stage.lock() {
                *s = "Building the package…".into();
            }
            let out = nvdriver::build_dir(&p.pkg.version);
            let removed = nvdriver::package::build(&p.pkg, &keep, &out)?;
            if let Ok(mut s) = stage.lock() {
                *s = "Installing with NVIDIA's installer (a few minutes; the screen goes dark for a moment)…".into();
            }
            job.removed = removed.into_iter().collect();
            job.version = p.pkg.version.clone();
            install::install(&out, clean, &job)
        });
    }

    pub(crate) fn driver_optimizer(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.driver.gpu.is_none() {
            self.driver.gpu = Some(nvdriver::detect());
            // Full GPU clocks cost battery life: the gaming profile starts off on laptops.
            self.driver.no_profile = crate::optimizer::hardware::has_battery();
        }
        self.driver_poll(&ctx);
        if self.driver.waiting {
            info_bar(
                ui,
                Status::Caution,
                "NVIDIA's installer is still running. Don't restart or turn off the PC; this page updates when it's done.",
                |ui| progress(ui, "Installing"),
            );
            ui.add_space(S3);
        }
        let Some(Some(gpu)) = self.driver.gpu.clone() else {
            self.driver_missing(ui);
            return;
        };
        if self.driver.list.is_none() && !self.driver.releases.running() {
            let (g, studio) = (gpu.clone(), self.driver.studio);
            self.driver.releases.start(&ctx, move || nvdriver::releases(&g, studio));
        }
        caption(
            ui,
            "Installs NVIDIA's driver without the NVIDIA App, its background services, overlay, recorder and telemetry, keeping every gaming feature. NVIDIA's signed files are never changed.",
        );
        ui.add_space(S3);
        let installed = gpu.installed.clone().unwrap_or_else(|| "unknown".into());
        let busy_now = self.driver.prepare.running() || self.driver.install.running() || self.driver.export.running();
        let (mut block_flip, mut rollback, mut forget) = (None, None, false);
        group(ui, |ui| {
            setting_row(ui, 0, RichText::new(&gpu.name).color(TEXT), &format!("Installed driver: {installed}"), |_| {});
            let t = self.driver.tele_now.clone();
            let desc = match &t {
                None => "Checking…".to_string(),
                Some(t) => {
                    let live = if !t.container_running {
                        "NVIDIA's display container isn't running.".to_string()
                    } else if t.connections.is_empty() {
                        "NVIDIA's display container has no internet connections right now.".to_string()
                    } else {
                        format!("NVIDIA's display container is connected to {}.", t.connections.join(", "))
                    };
                    let hosts = if t.reachable.is_empty() {
                        "Its telemetry servers can't be reached from this PC.".to_string()
                    } else {
                        format!("Still reachable: {}.", t.reachable.join(", "))
                    };
                    format!("The driver itself reports usage data to NVIDIA. Blocking its telemetry servers in the Windows hosts file stops that without touching the driver; game-profile updates and driver downloads keep working. {hosts} {live}")
                }
            };
            setting_row(ui, 1, RichText::new("Block NVIDIA telemetry").color(TEXT), &desc, |ui| {
                let mut on = t.as_ref().is_some_and(|t| t.blocked);
                let r = ui.add_enabled_ui(t.is_some() && !self.driver.tele_set.running(), |ui| toggle(ui, &mut on)).inner;
                if r.changed() {
                    block_flip = Some(on);
                }
            });
            let others: Vec<(String, std::path::PathBuf, u64)> =
                self.driver.kept_now.iter().flatten().filter(|k| k.0 != installed).cloned().collect();
            if let Some((v, _, bytes)) = others.last() {
                setting_row(
                    ui,
                    2,
                    RichText::new(format!("Previous driver: {v}")).color(TEXT),
                    &format!("Kept by Kestrel ({}) so you can go back if the new driver gives you trouble.", fmt_bytes(*bytes)),
                    |ui| {
                        if row_button(ui, !busy_now, "Reinstall").clicked() {
                            rollback = Some(v.clone());
                        }
                        forget = link(ui, "Delete").clicked();
                    },
                );
            }
        });
        if let Some(on) = block_flip {
            self.driver.tele_set.start(&ctx, move || telemetry::set_blocked(on));
        }
        if forget {
            self.driver.kept_now = None;
            self.driver.kept.start(&ctx, || {
                let _ = install::forget_kept();
                install::kept()
            });
        }
        if let Some(v) = rollback {
            self.driver.rollback = Some(v.clone());
            self.confirm = Some(Confirm {
                title: format!("Go back to NVIDIA driver {v}?"),
                body: format!("Reinstalls the stripped driver Kestrel kept.\n\n{INSTALL_NOTE}"),
                warning: None,
                ok: "Reinstall",
                action: Action::InstallNvDriver,
            });
        }

        // 1. The driver.
        let busy = self.driver.prepare.running() || self.driver.install.running() || self.driver.export.running();
        let mut source = None;
        let mut reload = false;
        section(ui, "1. Get a driver", |ui| {
            let mut studio = self.driver.studio;
            // The section's right side lays out right to left: the switch first puts the label before it.
            if ui.add_enabled_ui(!busy, |ui| toggle(ui, &mut studio)).inner.changed() {
                reload = true;
            }
            caption(ui, "Studio drivers");
            self.driver.studio = studio;
        });
        group(ui, |ui| {
            row(ui, 0, |ui| {
                ui.horizontal(|ui| match &self.driver.list {
                    None => progress(ui, "Asking NVIDIA for drivers for this card…"),
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).color(TEXT_2));
                    }
                    Some(Ok(list)) => {
                        let label = |r: &nvdriver::Release| format!("{} · {} · {} · {}", r.version, r.name, r.date, r.size);
                        let chosen = self.driver.chosen.min(list.len() - 1);
                        egui::ComboBox::from_id_salt("nv_release").width(420.0).selected_text(label(&list[chosen])).show_ui(ui, |ui| {
                            for (i, r) in list.iter().enumerate() {
                                ui.selectable_value(&mut self.driver.chosen, i, label(r));
                            }
                        });
                        if accent_button(ui, !busy, "Download").clicked() {
                            source = Some(Source::Download(list[chosen].clone()));
                        }
                    }
                });
            });
            row(ui, 1, |ui| {
                ui.horizontal(|ui| {
                    if button(ui, !busy, "Import a driver file…").clicked() {
                        source = pick_exe("Choose an NVIDIA driver file").map(Source::File);
                    }
                    caption(ui, "or drop an NVIDIA driver .exe on this window.");
                });
            });
        });
        ui.add_space(S2);
        caption(ui, "The newest driver isn't always the best one for your games. If yours works well, there's no need to change.");
        if !busy {
            let dropped = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
            });
            if let Some(p) = dropped {
                source = Some(Source::File(p.to_string_lossy().into_owned()));
            }
        }
        if reload {
            self.driver.list = None;
        }
        if let Some(s) = source {
            self.start_prepare(&ctx, s);
        }

        if self.driver.prepare.running() {
            ui.add_space(S3);
            ui.horizontal(|ui| {
                let p = &self.driver.progress;
                let (done, total) = (p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed));
                let stage = self.driver.stage.lock().map(|s| s.clone()).unwrap_or_default();
                let text = if stage.starts_with("Downloading") && total > 0 {
                    format!("{stage} {} of {}", fmt_bytes(done), fmt_bytes(total))
                } else {
                    stage
                };
                progress(ui, &text);
                if text.starts_with("Downloading") && link(ui, "Cancel").clicked() {
                    p.cancel.store(true, Ordering::Relaxed);
                }
            });
        }
        if let Some(e) = &self.driver.error {
            ui.add_space(S3);
            info_bar(ui, Status::Caution, e, |_| {});
        }
        if let Some(ready) = self.driver.ready.clone() {
            self.driver_review(ui, &ready, busy);
        }
        self.driver_results(ui);
    }

    fn driver_review(&mut self, ui: &mut Ui, ready: &Prepared, busy: bool) {
        let pkg = &ready.pkg;
        section(ui, &format!("2. Choose what to keep · driver {}", pkg.version), |_| {});
        match ready.supported {
            Some(false) => {
                info_bar(ui, Status::Caution, "This package doesn't list your graphics card, so it can't be installed on this PC.", |_| {})
            }
            None => info_bar(ui, Status::Neutral, "This package has no device list; NVIDIA's installer checks your card itself.", |_| {}),
            Some(true) => {}
        }
        let before = self.driver.preset;
        pivot(ui, &mut self.driver.preset, &Preset::ALL.map(|p| (p, p.label())));
        if self.driver.preset != before && self.driver.preset != Preset::Custom {
            self.driver.keep = pkg.preset(self.driver.preset);
        }
        caption(ui, self.driver.preset.desc());
        ui.add_space(S3);

        let keep = &self.driver.keep;
        let (total, kept) = (pkg.total_bytes(), pkg.kept_bytes(keep));
        let removed: Vec<&nvdriver::package::Component> = pkg.components.iter().filter(|c| !keep.contains(&c.folder)).collect();
        let telemetry: Vec<&str> =
            removed.iter().filter(|c| c.known().is_some_and(|k| k.telemetry)).map(|c| c.known().map_or("", |k| k.title)).collect();
        let runtime = removed.iter().filter(|c| c.known().is_some_and(|k| !k.runs.is_empty())).count();
        group(ui, |ui| {
            let pct = (100 * (total - kept)).checked_div(total).unwrap_or(0);
            setting_row(
                ui,
                0,
                RichText::new(format!("{} → {} ({pct}% smaller)", fmt_bytes(total), fmt_bytes(kept))).color(TEXT),
                &format!(
                    "{} component(s) left out, {runtime} of them with something running in the background{}. Smaller files alone don't add FPS; fewer background programs mean less to compete with your game.",
                    removed.len(),
                    if telemetry.is_empty() { String::new() } else { format!(", including {}", telemetry.join(", ")) }
                ),
                |_| {},
            );
        });

        section(ui, "Components", |_| {});
        let custom = self.driver.preset == Preset::Custom;
        let mut flip = None;
        group(ui, |ui| {
            for (i, c) in pkg.components.iter().enumerate() {
                let k = c.known();
                let on = keep.contains(&c.folder);
                let title = k.as_ref().map_or(c.folder.clone(), |k| k.title.to_string());
                let mut desc = match &k {
                    Some(k) => {
                        let runs = if k.runs.is_empty() {
                            "Nothing runs until something uses it.".to_string()
                        } else {
                            format!("Runs: {}", k.runs)
                        };
                        format!("{} {runs} Without it: {}", k.what, k.lose)
                    }
                    None => format!("Not in Kestrel's list (package {}). Kept, since its purpose isn't known.", c.main()),
                };
                let title_of = |f: &str| {
                    pkg.components.iter().find(|x| x.folder == f).and_then(|x| x.known()).map_or(f.to_string(), |k| k.title.to_string())
                };
                let needed_by: Vec<String> =
                    pkg.needed_by(&c.folder).into_iter().filter(|f| keep.contains(f)).map(|f| title_of(&f)).collect();
                if on && !needed_by.is_empty() && !Preset::Custom.locked(c.role()) {
                    desc.push_str(&format!(" Kept because {} needs it.", needed_by.join(", ")));
                }
                let role = c.role();
                setting_row(ui, i, RichText::new(format!("{title} · {}", fmt_bytes(c.bytes))).color(TEXT), &desc, |ui| {
                    if custom {
                        let mut want = on;
                        let locked = Preset::Custom.locked(role);
                        let r = ui.add_enabled_ui(!locked && !busy, |ui| toggle(ui, &mut want)).inner;
                        let r = if locked { r.on_disabled_hover_text("Always kept: the driver or your hardware needs it.") } else { r };
                        if r.changed() {
                            flip = Some((c.folder.clone(), want));
                        }
                    } else if on {
                        status(ui, Status::Ok, "Kept");
                    } else {
                        status(ui, Status::Neutral, "Left out");
                    }
                    if role == Role::Unknown && !on {
                        status(ui, Status::Caution, "Unknown");
                    }
                });
            }
        });
        if let Some((folder, on)) = flip {
            let d = &mut self.driver;
            if on {
                d.keep.insert(folder.clone());
                d.keep.extend(pkg.needs(&folder));
            } else {
                d.keep.remove(&folder);
                for f in pkg.needed_by(&folder) {
                    d.keep.remove(&f);
                }
            }
            pkg.complete(&mut d.keep);
        }

        // 3. Install or export.
        section(ui, "3. Install", |_| {});
        group(ui, |ui| {
            setting_row(
                ui,
                0,
                RichText::new("Clean installation").color(TEXT),
                "Resets all NVIDIA Control Panel settings to defaults first, including Kestrel's per-game NVIDIA switches and shader cache size. Leave off unless you're fixing a problem.",
                |ui| {
                    ui.add_enabled_ui(!busy, |ui| toggle(ui, &mut self.driver.clean));
                },
            );
            let mut block = !self.driver.no_block;
            setting_row(
                ui,
                1,
                RichText::new("Block NVIDIA telemetry").color(TEXT),
                "Blocks the driver's telemetry servers in the hosts file after installing (see above).",
                |ui| {
                    ui.add_enabled_ui(!busy, |ui| toggle(ui, &mut block));
                },
            );
            self.driver.no_block = !block;
            let mut profile = !self.driver.no_profile;
            setting_row(
                ui,
                2,
                RichText::new("Apply the NVIDIA gaming profile").color(TEXT),
                "Kestrel's one profile for every game, in the driver's global settings: Prefer maximum performance, Low Latency Mode On, a 10 GB shader cache, and the latest DLSS model on RTX 40 and 50 cards. Switch it on or off any time under GPU > Settings.",
                |ui| {
                    ui.add_enabled_ui(!busy, |ui| toggle(ui, &mut profile));
                },
            );
            self.driver.no_profile = !profile;
        });
        ui.add_space(S3);
        let can_install = ready.supported != Some(false) && !busy;
        let (mut go, mut export, mut eula) = (false, false, false);
        ui.horizontal(|ui| {
            go = accent_button(ui, can_install, &format!("Install driver {}", pkg.version)).clicked();
            export = button(ui, !busy, "Export…")
                .on_hover_text("Save the stripped package to a folder, to install later or on another PC")
                .clicked();
            if busy && !self.driver.prepare.running() {
                let stage = self.driver.stage.lock().map(|s| s.clone()).unwrap_or_default();
                progress(ui, &stage);
            }
        });
        ui.add_space(S2);
        ui.horizontal_wrapped(|ui| {
            caption(
                ui,
                "Uses NVIDIA's own installer, and Windows checks NVIDIA's signature as always. Installing accepts NVIDIA's licence.",
            );
            eula = link(ui, "Read it").clicked();
        });
        if eula {
            open_uri(&pkg.dir.join("EULA.txt").to_string_lossy());
        }
        if go {
            let removed: Vec<String> = pkg
                .components
                .iter()
                .filter(|c| !self.driver.keep.contains(&c.folder))
                .map(|c| c.known().map_or(c.folder.clone(), |k| k.title.to_string()))
                .collect();
            self.confirm = Some(Confirm {
                title: format!("Install NVIDIA driver {}?", pkg.version),
                body: format!(
                    "{}: left out are {}.\n\n{INSTALL_NOTE}{}",
                    self.driver.preset.label(),
                    if removed.is_empty() { "no components".to_string() } else { removed.join(", ") },
                    if self.driver.clean {
                        "\n\nClean installation removes the current driver first, so the PC is without one for longer, and resets your NVIDIA Control Panel settings."
                    } else {
                        ""
                    }
                ),
                warning: None,
                ok: "Install",
                action: Action::InstallNvDriver,
            });
        }
        if export {
            if let Some(folder) = pick_folder("Choose where to save the driver package") {
                let (p, keep) = (ready.clone(), self.driver.keep.clone());
                self.driver.outcome = None;
                if let Ok(mut s) = self.driver.stage.lock() {
                    *s = "Saving the package…".into();
                }
                self.driver.export.start(ui.ctx(), move || {
                    let out = std::path::Path::new(&folder).join(format!("NVIDIA {} for gaming (Kestrel)", p.pkg.version));
                    nvdriver::package::build(&p.pkg, &keep, &out).map(|_| out)
                });
            }
        }
    }

    /// No NVIDIA driver in Windows (an interrupted install, or none yet): offer the driver Kestrel kept.
    fn driver_missing(&mut self, ui: &mut Ui) {
        let kept = self.driver.kept_now.clone().unwrap_or_default();
        if kept.is_empty() {
            group(ui, |ui| {
                empty_state(
                    ui,
                    "No NVIDIA graphics driver",
                    "Driver Optimizer works with NVIDIA graphics cards. If yours is installed but has no driver, Windows Update installs one, or install NVIDIA's driver from nvidia.com.",
                )
            });
        } else {
            let busy = self.driver.install.running() || self.driver.waiting;
            let mut again = None;
            group(ui, |ui| {
                for (i, (v, _, bytes)) in kept.iter().enumerate() {
                    setting_row_note(
                        ui,
                        i,
                        Some(Status::Caution),
                        RichText::new(format!("No NVIDIA driver is installed. Install {v} again?")).color(TEXT),
                        &format!(
                            "Kestrel kept this stripped driver ({}). Installing it again restores your graphics driver.",
                            fmt_bytes(*bytes)
                        ),
                        None,
                        |ui| {
                            if row_button(ui, !busy, "Install").clicked() {
                                again = Some(v.clone());
                            }
                        },
                    );
                }
            });
            if let Some(v) = again {
                self.driver.rollback = Some(v.clone());
                self.confirm = Some(Confirm {
                    title: format!("Install NVIDIA driver {v}?"),
                    body: INSTALL_NOTE.to_string(),
                    warning: None,
                    ok: "Install",
                    action: Action::InstallNvDriver,
                });
            }
        }
        self.driver_results(ui);
    }

    /// How the last install or export went, and what's true after installing.
    fn driver_results(&mut self, ui: &mut Ui) {
        if let Some(outcome) = &self.driver.outcome {
            ui.add_space(S3);
            match outcome {
                Ok(t) => info_bar(ui, Status::Ok, t, |_| {}),
                Err(t) => info_bar(ui, Status::Caution, t, |_| {}),
            }
        }
        if self.driver.verify.running() {
            ui.add_space(S2);
            progress(ui, "Checking the result…");
        }
        let (mut restart, mut tidy) = (false, false);
        if !self.driver.checks.is_empty() {
            section(ui, "After installing", |_| {});
            group(ui, |ui| {
                for (i, (ok, title, detail)) in self.driver.checks.iter().enumerate() {
                    let lead = ok.map(|g| if g { Status::Ok } else { Status::Caution });
                    setting_row_note(ui, i, lead, RichText::new(title).color(TEXT), detail, None, |_| {});
                }
            });
            ui.add_space(S2);
            let needs_restart = self.driver.checks.iter().any(|(ok, t, _)| t == "Driver version" && *ok == Some(false));
            ui.horizontal(|ui| {
                if needs_restart {
                    restart = button(ui, true, "Restart now").clicked();
                }
                tidy = link(ui, "Delete Kestrel's temporary driver files").clicked();
            });
        }
        if restart {
            if let Err(e) =
                crate::system::run_hidden("shutdown.exe", &["/r", "/t", "5", "/c", "Kestrel: restarting to finish the NVIDIA driver."])
            {
                self.show_error("Restart", e);
            }
        }
        if tidy {
            self.driver.ready = None;
            self.driver.cleaning.start(ui.ctx(), nvdriver::clean_work);
        }
    }
}

/// What happens during an install, said before it starts.
const INSTALL_NOTE: &str = "Kestrel closes and a window shows the install for a few minutes. The screen may go dark or flicker, and for a while Windows shows no graphics driver: that's normal. Don't restart or turn off the PC until the window closes; Kestrel then opens again with the result. Windows may ask for administrator rights first.";

enum Source {
    Download(nvdriver::Release),
    File(String),
}
