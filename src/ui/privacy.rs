//! Privacy page: per-user Windows privacy switches, through the same reversible tweak system as Gaming.

use eframe::egui::Ui;

use super::components::*;
use super::tweak_rows;
use crate::app::state::{Action, Confirm};
use crate::app::App;
use crate::optimizer::Tweak;
use crate::system::open_uri;

impl App {
    pub(crate) fn privacy_page(&mut self, ui: &mut Ui) {
        page_header(
            ui,
            "Privacy",
            "Turn off Windows features that share your data or show promotions. Each original value is saved, so Revert puts it back.",
        );
        let build = self.build;
        let g = &mut self.gaming;
        let all: Vec<Tweak> = Tweak::ALL.into_iter().filter(|t| t.is_privacy()).collect();
        let chosen: Vec<Tweak> = all.iter().copied().filter(|t| g.selected[t.index()]).collect();
        let busy = g.apply.running();
        let (mut apply, mut pick_off) = (false, false);
        ui.horizontal(|ui| {
            apply = accent_button(ui, !busy && !chosen.is_empty(), &format!("Apply {} selected", chosen.len())).clicked();
            pick_off = button(ui, !busy, "Select all that are off").clicked();
            if busy || g.status_job.running() {
                progress(ui, if busy { "Working…" } else { "Reading current settings…" });
            }
        });
        if pick_off {
            for t in &all {
                g.selected[t.index()] = !g.status.get(t.index()).is_some_and(|s| s.0);
            }
        }
        ui.add_space(S3);
        let revert = group(ui, |ui| tweak_rows(ui, g, all.clone(), build));
        ui.add_space(S2);
        ui.horizontal_wrapped(|ui| {
            caption(
                ui,
                "These are the same switches as in Settings, for your account only. Settings that need administrator rights or a company policy (diagnostic data level, Bing results in Start search, Recall) are left to Windows.",
            );
            if link(ui, "Open Privacy & security").clicked() {
                open_uri("ms-settings:privacy");
            }
        });
        if let Some(t) = revert {
            self.start_restore(ui.ctx(), vec![t], false);
        }
        if apply {
            let body = chosen.iter().map(|t| t.info().title).collect::<Vec<_>>().join("\n");
            self.confirm = Some(Confirm {
                title: format!("Apply {} privacy settings?", chosen.len()),
                body: format!("{body}\n\nEach original value is saved first. Undo any time with Revert, or Settings > Restore everything."),
                warning: None,
                ok: "Apply",
                action: Action::ApplyTweaks(chosen),
            });
        }
    }
}
