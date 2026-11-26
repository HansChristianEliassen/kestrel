//! Privacy page: per-user Windows privacy switches, through the same reversible tweak system as Gaming.

use eframe::egui::Ui;

use super::components::*;
use super::tweak_rows;
use crate::app::App;
use crate::optimizer::Tweak;
use crate::system::open_uri;

impl App {
    pub(crate) fn privacy_page(&mut self, ui: &mut Ui) {
        let build = self.build;
        let g = &mut self.gaming;
        let all: Vec<Tweak> = Tweak::ALL.into_iter().filter(|t| t.is_privacy()).collect();
        if g.apply.running() || g.status_job.running() {
            progress(ui, if g.apply.running() { "Working…" } else { "Reading current settings…" });
        }
        ui.add_space(S3);
        let flipped = group(ui, |ui| tweak_rows(ui, g, all, build));
        ui.add_space(S2);
        ui.horizontal_wrapped(|ui| {
            caption(ui, "The same switches as in Settings, for your account only.");
            if link(ui, "Open Privacy & security").clicked() {
                open_uri("ms-settings:privacy");
            }
        });
        if let Some((t, on)) = flipped {
            self.set_tweak(ui.ctx(), t, on);
        }
    }
}
