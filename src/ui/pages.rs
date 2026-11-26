//! The single-area pages of the sidebar: Game Mode, GPU, Network, Controller & input and Tools, plus the status
//! checks they share with Home and Clean up & health.

use eframe::egui::{RichText, Ui};

use super::components::*;
use crate::app::state::{Page, ToolsTab};
use crate::app::App;
use crate::system::open_uri;

impl App {
    pub(crate) fn game_mode_screen(&mut self, ui: &mut Ui) {
        page_header(ui, "Game Mode", "Quiets your PC while you play; everything goes back when the game closes.");
        self.game_mode_page(ui);
    }

    pub(crate) fn gpu_page(&mut self, ui: &mut Ui) {
        page_header(ui, "GPU", "Graphics settings, and what Kestrel found about your graphics card and screen.");
        self.settings_list(ui, Page::Gpu);
        self.page_checks(
            ui,
            Some(&[
                "Graphics driver",
                "Resizable BAR",
                "Hardware-accelerated GPU scheduling",
                "Variable refresh rate and Auto HDR",
                "Display refresh rate",
                "Overlays and other optimizers",
            ]),
        );
    }

    pub(crate) fn network_page(&mut self, ui: &mut Ui) {
        page_header(ui, "Network", "Test your connection, and keep uploads from competing with your games.");
        self.network_test(ui);
        self.settings_list(ui, Page::Network);
        self.page_checks(ui, Some(&["Network connection"]));
    }

    /// Ping, jitter and packet loss to the router and the internet, with what they mean.
    fn network_test(&mut self, ui: &mut Ui) {
        use crate::optimizer::network::{self, Stats};
        let busy = self.net.job.running();
        let mut run = false;
        section(ui, "Connection test", |_| {});
        ui.horizontal(|ui| {
            run = accent_button(ui, !busy, if self.net.last.is_some() { "Test again" } else { "Test connection" }).clicked();
            if busy {
                progress(ui, "Testing (about 4 seconds)…");
            }
        });
        ui.add_space(S2);
        caption(
            ui,
            "Pings your router and Cloudflare's 1.1.1.1. A game's own servers can be farther away; run it while nothing else downloads.",
        );
        if let Some(result) = &self.net.last {
            ui.add_space(S3);
            match result {
                Err(e) => info_bar(ui, Status::Caution, &format!("The test didn't run: {e}"), |_| {}),
                Ok(r) => {
                    let line = |s: &Stats| format!("{:.0} ms ping · {:.1} ms jitter · {:.0}% lost", s.avg_ms, s.jitter_ms, s.loss_pct());
                    group(ui, |ui| {
                        let mut n = 0;
                        if let Some((label, s)) = &r.router {
                            setting_row(ui, n, RichText::new(label).color(TEXT), &line(s), |_| {});
                            n += 1;
                        }
                        setting_row(ui, n, RichText::new(network::INTERNET.1).color(TEXT), &line(&r.internet), |_| {});
                    });
                    ui.add_space(S2);
                    for v in network::verdict(r) {
                        ui.label(RichText::new(v).color(TEXT));
                    }
                }
            }
        }
        if run {
            self.net.job.start(ui.ctx(), network::run);
        }
    }

    pub(crate) fn input_page(&mut self, ui: &mut Ui) {
        page_header(ui, "Controller & input", "Mouse and controller settings.");
        self.settings_list(ui, Page::Input);
    }

    pub(crate) fn tools_page(&mut self, ui: &mut Ui) {
        page_header(ui, "Tools", "Measure what Windows runs, and rule out other software.");
        pivot(ui, &mut self.tools_tab, &[(ToolsTab::Analysis, "System analysis"), (ToolsTab::CleanTest, "Clean test")]);
        match self.tools_tab {
            ToolsTab::Analysis => self.analysis_tab(ui),
            ToolsTab::CleanTest => self.clean_test_tab(ui),
        }
    }

    fn clean_test_tab(&mut self, ui: &mut Ui) {
        let (desc, label) = match self.clean.active {
            Some((s, a)) => (format!("On: {s} services and {a} startup apps are off. Measure your game, then end the test."), "End test"),
            None => (
                "Turns off other programs that start with Windows for one restart, so you can measure your game without them, then puts them back. Windows' own services, anti-cheat, launchers and hardware software stay on.".to_string(),
                "Start…",
            ),
        };
        let mut click = false;
        group(ui, |ui| {
            setting_row(ui, 0, RichText::new("Clean test").color(TEXT), &desc, |ui| {
                click = button(ui, !self.clean.run.running(), label).clicked();
            });
        });
        if click {
            match self.clean.active {
                Some(_) => self.clean.run.start(ui.ctx(), crate::cleantest::end),
                None => self.open_clean_test(ui.ctx()),
            }
        }
    }

    /// The status checks with these titles (all of them for None), with their links and fixes.
    pub(crate) fn page_checks(&mut self, ui: &mut Ui, titles: Option<&[&str]>) {
        let checking = self.gaming.checks_job.running();
        let mut recheck = false;
        section(ui, "Found on this PC", |ui| {
            if checking {
                progress(ui, "Checking");
            } else {
                recheck = link(ui, "Check again").clicked();
            }
        });
        let mut fix = None;
        group(ui, |ui| {
            let shown: Vec<_> = self.gaming.checks.iter().filter(|c| titles.is_none_or(|t| t.contains(&c.title))).collect();
            if shown.is_empty() {
                row(ui, 0, |ui| {
                    if checking || self.gaming.checks.is_empty() {
                        progress(ui, "Checking…");
                    } else {
                        ui.label(RichText::new("Nothing to report here.").color(TEXT_2));
                    }
                });
            }
            for (i, c) in shown.into_iter().enumerate() {
                let lead = if c.attention { Status::Caution } else { Status::Ok };
                setting_row_note(ui, i, Some(lead), RichText::new(c.title).color(TEXT), &c.detail, None, |ui| {
                    if let Some((label, uri)) = c.link {
                        if row_button(ui, true, label).clicked() {
                            open_uri(uri);
                        }
                    }
                    if !c.refresh_fix.is_empty() && row_button(ui, self.gaming.refresh_trial.is_none(), "Fix").clicked() {
                        fix = Some(c.refresh_fix.clone());
                    }
                });
            }
        });
        if recheck {
            self.gaming.checks_job.start(ui.ctx(), crate::optimizer::health::run);
        }
        if let Some(fixes) = fix {
            self.try_refresh_fix(fixes);
        }
    }
}
