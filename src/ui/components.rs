//! Kestrel's design system: tokens, fonts and the few components every screen is built from.
//!
//! Direction: a Windows 11 utility, not a web dashboard. Neutral dark surfaces, one muted accent,
//! Segoe UI with semibold (never bold) for hierarchy, a 4 px spacing grid, 4 px corners on controls,
//! 6 px on list groups and 8 px on dialogs, 1 px borders instead of shadows, and list rows with
//! dividers instead of a card per item.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Layout, Margin, Rect, Response,
    RichText, Sense, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType,
};

// ---- Color tokens ---------------------------------------------------------------------------

/// Page background (Windows 11 dark base).
pub const BG: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
/// Navigation pane.
pub const SIDEBAR: Color32 = Color32::from_rgb(0x1b, 0x1b, 0x1b);
/// List groups, dialogs, notifications.
pub const LAYER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const LAYER_HOVER: Color32 = Color32::from_rgb(0x32, 0x32, 0x32);
pub const STROKE: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
pub const DIVIDER: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
pub const TEXT: Color32 = Color32::from_rgb(0xee, 0xee, 0xee);
pub const TEXT_2: Color32 = Color32::from_rgb(0xb0, 0xb0, 0xb0);
pub const TEXT_3: Color32 = Color32::from_rgb(0x86, 0x86, 0x86);
/// The single accent: a muted blue, used for the one primary action per screen, selection and "on" states.
pub const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x8b, 0xe8);
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0x8d, 0xb6, 0xf2);
// Semantic colors, only for status.
pub const SUCCESS: Color32 = Color32::from_rgb(0x6c, 0xcb, 0x5f);
pub const CAUTION: Color32 = Color32::from_rgb(0xe3, 0xb3, 0x41);
pub const CRITICAL: Color32 = Color32::from_rgb(0xf1, 0x70, 0x7b);

// ---- Type, spacing and shape tokens ---------------------------------------------------------

pub const TITLE: f32 = 20.0;
pub const BODY: f32 = 14.0;
pub const CAPTION: f32 = 12.0;
/// 4 px grid.
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S6: f32 = 24.0;
pub const R_CONTROL: u8 = 4;
pub const R_GROUP: u8 = 6;
pub const R_DIALOG: u8 = 8;

/// Width of the right-hand control column in setting rows.
pub const CONTROL_COL: f32 = 220.0;

// ---- Fonts ----------------------------------------------------------------------------------

const SEMIBOLD: &str = "semibold";
const ICONS: &str = "icons";

/// Segoe UI (regular + semibold), Segoe UI Symbol and Segoe Fluent Icons straight from Windows'
/// font folder, so nothing is bundled. Missing files fall back to egui's built-in fonts.
pub fn install_fonts(ctx: &egui::Context) {
    let dir = crate::system::env_path("WINDIR").join("Fonts");
    let mut fonts = FontDefinitions::default();
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut load = |name: &str, files: &[&str]| -> bool {
        let Some(bytes) = files.iter().find_map(|f| std::fs::read(dir.join(f)).ok()) else { return false };
        fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_owned(bytes)));
        true
    };
    let regular = load("segoe", &["segoeui.ttf"]);
    let semibold = load("segoe_sb", &["seguisb.ttf"]);
    let symbol = load("segoe_sym", &["seguisym.ttf"]);
    let icons = load("fluent_icons", &["SegoeIcons.ttf", "segmdl2.ttf"]);
    let mono = load("consolas", &["consola.ttf"]);

    let mut chain = Vec::new();
    if regular {
        chain.push("segoe".to_string());
    }
    if symbol {
        chain.push("segoe_sym".to_string());
    }
    chain.extend(defaults.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, chain.clone());
    let mut strong = if semibold { vec!["segoe_sb".to_string()] } else { vec![] };
    strong.extend(chain.iter().cloned());
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), strong);
    let mut icon_chain = if icons { vec!["fluent_icons".to_string()] } else { vec![] };
    icon_chain.extend(chain);
    fonts.families.insert(FontFamily::Name(ICONS.into()), icon_chain);
    if mono {
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "consolas".into());
    }
    ctx.set_fonts(fonts);
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Segoe Fluent Icons glyphs used in the app.
pub mod icon {
    pub const HOME: &str = "\u{E80F}";
    pub const BROOM: &str = "\u{EA99}";
    pub const APPS: &str = "\u{E71D}";
    pub const POWER: &str = "\u{E7E8}";
    pub const SPEED: &str = "\u{EC4A}";
    pub const GAME: &str = "\u{E7FC}";
    pub const SETTINGS: &str = "\u{E713}";
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

// ---- Text -----------------------------------------------------------------------------------

pub fn fmt_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

pub fn strong(text: impl Into<String>) -> RichText {
    RichText::new(text).font(semibold(BODY)).color(TEXT)
}

pub fn secondary(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text).size(BODY).color(TEXT_2))
}

pub fn caption(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text).size(CAPTION).color(TEXT_3))
}

/// Page title (Subtitle size, semibold) and one line of context.
pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(semibold(TITLE)).color(TEXT));
    ui.add_space(2.0);
    secondary(ui, subtitle);
    ui.add_space(S4);
}

/// Section heading above a list group; `right` adds actions aligned to the right edge.
pub fn section(ui: &mut Ui, title: &str, right: impl FnOnce(&mut Ui)) {
    ui.add_space(S6 - S2);
    ui.horizontal(|ui| {
        ui.label(strong(title));
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
    ui.add_space(S1);
}

// ---- Status ---------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum Status {
    Ok,
    Caution,
    Neutral,
}

impl Status {
    pub fn color(self) -> Color32 {
        match self {
            Status::Ok => SUCCESS,
            Status::Caution => CAUTION,
            Status::Neutral => TEXT_3,
        }
    }
}

/// A small colored dot and text: the one way status is shown. Painted as one unit, so the dot
/// stays on the left in right-aligned layouts too.
pub fn status(ui: &mut Ui, kind: Status, text: &str) -> Response {
    let color = if kind == Status::Neutral { TEXT_3 } else { TEXT_2 };
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(CAPTION), color);
    let (rect, resp) = ui.allocate_exact_size(vec2(14.0 + galley.size().x, 18.0), Sense::hover());
    ui.painter().circle_filled(pos2(rect.left() + 4.0, rect.center().y), 3.5, kind.color());
    ui.painter().galley(pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0), galley, color);
    resp
}

/// A quiet outlined tag, e.g. "Store".
pub fn tag(ui: &mut Ui, text: &str) {
    egui::Frame::default()
        .stroke(Stroke::new(1.0_f32, STROKE))
        .corner_radius(CornerRadius::same(R_CONTROL))
        .inner_margin(Margin::symmetric(5, 0))
        .show(ui, |ui| ui.label(RichText::new(text).size(CAPTION - 1.0).color(TEXT_3)));
}

/// Inline message bar: a colored edge, text and optional actions (Windows InfoBar).
pub fn info_bar(ui: &mut Ui, kind: Status, text: &str, actions: impl FnOnce(&mut Ui)) {
    let frame = egui::Frame::default()
        .fill(LAYER)
        .stroke(Stroke::new(1.0_f32, STROKE))
        .corner_radius(CornerRadius::same(R_GROUP))
        .inner_margin(Margin { left: 14, right: 10, top: 8, bottom: 8 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Actions first (right to left), then the text gets exactly the width they leave and wraps there,
            // so it can never run under a button however many actions there are.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                actions(ui);
                ui.add_space(S3);
                ui.allocate_ui_with_layout(vec2(ui.available_width(), 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.add(egui::Label::new(RichText::new(text).size(BODY).color(TEXT)).wrap());
                });
            });
        });
    let r = frame.response.rect;
    let edge = Rect::from_min_size(r.min + vec2(0.0, 4.0), vec2(3.0, r.height() - 8.0));
    ui.painter().rect_filled(edge, CornerRadius::same(2), kind.color());
}

// ---- Buttons and inputs ---------------------------------------------------------------------

/// Adds a button and draws the keyboard focus outline when it has focus.
fn add_button(ui: &mut Ui, enabled: bool, b: egui::Button) -> Response {
    let r = ui.add_enabled(enabled, b);
    if r.has_focus() {
        focus_ring(ui, r.rect, R_CONTROL);
    }
    r
}

/// The one primary action on a screen.
pub fn accent_button(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    let b = egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(ACCENT).stroke(Stroke::NONE).min_size(vec2(96.0, 32.0));
    add_button(ui, enabled, b)
}

/// Standard (secondary) button.
pub fn button(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    add_button(ui, enabled, egui::Button::new(text).min_size(vec2(0.0, 32.0)))
}

/// Compact button for row actions.
pub fn row_button(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    add_button(ui, enabled, egui::Button::new(RichText::new(text).size(CAPTION + 1.0)).min_size(vec2(0.0, 26.0)))
}

/// Text-only button (hyperlink style) for low-emphasis actions.
pub fn link(ui: &mut Ui, text: &str) -> Response {
    let r = ui.add(egui::Label::new(RichText::new(text).size(BODY).color(ACCENT_TEXT)).sense(Sense::click()));
    let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
    if r.hovered() || r.has_focus() {
        let y = r.rect.bottom() - 1.0;
        ui.painter().hline(r.rect.x_range(), y, Stroke::new(1.0_f32, ACCENT_TEXT));
    }
    r
}

fn focus_ring(ui: &Ui, rect: Rect, radius: u8) {
    ui.painter().rect_stroke(rect.expand(3.0), CornerRadius::same(radius + 3), Stroke::new(2.0_f32, TEXT), StrokeKind::Outside);
}

/// Windows-style toggle switch with "On"/"Off" text to its left (fixed width, so rows line up).
/// Painted as one unit; the text is part of the click target.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    const TEXT_W: f32 = 30.0;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(TEXT_W + 40.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, *on, if *on { "On" } else { "Off" }));
    if ui.is_rect_visible(rect) {
        let switch = Rect::from_min_size(pos2(rect.left() + TEXT_W, rect.center().y - 10.0), vec2(40.0, 20.0));
        let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
        ui.painter().text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            if *on { "On" } else { "Off" },
            FontId::proportional(BODY),
            if enabled { TEXT } else { TEXT_3 },
        );
        let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.08);
        let hover = resp.hovered() && enabled;
        let (fill, stroke, knob) = if *on {
            (dim(if hover { ACCENT.gamma_multiply(1.15) } else { ACCENT }), Stroke::NONE, dim(Color32::WHITE))
        } else {
            (if hover { LAYER_HOVER } else { Color32::TRANSPARENT }, Stroke::new(1.0_f32, dim(TEXT_2)), dim(TEXT_2))
        };
        ui.painter().rect(switch, CornerRadius::same(10), fill, stroke, StrokeKind::Inside);
        let x = egui::lerp(switch.left() + 10.0..=switch.right() - 10.0, t);
        ui.painter().circle_filled(pos2(x, switch.center().y), if hover { 6.0 } else { 5.0 }, knob);
        if resp.has_focus() {
            focus_ring(ui, switch, 10);
        }
    }
    resp
}

/// Windows-style checkbox: accent fill with a white check when on. The label is clickable too.
pub fn checkbox(ui: &mut Ui, on: &mut bool, label: impl Into<RichText>) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = S2;
        let (rect, mut resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
        let text = ui.add(egui::Label::new(label.into()).sense(Sense::click()));
        if resp.clicked() || text.clicked() {
            *on = !*on;
            resp.mark_changed();
        }
        let enabled = ui.is_enabled();
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, *on, ""));
        let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.4) };
        let hover = (resp.hovered() || text.hovered()) && enabled;
        if *on {
            ui.painter().rect_filled(rect, CornerRadius::same(R_CONTROL), dim(ACCENT));
            let c = rect.center();
            let pts = vec![pos2(c.x - 4.5, c.y + 0.5), pos2(c.x - 1.5, c.y + 3.5), pos2(c.x + 4.5, c.y - 3.5)];
            ui.painter().add(egui::Shape::line(pts, Stroke::new(1.6_f32, dim(Color32::WHITE))));
        } else {
            let fill = if hover { LAYER_HOVER } else { Color32::TRANSPARENT };
            ui.painter().rect(rect, CornerRadius::same(R_CONTROL), fill, Stroke::new(1.0_f32, dim(TEXT_2)), StrokeKind::Inside);
        }
        if resp.has_focus() {
            focus_ring(ui, rect, R_CONTROL);
        }
        resp
    })
    .inner
}

/// Pivot tabs: text items with an accent underline under the selected one.
pub fn pivot<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, items: &[(T, &str)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = S6;
        for &(value, label) in items {
            let selected = *current == value;
            let color = if selected { TEXT } else { TEXT_2 };
            let font = if selected { semibold(BODY) } else { FontId::proportional(BODY) };
            let r = ui.add(egui::Label::new(RichText::new(label).font(font).color(color)).sense(Sense::click()));
            if selected {
                let y = r.rect.bottom() + 5.0;
                let w = 16.0f32.min(r.rect.width());
                ui.painter().rect_filled(Rect::from_center_size(pos2(r.rect.center().x, y), vec2(w, 3.0)), CornerRadius::same(2), ACCENT);
            }
            if r.has_focus() {
                focus_ring(ui, r.rect, R_CONTROL);
            }
            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *current = value;
            }
        }
    });
    ui.add_space(S3);
}

/// A segmented choice (presets, profiles): joined buttons, the selected one filled.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, items: &[(T, &str)]) -> bool {
    let mut changed = false;
    egui::Frame::default()
        .stroke(Stroke::new(1.0_f32, STROKE))
        .corner_radius(CornerRadius::same(R_CONTROL))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for &(value, label) in items {
                    let selected = *current == value;
                    let b = egui::Button::new(RichText::new(label).color(if selected { TEXT } else { TEXT_2 }))
                        .fill(if selected { LAYER_HOVER } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(R_CONTROL - 1))
                        .min_size(vec2(96.0, 26.0));
                    if ui.add(b).clicked() && !selected {
                        *current = value;
                        changed = true;
                    }
                }
            });
        });
    changed
}

// ---- Lists ----------------------------------------------------------------------------------

/// A bordered list group. Put `row`s inside; rows after the first get a divider.
pub fn group<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::default()
        .fill(LAYER)
        .stroke(Stroke::new(1.0_f32, STROKE))
        .corner_radius(CornerRadius::same(R_GROUP))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

/// One list row (index `i` in its group) with standard padding.
pub fn row<R>(ui: &mut Ui, i: usize, add: impl FnOnce(&mut Ui) -> R) -> R {
    if i > 0 {
        let r = ui.available_rect_before_wrap();
        ui.painter().hline(r.left()..=r.right(), r.top(), Stroke::new(1.0_f32, DIVIDER));
    }
    egui::Frame::default()
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            add(ui)
        })
        .inner
}

/// Settings-style row: title and description on the left, controls on the right.
/// Returns the title's response (for tooltips).
pub fn setting_row(ui: &mut Ui, i: usize, title: impl Into<RichText>, desc: &str, controls: impl FnOnce(&mut Ui)) -> Response {
    setting_row_note(ui, i, None, title, desc, None, controls)
}

/// `setting_row` with an optional status dot before the title and an optional caution note.
pub fn setting_row_note(
    ui: &mut Ui,
    i: usize,
    lead: Option<Status>,
    title: impl Into<RichText>,
    desc: &str,
    note: Option<&str>,
    controls: impl FnOnce(&mut Ui),
) -> Response {
    row(ui, i, |ui| {
        ui.horizontal(|ui| {
            let left = (ui.available_width() - CONTROL_COL - S4).max(200.0);
            let title = ui
                .allocate_ui_with_layout(vec2(left, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(left);
                    let title = ui
                        .horizontal(|ui| {
                            if let Some(kind) = lead {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let (r, _) = ui.allocate_exact_size(vec2(8.0, 16.0), Sense::hover());
                                ui.painter().circle_filled(r.center(), 3.5, kind.color());
                            }
                            ui.label(title.into())
                        })
                        .inner;
                    if !desc.is_empty() {
                        ui.label(RichText::new(desc).size(CAPTION).color(TEXT_2));
                    }
                    if let Some(n) = note {
                        ui.label(RichText::new(n).size(CAPTION).color(CAUTION));
                    }
                    title
                })
                .inner;
            ui.with_layout(Layout::right_to_left(Align::Center), controls);
            title
        })
        .inner
    })
}

/// Fixed-width table cell; `right` aligns its content to the right edge.
pub fn cell<R>(ui: &mut Ui, width: f32, right: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = if right { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
    ui.allocate_ui_with_layout(vec2(width, 20.0), layout, |ui| {
        ui.set_width(width);
        add(ui)
    })
    .inner
}

/// Column header row for a table inside a group.
pub fn table_header(ui: &mut Ui, cols: &[(&str, f32, bool)]) {
    egui::Frame::default().inner_margin(Margin::symmetric(14, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            for &(name, width, right) in cols {
                let w = if width <= 0.0 { ui.available_width() - cols.iter().map(|c| c.1.max(0.0) + S2).sum::<f32>() + S2 } else { width };
                cell(ui, w, right, |ui| caption(ui, name));
            }
        });
    });
}

/// Placeholder shown inside a group when there is nothing to list yet.
pub fn empty_state(ui: &mut Ui, title: &str, text: &str) {
    row(ui, 0, |ui| {
        ui.add_space(S2);
        ui.label(strong(title));
        caption(ui, text);
        ui.add_space(S2);
    });
}

/// Inline progress: a spinner and what's happening.
pub fn progress(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(14.0).color(TEXT_2));
        secondary(ui, text);
    });
}

// ---- Dialogs and notifications --------------------------------------------------------------

/// Dialog shell: title, body, and buttons aligned right (primary first). Returns the inner value.
pub fn dialog<R>(ctx: &egui::Context, id: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let resp = egui::Modal::new(egui::Id::new(id))
        .frame(
            egui::Frame::default()
                .fill(LAYER)
                .stroke(Stroke::new(1.0_f32, STROKE))
                .corner_radius(CornerRadius::same(R_DIALOG))
                .inner_margin(Margin::same(24))
                .shadow(egui::Shadow { offset: [0, 8], blur: 32, spread: 0, color: Color32::from_black_alpha(110) }),
        )
        .show(ctx, |ui| {
            ui.set_width(width);
            add(ui)
        });
    let close = resp.should_close();
    (resp.inner, close)
}

pub fn dialog_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).font(semibold(TITLE)).color(TEXT));
    ui.add_space(S3);
}

/// Footer with right-aligned buttons. `buttons` adds them right to left, so add the primary button last.
pub fn dialog_buttons(ui: &mut Ui, buttons: impl FnOnce(&mut Ui)) {
    ui.add_space(S6);
    ui.with_layout(Layout::right_to_left(Align::Center), buttons);
}

/// Confirmation dialog. Some(true) = confirmed, Some(false) = cancelled, None = still open.
pub fn confirm(ctx: &egui::Context, title: &str, body: &str, warning: Option<&str>, ok: &str) -> Option<bool> {
    let (out, close) = dialog(ctx, "confirm", 440.0, |ui| {
        let mut out = None;
        dialog_title(ui, title);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| ui.label(RichText::new(body).size(BODY).color(TEXT_2)));
        if let Some(w) = warning {
            ui.add_space(S3);
            info_bar(ui, Status::Caution, w, |_| {});
        }
        dialog_buttons(ui, |ui| {
            if button(ui, true, "Cancel").clicked() {
                out = Some(false);
            }
            if accent_button(ui, true, ok).clicked() {
                out = Some(true);
            }
        });
        out
    });
    out.or(close.then_some(false))
}

/// Non-blocking notification (bottom right). Errors stay until dismissed; others fade after 8 s.
pub struct Toast {
    pub title: String,
    pub body: String,
    pub error: bool,
    pub created: Instant,
}

impl Toast {
    pub fn new(title: &str, body: impl Into<String>) -> Toast {
        let body = body.into();
        let t = title.to_lowercase();
        let error = t.contains("fail") || t.contains("couldn't") || t.contains("partly") || body.starts_with('✖') || body.contains("\n✖");
        Toast { title: title.to_string(), body, error, created: Instant::now() }
    }

    /// Always stays until dismissed, whatever the wording.
    pub fn error(title: &str, body: impl Into<String>) -> Toast {
        Toast { error: true, ..Toast::new(title, body) }
    }
}

pub fn toasts(ctx: &egui::Context, list: &mut Vec<Toast>) {
    const LIFE: Duration = Duration::from_secs(8);
    list.retain(|t| t.error || t.created.elapsed() < LIFE);
    if list.is_empty() {
        return;
    }
    let mut dismiss = None;
    egui::Area::new(egui::Id::new("toasts")).anchor(Align2::RIGHT_BOTTOM, vec2(-S4, -S4)).order(egui::Order::Foreground).show(ctx, |ui| {
        for (i, t) in list.iter().enumerate() {
            let frame = egui::Frame::default()
                .fill(LAYER)
                .stroke(Stroke::new(1.0_f32, STROKE))
                .corner_radius(CornerRadius::same(R_DIALOG))
                .inner_margin(Margin { left: 16, right: 12, top: 10, bottom: 10 })
                .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(90) })
                .show(ui, |ui| {
                    ui.set_width(340.0);
                    ui.horizontal(|ui| {
                        ui.label(strong(&t.title));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if link(ui, "Dismiss").clicked() {
                                dismiss = Some(i);
                            }
                        });
                    });
                    let lines: Vec<&str> = t.body.lines().collect();
                    for l in lines.iter().take(8) {
                        ui.label(RichText::new(*l).size(CAPTION).color(TEXT_2));
                    }
                    if lines.len() > 8 {
                        caption(ui, format!("…and {} more (see Settings > Activity).", lines.len() - 8));
                    }
                });
            let r = frame.response.rect;
            let color = if t.error { CRITICAL } else { SUCCESS };
            ui.painter().rect_filled(
                Rect::from_min_size(r.min + vec2(0.0, 6.0), vec2(3.0, r.height() - 12.0)),
                CornerRadius::same(2),
                color,
            );
            ui.add_space(S2);
        }
    });
    if let Some(i) = dismiss {
        list.remove(i);
    }
    if list.iter().any(|t| !t.error) {
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    /// Renders a banner off-screen at the narrowest content width Kestrel allows and checks that its text never
    /// overlaps its buttons (the 0.10/0.11 update banner did).
    #[test]
    fn info_bar_text_never_runs_under_actions() {
        use super::*;
        let text = "Update available: Kestrel 0.11.0. A new version with improvements and fixes is ready to install.";
        let ctx = egui::Context::default();
        let mut output = None;
        for _ in 0..2 {
            let input =
                egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(630.0, 400.0))), ..Default::default() };
            output = Some(ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    info_bar(ui, Status::Ok, text, |ui| {
                        link(ui, "Remind me later");
                        row_button(ui, true, "Release notes");
                        accent_button(ui, true, "Update now");
                    });
                });
            }));
        }
        let texts: Vec<(String, Rect)> = output
            .unwrap()
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
                _ => None,
            })
            .collect();
        let banner = texts.iter().find(|(s, _)| s.starts_with("Update available")).expect("banner text drawn").1;
        let buttons: Vec<&(String, Rect)> =
            texts.iter().filter(|(s, _)| ["Remind me later", "Release notes", "Update now"].contains(&s.as_str())).collect();
        assert_eq!(buttons.len(), 3);
        for (label, r) in buttons {
            assert!(!banner.intersects(*r), "banner text overlaps \"{label}\": {banner:?} vs {r:?}");
        }
    }

    #[test]
    fn bytes() {
        assert_eq!(super::fmt_bytes(512), "512 B");
        assert_eq!(super::fmt_bytes(1536), "1.5 KB");
        assert_eq!(super::fmt_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn toast_severity() {
        assert!(super::Toast::new("Couldn't restart", "x").error);
        assert!(super::Toast::new("Game Mode", "✔ A\n✖ B").error);
        assert!(!super::Toast::new("Cleanup complete", "✔ Removed").error);
    }
}
