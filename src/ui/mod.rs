//! Theme, navigation and the screens. Every screen is built from the components in `components`.

pub mod components;
mod game_settings;
mod gamemode;
mod measure;
mod privacy;
mod updates;
mod upgrades;

use std::path::Path;

use eframe::egui::{self, vec2, Align, Align2, Color32, CornerRadius, FontId, Label, Layout, RichText, Sense, Stroke, TextStyle, Ui};

use crate::app::state::{Action, AppsTab, CleanupTab, Confirm, GamingState, GamingTab, OptStage, Page, SortBy};
use crate::app::{preset_selection, App};
use crate::cleaner::Kind;
use crate::optimizer::games::Game;
use crate::optimizer::gaming::GameSetting;
use crate::optimizer::{Preset, Tweak};
use crate::system::{open_uri, pick_exe, relaunch_as_admin, show_in_folder};
use crate::uninstaller::{AppSource, InstalledApp};
use crate::utils::{data_dir, logging::read_history};
pub use components::fmt_bytes;

/// Licenses of the libraries compiled into Kestrel (regenerate with scripts/third-party-notices.ps1).
const THIRD_PARTY_NOTICES: &str = include_str!("../../THIRD-PARTY-NOTICES.txt");
use components::*;

pub fn apply_theme(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.text_styles = [
        (TextStyle::Heading, semibold(TITLE)),
        (TextStyle::Body, FontId::proportional(BODY)),
        (TextStyle::Button, FontId::proportional(BODY)),
        (TextStyle::Small, FontId::proportional(CAPTION)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
    ]
    .into();
    let v = &mut style.visuals;
    v.panel_fill = BG;
    v.window_fill = LAYER;
    v.extreme_bg_color = Color32::from_rgb(0x1c, 0x1c, 0x1c);
    v.faint_bg_color = LAYER;
    v.hyperlink_color = ACCENT_TEXT;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.5);
    v.selection.stroke = Stroke::new(1.0_f32, TEXT);
    v.window_corner_radius = CornerRadius::same(R_DIALOG);
    v.menu_corner_radius = CornerRadius::same(R_DIALOG);
    v.window_stroke = Stroke::new(1.0_f32, STROKE);
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(100) };
    v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(80) };
    v.widgets.noninteractive.bg_fill = LAYER;
    v.widgets.noninteractive.weak_bg_fill = LAYER;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, DIVIDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    // Standard controls: soft filled surfaces with a hairline edge (Windows 11 / Linear style), no heavy outlines.
    // "active" also covers keyboard focus, so its stroke doubles as the focus indicator.
    let edge = Stroke::new(1.0_f32, Color32::from_rgb(0x3e, 0x3e, 0x3e));
    for (w, fill, stroke) in [
        (&mut v.widgets.inactive, CONTROL, edge),
        (&mut v.widgets.hovered, CONTROL_HOVER, Stroke::new(1.0_f32, Color32::from_rgb(0x48, 0x48, 0x48))),
        (&mut v.widgets.active, Color32::from_rgb(0x2e, 0x2e, 0x2e), Stroke::new(1.5_f32, TEXT_2)),
        (&mut v.widgets.open, CONTROL_HOVER, Stroke::new(1.0_f32, Color32::from_rgb(0x48, 0x48, 0x48))),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = stroke;
        w.fg_stroke = Stroke::new(1.0_f32, TEXT);
        w.corner_radius = CornerRadius::same(R_CONTROL);
        w.expansion = 0.0;
    }
    // Text inputs: slightly darker than the page, hairline edge (egui fades disabled widgets toward the background).
    v.extreme_bg_color = Color32::from_rgb(0x1b, 0x1b, 0x1b);
    style.spacing.item_spacing = vec2(S2, S2);
    style.spacing.button_padding = vec2(S3 + 2.0, S1);
    style.spacing.interact_size = vec2(40.0, CONTROL_H);
    style.spacing.combo_width = 160.0;
    let mut scroll = egui::style::ScrollStyle::thin();
    // Keep the scrollbar off the lists' right edge.
    scroll.bar_outer_margin = 4.0;
    style.spacing.scroll = scroll;
    style.animation_time = 0.08;
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// Sidebar logo size; the bird fills about two thirds of it.
const BRAND: f32 = 36.0;

fn nav_item(ui: &mut Ui, page: Page, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, page.label()));
    let painter = ui.painter();
    if selected || resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(R_CONTROL + 2), if selected { LAYER } else { Color32::from_rgb(0x24, 0x24, 0x24) });
    }
    if selected {
        let pill = egui::Rect::from_center_size(egui::pos2(rect.left() + 1.5, rect.center().y), vec2(3.0, 14.0));
        painter.rect_filled(pill, CornerRadius::same(2), ACCENT);
    }
    let color = if selected { TEXT } else { TEXT_2 };
    // Icons in a fixed 16 px box, labels on one baseline.
    painter.text(egui::pos2(rect.left() + 22.0, rect.center().y), Align2::CENTER_CENTER, page.icon(), icon_font(16.0), color);
    let font = if selected { semibold(BODY) } else { FontId::proportional(BODY) };
    painter.text(rect.left_center() + vec2(42.0, 0.0), Align2::LEFT_CENTER, page.label(), font, color);
    if resp.has_focus() {
        painter.rect_stroke(rect.shrink(1.0_f32), CornerRadius::same(R_CONTROL), Stroke::new(2.0_f32, TEXT), egui::StrokeKind::Inside);
    }
    resp
}

impl App {
    pub(crate) fn sidebar(&mut self, ui: &mut Ui) {
        let brand = self
            .brand
            .get_or_insert_with(|| {
                // Just the bird (no tile), drawn at its exact on-screen pixel size so it stays sharp.
                let n = (BRAND * ui.ctx().pixels_per_point()).round() as usize;
                let image = egui::ColorImage::from_rgba_unmultiplied([n, n], &crate::logo::rgba(n, false));
                ui.ctx().load_texture("brand", image, egui::TextureOptions::LINEAR)
            })
            .clone();
        ui.horizontal(|ui| {
            // Page icons start 14 px into their row and labels at 42 px; the bird starts a quarter into its square.
            let row = ui.cursor().left();
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(14.0 - BRAND * 0.25);
            ui.add(egui::Image::new(&brand).fit_to_exact_size(vec2(BRAND, BRAND)));
            // Painted like the page names (nav_item), so it starts exactly where they do.
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), BRAND), Sense::hover());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Kestrel"));
            ui.painter().text(egui::pos2(row + 42.0, rect.center().y), Align2::LEFT_CENTER, "Kestrel", semibold(15.0), TEXT);
        });
        ui.add_space(S4 + 2.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let mut nav = None;
        // Three quiet groups: the overview, the system tools, and the gaming pages.
        for (i, p) in Page::ALL.into_iter().filter(|p| *p != Page::Settings).enumerate() {
            if matches!(p, Page::Cleaner | Page::Gaming) && i > 0 {
                ui.add_space(S3);
            }
            if nav_item(ui, p, self.page == p).clicked() {
                nav = Some(p);
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let (kind, text) = if self.admin { (Status::Ok, "Administrator") } else { (Status::Neutral, "Standard user") };
                status(ui, kind, text);
            });
            if self.gamemode.on.is_some() {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    status(ui, Status::Ok, "Game Mode on");
                });
            }
            ui.add_space(S2);
            if nav_item(ui, Page::Settings, self.page == Page::Settings).clicked() {
                nav = Some(Page::Settings);
            }
        });
        if let Some(p) = nav {
            self.go(p, ui.ctx());
        }
    }

    pub(crate) fn dashboard(&mut self, ui: &mut Ui) {
        page_header(ui, "Overview", "Clean up, manage apps and tune Windows for games. Nothing changes until you confirm it.");
        let ctx = ui.ctx().clone();
        let (mut optimize, mut scan) = (false, false);
        // The page's one primary action, on its own panel.
        egui::Frame::default()
            .fill(LAYER)
            .stroke(Stroke::new(1.0_f32, STROKE))
            .corner_radius(CornerRadius::same(R_GROUP))
            .inner_margin(egui::Margin { left: 20, right: 16, top: 16, bottom: 16 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(RichText::new("Set up this PC for gaming").font(semibold(16.0)).color(TEXT));
                        ui.label(
                            RichText::new(
                                "Finds what slows games on this PC and fixes it in one go. You see the list first; all can be undone.",
                            )
                            .size(CAPTION + 1.0)
                            .color(TEXT_2),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        optimize = accent_button(ui, self.opt.stage == OptStage::Closed, "Start").clicked();
                    });
                });
            });

        section(ui, "Quick actions", |_| {});
        let found: u64 = self.cleaner.items.iter().map(|i| i.found.bytes).sum();
        let cleanup = if self.cleaner.items.is_empty() {
            "Temporary files, caches and other leftovers. Nothing is deleted until you confirm.".to_string()
        } else {
            format!("{} found in the last scan. Nothing is deleted until you confirm.", fmt_bytes(found))
        };
        group(ui, |ui| {
            setting_row(ui, 0, RichText::new("Clean up").color(TEXT), &cleanup, |ui| {
                scan = button(ui, !self.cleaner.scan.running(), if self.cleaner.items.is_empty() { "Scan" } else { "Review" }).clicked();
            });
            self.game_mode_row(ui, 1);
        });
        if optimize {
            self.start_optimize(&ctx);
        }
        if scan {
            self.go(Page::Cleaner, &ctx);
            if self.cleaner.items.is_empty() {
                self.start_scan(&ctx);
            }
        }

        let checking = self.gaming.checks_job.running();
        let attention = self.gaming.checks.iter().filter(|c| c.attention).count();
        let mut recheck = false;
        section(ui, "Status", |ui| {
            if checking {
                progress(ui, "Checking");
            } else {
                recheck = link(ui, "Check again").clicked();
            }
        });
        if recheck {
            self.gaming.checks_job.start(&ctx, crate::optimizer::health::run);
        }
        let mut fix = None;
        let fine = self.gaming.checks.len() - attention;
        let show_all = self.show_ok_checks;
        let mut toggle_ok = false;
        group(ui, |ui| {
            if self.gaming.checks.is_empty() {
                row(ui, 0, |ui| progress(ui, "Checking display, drivers, memory, storage and network…"));
            }
            // Only what needs attention by default; the rest is one line away.
            let shown = self.gaming.checks.iter().filter(|c| c.attention || show_all);
            for (i, c) in shown.enumerate() {
                let lead = if c.attention { Status::Caution } else { Status::Ok };
                setting_row_note(ui, i, Some(lead), RichText::new(c.title).color(TEXT), &c.detail, None, |ui| {
                    if let Some((label, uri)) = c.link {
                        if row_button(ui, true, label).clicked() {
                            open_uri(uri);
                        }
                    }
                    if !c.refresh_fix.is_empty()
                        && row_button(ui, self.gaming.refresh_trial.is_none(), "Fix")
                            .on_hover_text("Switches to the highest refresh rate at your current resolution. It switches back after 15 seconds unless you keep it.")
                            .clicked()
                    {
                        fix = Some(c.refresh_fix.clone());
                    }
                });
            }
            if fine > 0 {
                let n = if show_all { self.gaming.checks.len() } else { attention };
                let text = match (show_all, attention) {
                    (true, _) => "Hide the checks that are fine".to_string(),
                    (false, 0) => format!("All {fine} checks look fine"),
                    (false, _) => format!("{fine} other checks look fine"),
                };
                row(ui, n, |ui| {
                    ui.horizontal(|ui| {
                        if !show_all {
                            status(ui, Status::Ok, "");
                        }
                        ui.label(RichText::new(text).color(TEXT_2));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            toggle_ok = link(ui, if show_all { "Hide" } else { "Show" }).clicked();
                        });
                    });
                });
            }
        });
        if toggle_ok {
            self.show_ok_checks = !self.show_ok_checks;
        }
        if let Some(fixes) = fix {
            self.try_refresh_fix(fixes);
        }
        ui.add_space(S2);
        caption(ui, "Status checks only read your settings. Only Fix changes anything, and only when you click it.");
    }

    pub(crate) fn cleaner_page(&mut self, ui: &mut Ui) {
        page_header(ui, "Cleanup", "Scanning only looks. Files are deleted after you choose categories and confirm.");
        pivot(
            ui,
            &mut self.cleaner.tab,
            &[(CleanupTab::Junk, "Junk files"), (CleanupTab::Large, "Large files"), (CleanupTab::Usage, "Disk usage")],
        );
        if self.cleaner.tab == CleanupTab::Usage {
            self.disk_usage_tab(ui);
            return;
        }
        if self.cleaner.tab == CleanupTab::Large {
            self.large_files_tab(ui);
            return;
        }
        let (scanning, cleaning) = (self.cleaner.scan.running(), self.cleaner.clean.running());
        let scanned = !self.cleaner.items.is_empty();
        let selected: Vec<_> = self.cleaner.items.iter().filter(|i| i.selected).collect();
        let sel_bytes: u64 = selected.iter().map(|i| i.found.bytes).sum();
        let (mut scan, mut clean) = (false, false);
        // Before the first scan the Scan button lives in the empty panel below; the toolbar is for results.
        if scanned || cleaning {
            ui.horizontal(|ui| {
                scan = button(ui, !scanning && !cleaning, "Scan again").clicked();
                if scanning || cleaning {
                    progress(ui, if scanning { "Scanning temp folders and caches…" } else { "Cleaning…" });
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    clean = accent_button(ui, !scanning && !cleaning && !selected.is_empty(), "Clean selected").clicked();
                    secondary(ui, format!("{} selected", fmt_bytes(sel_bytes)));
                });
            });
            ui.add_space(S3);
        }
        if clean {
            let body = selected
                .iter()
                .map(|i| format!("{}: {} items, {}", i.cat.name, i.found.count, fmt_bytes(i.found.bytes)))
                .collect::<Vec<_>>()
                .join("\n");
            self.confirm = Some(Confirm {
                title: format!("Delete {}?", fmt_bytes(sel_bytes)),
                body: format!("{body}\n\nFiles in use are skipped. Deleted files don't go to the Recycle Bin."),
                warning: selected.iter().find_map(|i| i.cat.warning).map(String::from),
                ok: "Delete",
                action: Action::Clean,
            });
        }
        group(ui, |ui| {
            if !scanned {
                if scanning {
                    row(ui, 0, |ui| progress(ui, "Looking through temp folders and caches…"));
                } else {
                    setting_row(
                        ui,
                        0,
                        RichText::new("Nothing scanned yet").color(TEXT),
                        "Scan to see how much space temporary files, caches and other leftovers are using. Scanning only looks.",
                        |ui| scan = accent_button(ui, true, "Scan").clicked(),
                    );
                }
                return;
            }
            table_header(ui, &[("Category", 0.0, false), ("Items", 80.0, true), ("Size", 90.0, true)]);
            for (i, item) in self.cleaner.items.iter_mut().enumerate() {
                row(ui, i + 1, |ui| {
                    let has = item.found.count > 0;
                    if !has {
                        item.selected = false;
                    }
                    ui.horizontal(|ui| {
                        let fill = ui.available_width() - 80.0 - 90.0 - 2.0 * S2;
                        cell(ui, fill, false, |ui| ui.add_enabled_ui(has, |ui| checkbox(ui, &mut item.selected, item.cat.name)));
                        // The component store's size is only known after DISM has cleaned it.
                        let unknown = item.cat.kind == Kind::ComponentStore;
                        let unit = if item.cat.kind == Kind::RecycleBin { "items" } else { "files" };
                        cell(ui, 80.0, true, |ui| {
                            caption(ui, if unknown { String::new() } else { format!("{} {unit}", item.found.count) })
                        });
                        cell(ui, 90.0, true, |ui| {
                            let size = if unknown && has { "Varies".to_string() } else { fmt_bytes(item.found.bytes) };
                            ui.label(RichText::new(size).color(if has { TEXT } else { TEXT_3 }))
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(26.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(item.cat.desc).size(CAPTION).color(TEXT_2));
                            for n in &item.found.notes {
                                caption(ui, n);
                            }
                            if let Some(w) = item.cat.warning {
                                ui.label(RichText::new(w).size(CAPTION).color(CAUTION));
                            }
                            if !item.found.files.is_empty() {
                                let label = if item.show_files { "Hide files" } else { "View files" };
                                if link(ui, label).clicked() {
                                    item.show_files = !item.show_files;
                                }
                            }
                            if item.show_files {
                                const SHOWN: usize = 200;
                                egui::ScrollArea::vertical().id_salt(("files", i)).max_height(180.0).show(ui, |ui| {
                                    for f in item.found.files.iter().take(SHOWN) {
                                        ui.add(Label::new(RichText::new(f.to_string_lossy()).size(CAPTION).color(TEXT_3)).truncate())
                                            .on_hover_text(f.to_string_lossy());
                                    }
                                });
                                if item.found.files.len() > SHOWN {
                                    caption(ui, format!("…and {} more files.", item.found.files.len() - SHOWN));
                                }
                            }
                        });
                    });
                });
            }
        });
        if scan {
            self.start_scan(ui.ctx());
        }
    }

    /// What's using space: the folders and files directly inside a folder, largest first, with drill-down.
    fn disk_usage_tab(&mut self, ui: &mut Ui) {
        let c = &mut self.cleaner;
        let busy = c.usage_job.running();
        let mut open: Option<std::path::PathBuf> = None;
        ui.horizontal(|ui| {
            for (letter, _, free, total) in crate::optimizer::health::drives() {
                let label = format!("{letter}:  {} free of {}", fmt_bytes(free), fmt_bytes(total));
                if button(ui, !busy, &label).clicked() {
                    open = Some(std::path::PathBuf::from(format!(r"{letter}:\")));
                }
            }
        });
        ui.add_space(S3);
        match &c.usage_dir {
            None => {
                group(ui, |ui| {
                    empty_state(
                        ui,
                        "Choose a drive",
                        "Kestrel adds up every folder on it and lists the largest. Only looks; nothing is changed.",
                    );
                });
                if let Some(p) = open {
                    self.start_usage(ui.ctx(), p);
                }
                return;
            }
            Some(dir) => {
                ui.horizontal(|ui| {
                    if let Some(parent) = dir.parent() {
                        if button(ui, !busy, "Up").clicked() {
                            open = Some(parent.to_path_buf());
                        }
                    }
                    ui.label(RichText::new(dir.to_string_lossy()).color(TEXT));
                });
            }
        }
        ui.add_space(S2);
        let total: u64 = c.usage.iter().map(|e| e.1).sum();
        let largest = c.usage.first().map_or(1, |e| e.1.max(1));
        const SHOWN: usize = 100;
        group(ui, |ui| {
            if busy {
                row(ui, 0, |ui| progress(ui, "Adding up every folder inside. Large drives take a minute…"));
            } else if c.usage.is_empty() {
                empty_state(ui, "Nothing here", "This folder is empty, or Windows didn't let Kestrel read it.");
            }
            for (i, (path, bytes, is_dir)) in c.usage.iter().take(SHOWN).enumerate() {
                row(ui, i, |ui| {
                    ui.horizontal(|ui| {
                        let name = path.file_name().map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy());
                        let text = RichText::new(name).color(TEXT);
                        cell(ui, (ui.available_width() - 360.0).max(120.0), false, |ui| {
                            ui.add(Label::new(if *is_dir { text.strong() } else { text }).truncate()).on_hover_text(path.to_string_lossy());
                        });
                        // Share of the largest entry, so the biggest space users stand out.
                        cell(ui, 120.0, false, |ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(110.0, 6.0), Sense::hover());
                            ui.painter().rect_filled(r, CornerRadius::same(3), CONTROL);
                            let w = r.width() * (*bytes as f32 / largest as f32);
                            ui.painter().rect_filled(egui::Rect::from_min_size(r.min, vec2(w, r.height())), CornerRadius::same(3), ACCENT);
                        });
                        cell(ui, 80.0, true, |ui| caption(ui, fmt_bytes(*bytes)));
                        cell(ui, 140.0, true, |ui| {
                            if row_button(ui, true, "Show").on_hover_text("Show in File Explorer").clicked() {
                                show_in_folder(path);
                            }
                            if *is_dir && row_button(ui, !busy, "Open").clicked() {
                                open = Some(path.clone());
                            }
                        });
                    });
                });
            }
        });
        ui.add_space(S2);
        if busy {
            return;
        }
        let more = c.usage.len().saturating_sub(SHOWN);
        caption(
            ui,
            format!(
                "{} in this folder{}. Folders Windows doesn't let Kestrel read count as empty; running as administrator sees more. Links are never followed, so nothing is counted twice.",
                fmt_bytes(total),
                if more > 0 { format!(", plus {more} smaller items not listed") } else { String::new() }
            ),
        );
        if let Some(p) = open {
            self.start_usage(ui.ctx(), p);
        }
    }

    pub(crate) fn start_usage(&mut self, ctx: &egui::Context, dir: std::path::PathBuf) {
        let c = &mut self.cleaner;
        if c.usage_job.running() {
            return;
        }
        c.usage_dir = Some(dir.clone());
        c.usage.clear();
        c.usage_job.start(ctx, move || crate::cleaner::usage::list(&dir));
    }

    fn large_files_tab(&mut self, ui: &mut Ui) {
        let searching = self.cleaner.large_job.running();
        let mut search = false;
        ui.horizontal(|ui| {
            let label = if self.cleaner.large.is_some() { "Search again" } else { "Find large files" };
            search = button(ui, !searching, label).clicked();
            if searching {
                progress(ui, "Looking through your folders…");
            }
        });
        if search {
            self.cleaner.large_job.start(ui.ctx(), crate::cleaner::large::find);
        }
        ui.add_space(S3);
        group(ui, |ui| match &self.cleaner.large {
            None => empty_state(
                ui,
                "Not searched yet",
                &format!(
                    "Lists files of {} or more in Downloads, Desktop, Documents, Videos, Pictures and Music. Nothing is deleted.",
                    fmt_bytes(crate::cleaner::large::MIN_SIZE)
                ),
            ),
            Some(files) if files.is_empty() => {
                empty_state(ui, "No large files", "Nothing that big in your personal folders.");
            }
            Some(files) => {
                for (i, (path, size)) in files.iter().enumerate() {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let folder = path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                    setting_row(ui, i, RichText::new(name).color(TEXT), &format!("{} · {folder}", fmt_bytes(*size)), |ui| {
                        if row_button(ui, true, "Show in folder").clicked() {
                            show_in_folder(path);
                        }
                    });
                }
            }
        });
        ui.add_space(S2);
        caption(
            ui,
            "Kestrel only lists these. Open the folder to move or delete files yourself; deleting there sends them to the Recycle Bin.",
        );
    }
}

/// One confirmation for one or several apps, naming each and what removal means.
fn uninstall_confirm(apps: Vec<InstalledApp>) -> Confirm {
    let unknown = |s: &str| if s.is_empty() { "Unknown".to_string() } else { s.to_string() };
    let dev: Vec<&str> = apps.iter().filter(|a| a.developer).map(|a| a.name.as_str()).collect();
    let warning = (!dev.is_empty()).then(|| {
        format!(
            "{} {} a developer tool. Editors, build scripts and other programs that rely on it may stop working without it.",
            dev.join(", "),
            if dev.len() == 1 { "is" } else { "are" }
        )
    });
    let (title, body) = if let [app] = &apps[..] {
        let consequence = match (&app.source, app.optional) {
            (AppSource::Store { .. }, true) => "Microsoft preinstalls this app. You can reinstall it from the Microsoft Store at any time.",
            (AppSource::Store { .. }, false) => "The app and its local data are removed for your account. You can reinstall it from the Microsoft Store.",
            _ => "The app's own uninstaller decides what is removed. Settings and data kept in its install folder (some games keep saves there) may be deleted.",
        };
        (
            format!("Uninstall {}?", app.name),
            format!(
                "Publisher: {}\nVersion: {}\n\n{consequence}\n\nKestrel starts the app's official uninstaller. Follow its window to finish.",
                unknown(&app.publisher),
                unknown(&app.version)
            ),
        )
    } else {
        let list: Vec<String> = apps
            .iter()
            .map(|a| {
                format!(
                    "• {} ({}{})",
                    a.name,
                    unknown(&a.publisher),
                    if matches!(a.source, AppSource::Store { .. }) { ", Store" } else { "" }
                )
            })
            .collect();
        (
            format!("Uninstall {} apps?", apps.len()),
            format!(
                "{}\n\nKestrel starts each app's official uninstaller, one after another. Follow each window to finish; you can skip one with Stop waiting or cancel the rest. Store apps are removed for your account without a window. Each uninstaller decides what is removed, and data in an install folder (some games keep saves there) may be deleted.",
                list.join("\n")
            ),
        )
    };
    Confirm { title, body, warning, ok: "Uninstall", action: Action::Uninstall(apps) }
}

impl App {
    pub(crate) fn apps_page(&mut self, ui: &mut Ui) {
        page_header(ui, "Apps", "Uninstall programs and Store apps with their own uninstaller, or get new ones. You confirm every change.");
        pivot(ui, &mut self.apps.tab, &[(AppsTab::Installed, "Installed apps"), (AppsTab::Get, "Get apps")]);
        if self.apps.tab == AppsTab::Get {
            self.get_apps_tab(ui);
            return;
        }
        let busy = self.apps.load.running() || self.apps.uninstall.running();
        let mut refresh = false;
        ui.horizontal(|ui| {
            search_box(ui, &mut self.apps.search, "Search by name or publisher", 260.0);
            egui::ComboBox::from_id_salt("sort")
                .selected_text(if self.apps.sort == SortBy::Name { "Sort by name" } else { "Sort by size" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.apps.sort, SortBy::Name, "Sort by name");
                    ui.selectable_value(&mut self.apps.sort, SortBy::Size, "Sort by size");
                });
            checkbox(ui, &mut self.apps.optional_only, "Preinstalled extras only");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.apps.load.running() {
                    progress(ui, "Loading");
                } else {
                    refresh = link(ui, "Refresh").clicked();
                }
            });
        });
        if refresh {
            self.load_apps(ui.ctx());
        }
        let mut bulk = false;
        if !self.apps.selected.is_empty() && !busy {
            ui.add_space(S2);
            let n = self.apps.selected.len();
            info_bar(ui, Status::Neutral, &format!("{n} app{} selected.", if n == 1 { "" } else { "s" }), |ui| {
                bulk = row_button(ui, true, "Uninstall selected").clicked();
                if row_button(ui, true, "Clear").clicked() {
                    self.apps.selected.clear();
                }
            });
        }
        if self.apps.uninstall.running() {
            use std::sync::atomic::Ordering::Relaxed;
            ui.add_space(S2);
            let (mut stop, mut cancel) = (false, false);
            let i = self.apps.queue_pos.load(Relaxed);
            let name = self.apps.queue.get(i).cloned().unwrap_or_default();
            let text = if self.apps.queue.len() > 1 {
                format!("Uninstalling {} of {}: {name}. Follow its window to complete the removal.", i + 1, self.apps.queue.len())
            } else {
                format!("Waiting for the uninstaller of {name} to finish. Follow its window to complete the removal.")
            };
            let more = self.apps.queue.len() > i + 1 && !self.apps.cancel_queue.load(Relaxed);
            info_bar(ui, Status::Neutral, &text, |ui| {
                stop = row_button(ui, true, "Stop waiting").on_hover_text("Use this if you cancelled the uninstaller.").clicked();
                if more {
                    cancel = row_button(ui, true, "Cancel the rest").on_hover_text("No more uninstallers start after this one.").clicked();
                }
            });
            if stop {
                self.apps.stop_waiting.store(true, Relaxed);
            }
            if cancel {
                self.apps.cancel_queue.store(true, Relaxed);
            }
        }
        if let Some(n) = &self.apps.note {
            ui.add_space(S2);
            info_bar(ui, Status::Caution, n, |_| {});
        }
        let mut dismiss = None;
        for (n, (name, folders)) in self.apps.leftovers.iter().enumerate() {
            section(ui, &format!("Left behind by {name}"), |ui| {
                if link(ui, "Dismiss").clicked() {
                    dismiss = Some(n);
                }
            });
            group(ui, |ui| {
                for (i, (path, size)) in folders.iter().enumerate() {
                    setting_row(ui, i, RichText::new(path.to_string_lossy()).color(TEXT), &fmt_bytes(*size), |ui| {
                        if row_button(ui, true, "Open").clicked() {
                            open_uri(&path.to_string_lossy());
                        }
                    });
                }
            });
        }
        if !self.apps.leftovers.is_empty() {
            ui.add_space(S2);
            caption(ui, "Kestrel never deletes these. Some apps keep settings or saved games here, so check each folder in File Explorer and delete only what you don't need.");
        }
        if let Some(n) = dismiss {
            self.apps.leftovers.remove(n);
        }

        let q = self.apps.search.to_lowercase();
        let mut shown: Vec<&InstalledApp> = self
            .apps
            .apps
            .iter()
            .filter(|a| !self.apps.optional_only || a.optional)
            .filter(|a| q.is_empty() || a.name.to_lowercase().contains(&q) || a.publisher.to_lowercase().contains(&q))
            .collect();
        if self.apps.sort == SortBy::Size {
            shown.sort_by_key(|a| std::cmp::Reverse(a.size.unwrap_or(0)));
        }
        ui.add_space(S3);
        let mut target: Option<Vec<InstalledApp>> = None;
        if bulk {
            // Selection order follows the full list (by name), whatever the current filter shows.
            let picked: Vec<InstalledApp> =
                self.apps.apps.iter().filter(|a| self.apps.selected.contains(&a.id()) && a.blocked().is_none()).cloned().collect();
            target = (!picked.is_empty()).then_some(picked);
        }
        let selected = &self.apps.selected;
        let mut toggled = None;
        group(ui, |ui| {
            if shown.is_empty() {
                if self.apps.load.running() {
                    row(ui, 0, |ui| progress(ui, "Reading installed programs and Store apps…"));
                } else {
                    empty_state(
                        ui,
                        "No apps found",
                        if q.is_empty() { "Nothing matches the current filter." } else { "No app name or publisher matches your search." },
                    );
                }
                return;
            }
            const PUBLISHER: f32 = 190.0;
            const VERSION: f32 = 110.0;
            const SIZE: f32 = 76.0;
            const ACTION: f32 = 88.0;
            table_header(
                ui,
                &[
                    ("Name", 0.0, false),
                    ("Publisher", PUBLISHER, false),
                    ("Version", VERSION, false),
                    ("Size", SIZE, true),
                    ("", ACTION, true),
                ],
            );
            for (i, app) in shown.iter().enumerate() {
                row(ui, i + 1, |ui| {
                    ui.horizontal(|ui| {
                        let fill = ui.available_width() - PUBLISHER - VERSION - SIZE - ACTION - 4.0 * S2;
                        cell(ui, fill, false, |ui| {
                            let tags: Vec<&str> = [
                                (matches!(app.source, AppSource::Store { .. }), "Store"),
                                (app.optional, "Preinstalled"),
                                (app.protected.is_some(), "Protected"),
                                (app.developer, "Developer tool"),
                            ]
                            .into_iter()
                            .filter_map(|(on, t)| on.then_some(t))
                            .collect();
                            // Leave room for the tags so a long name truncates instead of pushing them out.
                            let tag_w: f32 = tags.iter().map(|t| t.len() as f32 * 6.0 + 20.0).sum();
                            let id = app.id();
                            let mut ticked = selected.contains(&id);
                            let blocked = app.blocked();
                            let r = ui.add_enabled_ui(blocked.is_none() && !busy, |ui| checkbox(ui, &mut ticked, "")).inner;
                            if r.on_disabled_hover_text(blocked.unwrap_or("Wait for the current uninstall to finish.")).changed() {
                                toggled = Some((id, ticked));
                            }
                            ui.allocate_ui(vec2((fill - tag_w - 30.0).max(80.0), 20.0), |ui| {
                                ui.add(Label::new(RichText::new(&app.name).color(TEXT)).truncate());
                            });
                            for t in tags {
                                tag(ui, t);
                            }
                        });
                        cell(ui, PUBLISHER, false, |ui| {
                            ui.add(Label::new(RichText::new(&app.publisher).size(CAPTION + 1.0).color(TEXT_2)).truncate())
                        });
                        cell(ui, VERSION, false, |ui| {
                            ui.add(Label::new(RichText::new(&app.version).size(CAPTION).color(TEXT_3)).truncate())
                        });
                        cell(ui, SIZE, true, |ui| caption(ui, app.size.map(fmt_bytes).unwrap_or_default()));
                        cell(ui, ACTION, true, |ui| {
                            let reason = app.blocked();
                            let r = row_button(ui, reason.is_none() && !busy, "Uninstall");
                            if r.on_disabled_hover_text(reason.unwrap_or("Another uninstall is running.")).clicked() {
                                target = Some(vec![(*app).clone()]);
                            }
                        });
                    });
                });
            }
        });
        ui.add_space(S2);
        caption(ui, format!("{} apps. Kestrel doesn't guess which apps are unused: Windows keeps no reliable record of when desktop apps were last used.", shown.len()));
        if let Some((id, on)) = toggled {
            if on {
                self.apps.selected.insert(id);
            } else {
                self.apps.selected.remove(&id);
            }
        }
        if let Some(apps) = target {
            self.confirm = Some(uninstall_confirm(apps));
        }
    }

    pub(crate) fn startup_page(&mut self, ui: &mut Ui) {
        page_header(ui, "Startup apps", "Choose what starts when you sign in. Turning an app off doesn't uninstall it.");
        let mut refresh = false;
        ui.horizontal(|ui| {
            if !self.admin {
                caption(ui, "Apps that start for all users need administrator rights to change.");
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.startup.load.running() {
                    progress(ui, "Loading");
                } else {
                    refresh = link(ui, "Refresh").clicked();
                }
            });
        });
        if refresh {
            self.load_startup(ui.ctx());
        }
        ui.add_space(S2);
        let mut flip = None;
        let admin = self.admin;
        let busy = self.startup.toggle.running();
        group(ui, |ui| {
            if self.startup.entries.is_empty() {
                if self.startup.load.running() {
                    row(ui, 0, |ui| progress(ui, "Reading startup entries…"));
                } else {
                    empty_state(ui, "No startup apps", "Nothing is set to start when you sign in.");
                }
            }
            for (i, e) in self.startup.entries.iter().enumerate() {
                let publisher = if e.publisher.is_empty() { "Unknown publisher" } else { &e.publisher };
                let desc = format!("{publisher} · {}", e.location);
                let reason = if e.protected && e.enabled {
                    Some("Security software stays on.")
                } else if e.machine && !admin {
                    Some("Needs administrator rights (Settings > Restart as administrator).")
                } else {
                    None
                };
                let title = setting_row(ui, i, RichText::new(&e.name).color(TEXT), &desc, |ui| {
                    let mut on = e.enabled;
                    let r = ui.add_enabled_ui(reason.is_none() && !busy, |ui| toggle(ui, &mut on)).inner;
                    if r.on_disabled_hover_text(reason.unwrap_or_default()).changed() {
                        flip = Some(i);
                    }
                });
                title.on_hover_text(&e.command);
            }
        });
        ui.add_space(S2);
        caption(ui, "You can also change these in Task Manager > Startup apps (or Task Scheduler for scheduled tasks). Settings > Restore everything puts back what Kestrel changed.");
        if let Some(i) = flip {
            let e = self.startup.entries[i].clone();
            self.startup.toggle.start(ui.ctx(), move || (i, crate::startup::set_enabled(&e, !e.enabled)));
        }
    }

    pub(crate) fn gaming_page(&mut self, ui: &mut Ui) {
        page_header(
            ui,
            "Gaming",
            "Settings here stay on until you revert them; Kestrel saves each original value first. For changes that only last while you play, use Game Mode.",
        );
        pivot(
            ui,
            &mut self.gaming.tab,
            &[(GamingTab::Settings, "Windows settings"), (GamingTab::Games, "Per-game"), (GamingTab::Measure, "Measure")],
        );
        match self.gaming.tab {
            GamingTab::Settings => self.gaming_settings_tab(ui),
            GamingTab::Games => self.games_tab(ui),
            GamingTab::Measure => self.measure_tab(ui),
        }
    }

    fn gaming_settings_tab(&mut self, ui: &mut Ui) {
        let build = self.build;
        let admin = self.admin;
        let g = &mut self.gaming;
        let mut preset = g.preset;
        ui.horizontal(|ui| {
            secondary(ui, "Preset");
            if segmented(
                ui,
                &mut preset,
                &[(Preset::Balanced, "Balanced"), (Preset::Performance, "Performance"), (Preset::Custom, "Custom")],
            ) {
                g.preset = preset;
                if preset != Preset::Custom {
                    g.selected = preset_selection(preset, build);
                }
            }
            caption(ui, g.preset.desc());
        });
        let chosen: Vec<Tweak> = Tweak::ALL.into_iter().filter(|t| !t.is_privacy() && g.selected[t.index()]).collect();
        let busy = g.apply.running();
        let (mut apply, mut restore, mut restore_point) = (false, false, false);
        ui.add_space(S2);
        ui.horizontal(|ui| {
            apply = accent_button(ui, !busy && !chosen.is_empty(), &format!("Apply {} selected", chosen.len())).clicked();
            restore = button(ui, !busy, "Restore originals").clicked();
            restore_point = button(ui, !busy && admin, "Create restore point")
                .on_hover_text("A Windows System Restore point: a full rollback option on top of Kestrel's own backups.")
                .on_disabled_hover_text("Needs administrator rights (Settings > Restart as administrator).")
                .clicked();
            if busy || g.status_job.running() {
                progress(ui, if busy { "Working…" } else { "Reading current settings…" });
            }
        });

        // Saved original but no longer active: Windows, another app or the user changed it back.
        let changed: Vec<Tweak> = Tweak::ALL
            .into_iter()
            .filter(|t| {
                !t.is_privacy()
                    && t.unsupported(build).is_none()
                    && g.status.get(t.index()).is_some_and(|(applied, backup, _)| *backup && !applied)
            })
            .collect();
        let mut reapply = false;
        if !changed.is_empty() {
            ui.add_space(S3);
            let text = format!("{} setting(s) were changed back by Windows Update, another app or you.", changed.len());
            info_bar(ui, Status::Caution, &text, |ui| reapply = row_button(ui, true, "Apply again").clicked());
        }

        let mut revert = None;
        for (title, recommended) in [("Recommended", true), ("Advanced", false)] {
            section(ui, title, |ui| {
                caption(ui, if recommended { "Low risk, useful on most PCs" } else { "Useful in specific cases; check the trade-off" });
            });
            group(ui, |ui| {
                let has_backup = |t: &Tweak| g.status.get(t.index()).is_some_and(|s| s.1);
                // Windows 10-only setting: shown only so an old change can still be reverted.
                let shown: Vec<Tweak> = Tweak::ALL
                    .into_iter()
                    .filter(|t| {
                        !t.is_privacy()
                            && *t != Tweak::MemoryIntegrityOff
                            && Preset::Balanced.includes(*t) == recommended
                            && (*t != Tweak::NoBackgroundApps || has_backup(t))
                    })
                    .collect();
                if let Some(t) = tweak_rows(ui, g, shown, build) {
                    revert = Some(t);
                }
            });
        }
        ui.add_space(S4);
        egui::CollapsingHeader::new(RichText::new("What Kestrel doesn't do, and why").color(TEXT_2)).show(ui, |ui| {
            for (what, why) in [
                ("Strip or \"debloat\" Windows, or install a custom Windows", "Independent tests of debloated builds found no real FPS gain, and undoing them means reinstalling Windows. Apps that aren't running don't slow games."),
                ("Turn off Windows services or Microsoft Defender", "No measured gain, and it breaks features (Store, Xbox, updates) or leaves the PC unprotected."),
                ("Change timer resolution, HPET or boot settings", "Modern Windows already handles timers per app; tests show no gain, and some of these settings make games slower."),
                ("\"Clean\" memory or close apps by force", "Windows manages memory itself; force-closing apps loses their work. Game Mode's efficiency mode is the safe way to quiet background apps."),
                ("Network or registry \"ping\" tweaks", "Ping depends on the route to the game server; local tweaks have no credible evidence."),
                ("Delete shader caches automatically", "Games rebuild them, which causes stutter the next time you play."),
            ] {
                ui.label(RichText::new(what).color(TEXT));
                caption(ui, why);
                ui.add_space(S1);
            }
        });
        if let Some(t) = revert {
            self.start_restore(ui.ctx(), vec![t], false);
        }
        if reapply {
            let body = changed.iter().map(|t| t.info().title).collect::<Vec<_>>().join("\n");
            self.confirm = Some(Confirm {
                title: "Apply these settings again?".into(),
                body: format!("{body}\n\nYour original values stay saved, so Revert still works afterwards."),
                warning: None,
                ok: "Apply",
                action: Action::ApplyTweaks(changed),
            });
        }
        if restore_point {
            self.start_restore_point(ui.ctx());
        }
        if apply {
            let body = chosen.iter().map(|t| t.info().title).collect::<Vec<_>>().join("\n");
            self.confirm = Some(Confirm {
                title: format!("Apply {} settings?", chosen.len()),
                body: format!("{body}\n\nEach original value is saved first. Undo any time with Revert or Restore originals."),
                warning: chosen
                    .contains(&Tweak::HighPerfPower)
                    .then(|| "High performance uses more power; on a laptop, use it while plugged in.".into()),
                ok: "Apply",
                action: Action::ApplyTweaks(chosen),
            });
        }
        if restore {
            self.confirm = Some(restore_all_confirm());
        }
    }

    fn games_tab(&mut self, ui: &mut Ui) {
        let mut add = false;
        let mut refresh = false;
        ui.horizontal(|ui| {
            search_box(ui, &mut self.gaming.game_search, "Search games", 240.0);
            add = button(ui, true, "Add a game…").on_hover_text("Choose any game's .exe file").clicked();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.gaming.games_job.running() {
                    progress(ui, "Looking for installed games");
                } else {
                    refresh = link(ui, "Refresh").clicked();
                }
            });
        });
        if refresh {
            self.load_games(ui.ctx());
        }
        if add {
            if let Some(exe) = pick_exe() {
                let g = &mut self.gaming;
                match g.games.iter().position(|x| x.exes.iter().any(|e| e.eq_ignore_ascii_case(&exe))) {
                    Some(i) => g.game_search = g.games[i].name.clone(),
                    None => {
                        g.games.insert(0, Game::from_exe(exe, "Added"));
                        g.game_search.clear();
                    }
                }
            }
        }
        ui.add_space(S3);
        let build = self.build;
        let q = self.gaming.game_search.to_lowercase();
        let searching = self.gaming.games_job.running();
        let mut error = None;
        let (mut play, mut settings) = (None, None);
        group(ui, |ui| {
            let mut shown = self.gaming.games.iter_mut().filter(|g| q.is_empty() || g.name.to_lowercase().contains(&q)).peekable();
            if shown.peek().is_none() {
                if searching {
                    row(ui, 0, |ui| progress(ui, "Looking for Steam, Epic, GOG, Battle.net, EA, Riot, Ubisoft and Rockstar games…"));
                } else if q.is_empty() {
                    empty_state(ui, "No games found", "Use \"Add a game…\" to choose a game's .exe file.");
                } else {
                    empty_state(ui, "No matches", "No game name matches your search.");
                }
            }
            for (i, game) in shown.enumerate() {
                row(ui, i, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&game.name).color(TEXT));
                        caption(ui, game.source);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if row_button(ui, true, "Game settings")
                                .on_hover_text("Quality, Balanced or Performance presets for the game's own graphics settings.")
                                .clicked()
                            {
                                settings = Some((game.name.clone(), game.exe().to_string()));
                            }
                            if row_button(ui, true, "Play")
                                .on_hover_text("Turns on Game Mode with this game's profile, then starts the game.")
                                .clicked()
                            {
                                play = Some(game.exe().to_string());
                            }
                            ui.add_space(S2);
                            let file = |p: &str| Path::new(p).file_name().map_or(p.to_string(), |f| f.to_string_lossy().into_owned());
                            if game.exes.len() > 1 {
                                let before = game.chosen;
                                egui::ComboBox::from_id_salt(("exe", i)).selected_text(file(game.exe())).show_ui(ui, |ui| {
                                    for (n, e) in game.exes.iter().enumerate() {
                                        ui.selectable_value(&mut game.chosen, n, file(e)).on_hover_text(e);
                                    }
                                });
                                if game.chosen != before {
                                    game.refresh();
                                }
                            } else {
                                caption(ui, file(game.exe())).on_hover_text(game.exe());
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = S6;
                        for (n, s) in GameSetting::ALL.into_iter().enumerate() {
                            let (on, by_kestrel) = game.status[n];
                            let mut want = on;
                            let why = s.unsupported(build);
                            let mut hint = s.desc().to_string();
                            if on && !by_kestrel {
                                hint.push_str("\n\nSet outside Kestrel (in Windows or by another app). Turning it off returns this game to the Windows default.");
                            }
                            let r = ui.add_enabled_ui(why.is_none(), |ui| checkbox(ui, &mut want, RichText::new(s.label()).size(CAPTION + 1.0).color(TEXT_2))).inner;
                            let r = r.on_hover_text(hint);
                            let r = match why {
                                Some(w) => r.on_disabled_hover_text(w),
                                None => r,
                            };
                            if r.changed() {
                                let exe = game.exe().to_string();
                                if let Err(e) = if want { s.apply(&exe) } else { s.undo(&exe) } {
                                    error = Some(e);
                                }
                                game.refresh();
                            }
                        }
                    });
                });
            }
        });
        ui.add_space(S2);
        caption(ui, "Each option is the same per-program switch Windows uses (Settings > Graphics, and the program's Compatibility tab). Changes take effect the next time the game starts and can be turned off again.");
        if let Some(e) = error {
            self.show_error("Couldn't change the game setting", e);
        }
        if let Some(exe) = play {
            self.play(ui.ctx(), &exe);
        }
        if let Some((name, exe)) = settings {
            self.open_game_settings(name, exe);
        }
    }

    pub(crate) fn settings_page(&mut self, ui: &mut Ui) {
        page_header(ui, "Settings", "Permissions, recovery and activity.");
        let admin = self.admin;
        section(ui, "Permissions", |_| {});
        let mut relaunch = false;
        group(ui, |ui| {
            let desc = if admin {
                "Kestrel is running as administrator, so system-wide cleanup, all-users startup apps and restore points are available."
            } else {
                "Needed for system-wide cleanup locations, apps that start for all users, and restore points."
            };
            setting_row(ui, 0, RichText::new("Administrator rights").color(TEXT), desc, |ui| {
                if admin {
                    status(ui, Status::Ok, "Running as administrator");
                } else {
                    relaunch = button(ui, true, "Restart as administrator").clicked();
                }
            });
        });
        if relaunch {
            if let Err(e) = relaunch_as_admin("settings") {
                self.show_notice("Couldn't restart as administrator", e);
            }
        }

        section(ui, "Window", |_| {});
        let cfg = &mut self.gamemode.config;
        let mut save = false;
        group(ui, |ui| {
            let note = if cfg.auto {
                "Closing the window keeps Kestrel in the notification area, so automatic Game Mode keeps working. Click the icon to reopen it; its menu has Exit."
            } else {
                "Closing the window keeps Kestrel in the notification area instead of exiting. Click the icon to reopen it; its menu has Exit."
            };
            setting_row(ui, 0, RichText::new("Minimize to tray when closed").color(TEXT), note, |ui| {
                save |= toggle(ui, &mut cfg.tray).changed();
            });
        });
        ui.add_space(S2);
        caption(ui, "Off by default. Kestrel never starts with Windows, and uses no CPU while it waits in the tray.");
        if save {
            if let Err(e) = self.gamemode.config.save() {
                self.show_error("Settings", e);
            }
        }

        self.updates_section(ui);

        section(ui, "Recovery", |_| {});
        let (mut restore, mut point) = (false, false);
        let busy = self.gaming.apply.running();
        group(ui, |ui| {
            setting_row(
                ui,
                0,
                RichText::new("Restore everything").color(TEXT),
                "Puts back every original Kestrel saved: gaming settings, startup apps, per-game settings and display refresh rates.",
                |ui| restore = button(ui, !busy, "Restore…").clicked(),
            );
            setting_row(
                ui,
                1,
                RichText::new("Create a restore point").color(TEXT),
                "A Windows System Restore point, as a full rollback option on top of Kestrel's own backups.",
                |ui| point = button(ui, admin && !busy, "Create").on_disabled_hover_text("Needs administrator rights.").clicked(),
            );
        });
        if restore {
            self.confirm = Some(restore_all_confirm());
        }
        if point {
            self.start_restore_point(ui.ctx());
        }

        let mut reload = false;
        section(ui, "Activity", |ui| {
            // Right-to-left: the first link added ends up rightmost.
            reload = link(ui, "Refresh").clicked();
            if link(ui, "Open data folder").clicked() {
                open_uri(&data_dir().to_string_lossy());
            }
        });
        if reload {
            self.history = read_history();
        }
        group(ui, |ui| {
            row(ui, 0, |ui| {
                egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.label(RichText::new(&self.history).monospace().size(12.0).color(TEXT_2));
                });
            });
        });

        let mut copy = false;
        section(ui, "About this PC", |ui| copy = link(ui, "Copy").on_hover_text("For support forums or a friend helping out.").clicked());
        group(ui, |ui| {
            for (i, (part, value)) in self.about.iter().enumerate() {
                row(ui, i, |ui| {
                    ui.horizontal(|ui| {
                        ui.allocate_ui(vec2(110.0, 18.0), |ui| caption(ui, *part));
                        ui.add(Label::new(RichText::new(value).color(TEXT)).wrap());
                    });
                });
            }
        });
        if copy {
            let text: Vec<String> = self.about.iter().map(|(part, value)| format!("{part}: {value}")).collect();
            ui.ctx().copy_text(text.join("\n"));
            self.show_notice("Copied", "Your PC's details are on the clipboard.");
        }

        section(ui, "About", |_| {});
        let mut notices = false;
        group(ui, |ui| {
            setting_row(
                ui,
                0,
                RichText::new(format!("Kestrel {}", env!("CARGO_PKG_VERSION"))).color(TEXT),
                "Runs only while open. No services, telemetry or automatic cleanup. The only network access is the optional update check.",
                |_| {},
            );
            setting_row(
                ui,
                1,
                RichText::new("License").color(TEXT),
                "Free for personal and other noncommercial use (PolyForm Noncommercial 1.0.0). Built with open-source libraries that keep their own licenses.",
                |ui| {
                    if link(ui, "Third-party notices").clicked() {
                        notices = true;
                    }
                    if link(ui, "License").clicked() {
                        open_uri(&format!("{}/blob/main/LICENSE.md", crate::update::REPO));
                    }
                },
            );
            setting_row(
                ui,
                2,
                RichText::new("Left out on purpose").color(TEXT),
                "Kestrel doesn't mark apps as unused, because Windows has no reliable usage data for them.",
                |_| {},
            );
        });
        if notices {
            // Built into the exe, so every copy carries the libraries' licenses; written out only to be read.
            let path = data_dir().join("THIRD-PARTY-NOTICES.txt");
            match std::fs::write(&path, THIRD_PARTY_NOTICES) {
                Ok(()) => open_uri(&path.to_string_lossy()),
                Err(e) => self.show_error("Third-party notices", format!("Couldn't save the notices to open them: {e}")),
            }
        }
    }

    pub(crate) fn optimize_dialog(&mut self, ctx: &egui::Context) {
        let mut next = None;
        let (mut apply, mut restart_now) = (false, false);
        let admin = self.admin;
        let (_, close) = dialog(ctx, "optimize", 560.0, |ui| {
            dialog_title(ui, "Set up this PC for gaming");
            match &self.opt.stage {
                OptStage::Scanning => progress(ui, "Checking what slows games down on this PC…"),
                OptStage::Applying => progress(ui, "Applying your approved changes…"),
                OptStage::Review => {
                    secondary(ui, "Untick anything you don't want. Nothing changes until you confirm.");
                    ui.add_space(S3);
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        let heading = |ui: &mut Ui, text: &str| {
                            ui.add_space(S2);
                            ui.label(strong(text));
                            ui.add_space(S1);
                        };
                        heading(ui, "Windows settings");
                        if self.opt.tweaks.is_empty() {
                            caption(ui, "All recommended settings are already on.");
                        } else if self.opt.already_active > 0 {
                            caption(ui, format!("{} more recommended settings are already on.", self.opt.already_active));
                        }
                        for (t, on) in &mut self.opt.tweaks {
                            let info = t.info();
                            let label = match t.takes_effect() {
                                Some(when) => format!("{} ({when})", info.title),
                                None => info.title.to_string(),
                            };
                            let allowed = admin || !t.needs_admin();
                            ui.add_enabled_ui(allowed, |ui| checkbox(ui, on, label))
                                .response
                                .on_hover_text(format!("{}\n\nTrade-off: {}", info.purpose, info.drawback))
                                .on_disabled_hover_text("Needs administrator rights (Settings > Restart as administrator).");
                        }
                        if !self.opt.refresh.is_empty() {
                            heading(ui, "Display");
                            let list = self.opt.refresh.iter().map(|(_, c, m)| format!("{c} Hz to {m} Hz")).collect::<Vec<_>>().join(", ");
                            checkbox(ui, &mut self.opt.refresh_on, format!("Raise refresh rate ({list})"))
                                .on_hover_text("Switches back after 15 seconds unless you press Keep, so a black screen fixes itself.");
                        }
                        if !self.opt.startup_off.is_empty() {
                            heading(ui, "Apps that don't need to start with Windows");
                            caption(ui, "They keep running in the background and use memory while you play. They still open normally when you start them. Undo on the Startup apps page.");
                            for (e, on, why) in &mut self.opt.startup_off {
                                checkbox(ui, on, e.name.as_str()).on_hover_text(*why);
                            }
                        }
                        if self.opt.gm_on || !self.opt.gm_games.is_empty() {
                            heading(ui, "Your games");
                            let n = self.opt.gm_games.len();
                            let label = if n > 0 {
                                format!("Game Mode turns on by itself for your {n} game{}", if n == 1 { "" } else { "s" })
                            } else {
                                "Game Mode turns on by itself when a game in your list starts".to_string()
                            };
                            checkbox(ui, &mut self.opt.gm_on, label).on_hover_text(
                                "While you play, apps in your background list get efficiency mode and the power settings switch to performance. Everything goes back when the game closes.",
                            );
                            if n > 0 {
                                let names: Vec<&str> = self.opt.gm_games.iter().map(|(name, _)| name.as_str()).collect();
                                ui.horizontal_wrapped(|ui| {
                                    ui.add_space(26.0);
                                    caption(ui, names.join(", "));
                                });
                            }
                        }
                        heading(ui, "Files");
                        if self.opt.items.is_empty() {
                            caption(ui, "No unnecessary files found.");
                        }
                        for i in &mut self.opt.items {
                            ui.horizontal(|ui| {
                                checkbox(ui, &mut i.selected, i.cat.name);
                                caption(ui, fmt_bytes(i.found.bytes));
                            });
                            if let Some(w) = i.cat.warning {
                                ui.horizontal(|ui| {
                                    ui.add_space(26.0);
                                    ui.label(RichText::new(w).size(CAPTION).color(CAUTION));
                                });
                            }
                        }
                        if !self.opt.extras.is_empty() {
                            heading(ui, "Optional");
                            let n = self.opt.extras.len();
                            checkbox(ui, &mut self.opt.extras_on, format!("Remove {n} preinstalled extra{}", if n == 1 { "" } else { "s" }))
                                .on_hover_text(self.opt.extras.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join("\n"));
                            ui.horizontal_wrapped(|ui| {
                                ui.add_space(26.0);
                                caption(ui, "Frees space and clutter; it doesn't raise FPS, because these apps don't run while you play. Xbox apps and the Store are kept. Reinstall any of them from the Microsoft Store.");
                            });
                        }
                        if !self.opt.attention.is_empty() {
                            heading(ui, "Only you can fix");
                            ui.label(
                                RichText::new(format!("{}. Details and buttons: Overview > Status, or measure a game on Gaming > Measure.", self.opt.attention.join(", ")))
                                    .size(CAPTION)
                                    .color(TEXT_2),
                            );
                        }
                        if let Some(on) = &mut self.opt.memory_integrity {
                            ui.add_space(S3);
                            egui::Frame::default()
                                .fill(CAUTION.gamma_multiply(0.08))
                                .stroke(egui::Stroke::new(1.0_f32, CAUTION.gamma_multiply(0.5)))
                                .corner_radius(egui::CornerRadius::same(R_CONTROL))
                                .inner_margin(egui::Margin::same(12))
                                .show(ui, |ui| {
                                    ui.add_enabled_ui(admin, |ui| checkbox(ui, on, strong("Maximum performance: turn off memory integrity")))
                                        .response
                                        .on_disabled_hover_text("Needs administrator rights (Settings > Restart as administrator).");
                                    ui.label(
                                        RichText::new("Memory integrity protects Windows against malicious drivers. Turning it off gives about 4 to 10 percent more FPS in games limited by the processor. Leave it on if this PC is for work, or if a game's anti-cheat requires it. Needs a restart; you can turn it back on with Revert or in Windows Security.")
                                            .size(CAPTION)
                                            .color(TEXT_2),
                                    );
                                });
                        }
                    });
                    ui.add_space(S3);
                    ui.add_enabled_ui(admin, |ui| checkbox(ui, &mut self.opt.restore_point, "Create a Windows restore point first"))
                        .response
                        .on_disabled_hover_text("Needs administrator rights (Settings > Restart as administrator).");
                    dialog_buttons(ui, |ui| {
                        if button(ui, true, "Cancel").clicked() {
                            next = Some(OptStage::Closed);
                        }
                        if accent_button(ui, true, "Continue").clicked() {
                            next = Some(OptStage::Confirm);
                        }
                    });
                }
                OptStage::Confirm => {
                    let n = self.opt.tweaks.iter().filter(|(_, on)| *on).count()
                        + usize::from(self.opt.refresh_on && !self.opt.refresh.is_empty())
                        + self.opt.startup_off.iter().filter(|(_, on, _)| *on).count()
                        + usize::from(self.opt.gm_on)
                        + usize::from(self.opt.memory_integrity == Some(true))
                        + usize::from(self.opt.extras_on && !self.opt.extras.is_empty());
                    let (files, bytes) =
                        self.opt.items.iter().filter(|i| i.selected).fold((0, 0), |(c, b), i| (c + i.found.count, b + i.found.bytes));
                    ui.label(format!("Apply {n} setting change(s) and delete {files} item(s) ({})?", fmt_bytes(bytes)));
                    ui.add_space(S1);
                    secondary(ui, "Settings can be undone from Settings > Restore everything. Deleted files don't go to the Recycle Bin.");
                    dialog_buttons(ui, |ui| {
                        if button(ui, true, "Back").clicked() {
                            next = Some(OptStage::Review);
                        }
                        apply = accent_button(ui, true, "Apply").clicked();
                    });
                }
                OptStage::Done(lines) => {
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| result_summary(ui, lines));
                    let restart = lines.iter().any(|l| l.starts_with("\u{2714}") && l.contains("after a restart"));
                    ui.add_space(S2);
                    secondary(
                        ui,
                        if restart {
                            "Some changes need a restart. Afterwards, measure a game on Gaming > Measure to see the difference."
                        } else {
                            "Measure a game on Gaming > Measure to see the difference."
                        },
                    );
                    dialog_buttons(ui, |ui| {
                        if accent_button(ui, true, "Close").clicked() {
                            next = Some(OptStage::Closed);
                        }
                        if restart && button(ui, true, "Restart now").clicked() {
                            restart_now = true;
                        }
                    });
                }
                OptStage::Closed => {}
            }
        });
        if restart_now {
            // Windows restarts and Kestrel closes normally on the way (ending Game Mode if it's on).
            if let Err(e) =
                crate::system::run_hidden("shutdown.exe", &["/r", "/t", "5", "/c", "Kestrel: restarting to finish the gaming setup."])
            {
                self.show_error("Restart", e);
            }
        }
        let idle = matches!(self.opt.stage, OptStage::Review | OptStage::Done(_));
        if close && idle && next.is_none() {
            next = Some(OptStage::Closed);
        }
        if apply {
            self.apply_optimize(ctx);
        } else if let Some(s) = next {
            self.opt.stage = s;
        }
    }
}

/// What Optimize PC did, grouped so it's clear what actually changed. No scores or FPS claims.
fn result_summary(ui: &mut Ui, lines: &[String]) {
    use crate::app::{ALREADY, NOT_NEEDED};
    let failed: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("✖ ")).collect();
    let changed: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("✔ ")).collect();
    let already: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix(ALREADY)).collect();
    let not_needed: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix(NOT_NEEDED)).collect();
    let other: Vec<&str> =
        lines.iter().map(String::as_str).filter(|l| !["✖ ", "✔ ", ALREADY, NOT_NEEDED].iter().any(|p| l.starts_with(p))).collect();
    for (title, list, kind) in [
        ("Changed", changed, Status::Ok),
        ("Failed", failed, Status::Caution),
        ("Already set", already, Status::Neutral),
        ("Not needed on this PC", not_needed, Status::Neutral),
        ("Files and other results", other, Status::Neutral),
    ] {
        if list.is_empty() {
            continue;
        }
        ui.add_space(S2);
        status(ui, kind, &format!("{title} ({})", list.len()));
        for l in list {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.add(Label::new(RichText::new(l).size(CAPTION + 1.0).color(TEXT_2)).wrap());
            });
        }
    }
    ui.add_space(S3);
    caption(ui, "No restart is needed. Every changed setting can be put back in Settings > Restore everything. Results vary by PC: Kestrel doesn't promise a particular FPS gain.");
}

/// One row per tweak: checkbox, name, state and Revert on the title line, then what it does and its trade-off.
/// Returns the tweak whose Revert was clicked.
pub(super) fn tweak_rows(ui: &mut Ui, g: &mut GamingState, shown: Vec<Tweak>, build: u32) -> Option<Tweak> {
    let mut revert = None;
    for (i, t) in shown.into_iter().enumerate() {
        let info = t.info();
        let (applied, backup, not_needed) = g.status.get(t.index()).cloned().unwrap_or_default();
        let unsupported = t.unsupported(build).map(str::to_string).or(not_needed);
        let not_needed = unsupported.is_some() && t.unsupported(build).is_none();
        if unsupported.is_some() {
            g.selected[t.index()] = false;
        }
        row(ui, i, |ui| {
            // Title line: checkbox and name on the left; state and Revert on the same line, right.
            ui.horizontal(|ui| {
                let r = ui
                    .add_enabled_ui(unsupported.is_none(), |ui| {
                        checkbox(ui, &mut g.selected[t.index()], RichText::new(info.title).color(TEXT))
                    })
                    .inner;
                // Hand-picking gaming settings turns the preset into Custom; privacy settings aren't in presets.
                if r.on_hover_text(format!("Benefit: {}", info.benefit)).changed() && !t.is_privacy() {
                    g.preset = Preset::Custom;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if backup && row_button(ui, true, "Revert").on_hover_text("Put back the value from before Kestrel changed it").clicked()
                    {
                        revert = Some(t);
                    }
                    match unsupported {
                        Some(_) if not_needed => status(ui, Status::Ok, "Not needed"),
                        Some(_) => status(ui, Status::Neutral, "Not available"),
                        None if backup && !applied => status(ui, Status::Caution, "Changed back"),
                        None if applied => status(ui, Status::Ok, "On"),
                        None => status(ui, Status::Neutral, "Off"),
                    };
                });
            });
            // Description under the name, kept to a readable width.
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - CONTROL_COL + 60.0).max(240.0));
                    ui.label(RichText::new(unsupported.as_deref().unwrap_or(info.purpose)).size(CAPTION).color(TEXT_2));
                    if unsupported.is_none() {
                        caption(ui, format!("Trade-off: {}", info.drawback));
                    }
                });
            });
        });
    }
    revert
}

fn restore_all_confirm() -> Confirm {
    Confirm {
        title: "Restore original settings?".into(),
        body: "Everything Kestrel changed goes back to the value saved before the change: gaming settings, startup apps, per-game settings and display refresh rates.".into(),
        warning: None,
        ok: "Restore",
        action: Action::RestoreAll,
    }
}
