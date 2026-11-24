//! Windows > System analysis: this PC's counts now against a saved baseline, what the change means, and what other
//! optimizers or earlier changes have already done.

use eframe::egui::{RichText, Ui};

use super::components::*;
use crate::app::App;
use crate::optimizer::analysis::{self, Snapshot};

/// (name, what it means, how to read it from a snapshot, unit).
type Metric = (&'static str, &'static str, fn(&Snapshot) -> u64, Unit);

#[derive(Clone, Copy)]
enum Unit {
    Count,
    Megabytes,
    Permille,
}

impl Unit {
    fn format(self, v: u64) -> String {
        match self {
            Unit::Count => v.to_string(),
            Unit::Megabytes => format!("{:.1} GB", v as f64 / 1024.0),
            Unit::Permille => format!("{:.1}%", v as f64 / 10.0),
        }
    }
}

impl App {
    pub(crate) fn analysis_tab(&mut self, ui: &mut Ui) {
        let a = &mut self.analysis;
        if !a.loaded {
            a.loaded = true;
            a.baseline = analysis::baseline();
            a.job.start(ui.ctx(), || (analysis::take(), analysis::findings()));
        }
        let busy = a.job.running();
        let (mut again, mut save) = (false, false);
        ui.horizontal(|ui| {
            again = accent_button(ui, !busy, "Measure again").clicked();
            save = button(ui, !busy && a.now.is_some(), "Save as baseline")
                .on_hover_text("Keep this measurement to compare with after you change things")
                .clicked();
            if busy {
                progress(ui, "Measuring (about two seconds)…");
            }
        });
        ui.add_space(S2);
        caption(ui, "For numbers you can compare, measure a few minutes after starting Windows with nothing else open.");
        ui.add_space(S3);

        let date = |s: &Snapshot| crate::upgrade::drivers::date_from_unix_days((s.when / 86_400) as i64);
        let base_label = a.baseline.as_ref().map_or("Baseline".to_string(), |b| format!("Baseline ({})", date(b)));
        group(ui, |ui| {
            table_header(ui, &[("", 0.0, false), (&base_label, 130.0, true), ("Now", 90.0, true), ("Change", 90.0, true)]);
            let Some(now) = &a.now else {
                row(ui, 0, |ui| progress(ui, "Measuring…"));
                return;
            };
            let rows: [Metric; 9] = [
                ("Processes", "Programs and service hosts running", |s| s.processes.into(), Unit::Count),
                ("Service hosts (svchost)", "Fewer can mean grouping, not less work", |s| s.svchost.into(), Unit::Count),
                ("Running services", "In service hosts, and in their own programs", |s| s.services.into(), Unit::Count),
                ("Threads", "Units of work that can run", |s| s.threads.into(), Unit::Count),
                ("Handles", "Open files, keys and objects", |s| s.handles.into(), Unit::Count),
                ("Memory in use", "Includes Windows' file cache", |s| s.ram_mb, Unit::Megabytes),
                ("Background CPU", "Over one second", |s| s.cpu_permille.into(), Unit::Permille),
                ("Startup apps on", "Start when you sign in", |s| s.startup.into(), Unit::Count),
                ("Scheduled tasks on", "Windows' own included", |s| s.tasks.into(), Unit::Count),
            ];
            for (i, (name, what, get, unit)) in rows.iter().enumerate() {
                let fmt = |v: u64| unit.format(v);
                row(ui, i + 1, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(*name).color(TEXT));
                            caption(ui, *what);
                        });
                        let now_v = get(now);
                        let base_v = a.baseline.as_ref().map(get);
                        ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                            let change = base_v.map(|b| now_v as i64 - b as i64).filter(|d| *d != 0);
                            cell(ui, 90.0, true, |ui| match change {
                                Some(d) if d < 0 => ui.label(RichText::new(format!("−{}", fmt(d.unsigned_abs()))).color(TEXT)),
                                Some(d) => ui.label(RichText::new(format!("+{}", fmt(d as u64))).color(TEXT_2)),
                                None => ui.label(RichText::new("–").color(TEXT_3)),
                            });
                            cell(ui, 90.0, true, |ui| ui.label(RichText::new(fmt(now_v)).color(TEXT)));
                            cell(ui, 130.0, true, |ui| ui.label(RichText::new(base_v.map_or("–".to_string(), &fmt)).color(TEXT_2)));
                        });
                    });
                });
            }
        });
        if let (Some(b), Some(n)) = (&a.baseline, &a.now) {
            if b.when != n.when {
                ui.add_space(S3);
                group(ui, |ui| {
                    for (i, line) in analysis::verdict(b, n).iter().enumerate() {
                        row(ui, i, |ui| {
                            ui.label(RichText::new(line).color(TEXT));
                        });
                    }
                });
            }
        }

        section(ui, "Found on this PC", |_| {});
        group(ui, |ui| {
            if a.findings.is_empty() {
                row(ui, 0, |ui| progress(ui, "Checking…"));
            }
            for (i, f) in a.findings.iter().enumerate() {
                setting_row_note(
                    ui,
                    i,
                    Some(if f.risk { Status::Caution } else { Status::Ok }),
                    RichText::new(f.title).color(TEXT),
                    &f.detail,
                    None,
                    |_| {},
                );
            }
        });
        ui.add_space(S2);
        caption(
            ui,
            "Fewer processes isn't the goal. To see whether a change helped a game, measure the game itself (Games > a game > Measure).",
        );

        if again {
            a.job.start(ui.ctx(), || (analysis::take(), analysis::findings()));
        }
        if save {
            if let Some(n) = a.now {
                match analysis::save_baseline(&n) {
                    Ok(()) => a.baseline = Some(n),
                    Err(e) => self.show_error("System analysis", e),
                }
            }
        }
    }
}
