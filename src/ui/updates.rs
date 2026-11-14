//! Update UI: the one-time opt-in question, the "update available" banner and Settings > Updates.

use std::sync::atomic::Ordering;

use eframe::egui::{self, RichText, Ui};

use super::components::*;
use crate::app::App;
use crate::system::open_uri;
use crate::update;

/// How long "Remind me later" hides the banner.
const SNOOZE: u64 = 3 * 24 * 60 * 60;

fn ago(when: u64) -> String {
    if when == 0 {
        return "Never".into();
    }
    match update::now().saturating_sub(when) / 3600 {
        0 => "Less than an hour ago".into(),
        h @ 1..=47 => format!("{h} hour{} ago", if h == 1 { "" } else { "s" }),
        h => format!("{} days ago", h / 24),
    }
}

impl App {
    /// Asked once, on first launch. Until it's answered nothing goes online.
    pub(crate) fn update_opt_in(&mut self, ctx: &egui::Context) {
        let mut answer = None;
        let (_, close) = dialog(ctx, "update_opt_in", 440.0, |ui| {
            dialog_title(ui, "Keep Kestrel up to date?");
            secondary(ui, "Turn on update checks to be told when a new version with improvements and fixes is available.");
            ui.add_space(S2);
            caption(
                ui,
                "Kestrel then contacts github.com at most once a day when it starts, which needs an internet connection. \
                 Nothing about you or your PC is sent, and updates are only installed when you choose to. \
                 You can change this any time in Settings > Updates.",
            );
            dialog_buttons(ui, |ui| {
                if button(ui, true, "Not now").clicked() {
                    answer = Some(false);
                }
                if accent_button(ui, true, "Turn on update checks").clicked() {
                    answer = Some(true);
                }
            });
        });
        // Closing the dialog counts as "Not now"; it won't be asked again.
        let Some(enabled) = answer.or(close.then_some(false)) else { return };
        let prefs = &mut self.updates.prefs;
        prefs.asked = true;
        prefs.enabled = enabled;
        if let Err(e) = prefs.save() {
            self.show_error("Updates", e);
        }
        if enabled {
            self.start_update_check(ctx, false);
        }
    }

    /// Shown above every page while a newer version is available (unless snoozed) or downloading.
    pub(crate) fn update_banner(&mut self, ui: &mut Ui) {
        let u = &self.updates;
        let downloading = u.download.running();
        let Some(release) = u.available.clone() else { return };
        if !downloading && (u.hidden || u.staged.is_some() || update::now() < u.prefs.snooze_until) {
            return;
        }
        let (mut now, mut notes, mut later) = (false, false, false);
        let text = if downloading {
            let (done, total) = (u.progress.0.load(Ordering::Relaxed), u.progress.1.load(Ordering::Relaxed));
            match total {
                0 => format!("Downloading Kestrel {}… {}", release.version, fmt_bytes(done)),
                t => format!("Downloading Kestrel {}… {} of {}", release.version, fmt_bytes(done), fmt_bytes(t)),
            }
        } else {
            format!("Update available: Kestrel {}. A new version with improvements and fixes is ready to install.", release.version)
        };
        info_bar(ui, Status::Ok, &text, |ui| {
            if !downloading {
                later = link(ui, "Remind me later").clicked();
                notes = row_button(ui, true, "Release notes").clicked();
                now = accent_button(ui, true, "Update now").clicked();
            }
        });
        ui.add_space(S3);
        if notes {
            open_uri(&release.notes_url());
        }
        if later {
            self.updates.hidden = true;
            self.updates.prefs.snooze_until = update::now() + SNOOZE;
            let _ = self.updates.prefs.save();
        }
        if now {
            self.start_update_download(ui.ctx());
        }
    }

    pub(crate) fn updates_section(&mut self, ui: &mut Ui) {
        section(ui, "Updates", |_| {});
        let ctx = ui.ctx().clone();
        let (mut save, mut check, mut notes, mut install) = (false, false, false, false);
        let checking = self.updates.check.running();
        group(ui, |ui| {
            let u = &mut self.updates;
            setting_row(
                ui,
                0,
                RichText::new("Check for updates automatically").color(TEXT),
                "At most once a day, when Kestrel starts. Needs an internet connection; updates are never installed without asking.",
                |ui| save = toggle(ui, &mut u.prefs.enabled).changed(),
            );
            let mut desc = format!("Last checked: {}. Channel: Stable.", ago(u.prefs.last_check));
            if let Some(e) = &u.last_error {
                desc.push_str(&format!(" Last check failed: {e}"));
            }
            setting_row(ui, 1, RichText::new(format!("Kestrel {}", update::CURRENT)).color(TEXT), &desc, |ui| {
                if checking {
                    progress(ui, "Checking");
                } else if u.staged.is_some() {
                    install = accent_button(ui, true, "Restart to update").clicked();
                } else {
                    check = button(ui, !u.download.running(), "Check now").clicked();
                }
                notes = link(ui, "Release notes").clicked();
            });
        });
        if save {
            // Turning checks on counts as having answered the first-launch question.
            self.updates.prefs.asked = true;
            if let Err(e) = self.updates.prefs.save() {
                self.show_error("Updates", e);
            }
        }
        if check {
            self.updates.hidden = false;
            self.updates.prefs.snooze_until = 0;
            self.start_update_check(&ctx, true);
        }
        if notes {
            match &self.updates.available {
                Some(r) => open_uri(&r.notes_url()),
                None => open_uri(&format!("{}/releases", update::REPO)),
            }
        }
        if install {
            self.run_install_update(&ctx);
        }
    }
}
