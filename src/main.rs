#![windows_subsystem = "windows"]

mod app;
mod cleaner;
mod gamemode;
mod logo;
mod optimizer;
mod startup;
mod system;
mod ui;
mod uninstaller;
mod update;
mod upgrade;
mod utils;

use eframe::egui;

fn main() -> eframe::Result {
    // Run by Windows' RunOnce at sign-in after a crash or restart during Game Mode: restore and exit, no window.
    if std::env::args().any(|a| a == gamemode::RESTORE_FLAG) {
        gamemode::restore();
        return Ok(());
    }
    let relaunched = std::env::args().any(|a| a == system::RELAUNCHED);
    // Debug screenshot runs (KESTREL_SHOTS) may run next to a normal Kestrel.
    let shots = cfg!(debug_assertions) && std::env::var_os("KESTREL_SHOTS").is_some();
    if !shots && !system::claim_single_instance(relaunched) {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Kestrel")
            .with_icon(icon())
            .with_inner_size([1080.0, 720.0])
            .with_min_inner_size([900.0, 580.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Kestrel",
        options,
        Box::new(|cc| {
            ui::apply_theme(&cc.egui_ctx);
            Ok(Box::new(app::App::new()))
        }),
    )
}

/// Window, taskbar and tray icon (see `logo`).
pub(crate) fn icon() -> egui::IconData {
    icon_sized(64)
}

pub(crate) fn icon_sized(n: usize) -> egui::IconData {
    egui::IconData { rgba: logo::rgba(n, true), width: n as u32, height: n as u32 }
}

#[cfg(test)]
mod icon_tests {
    fn px(i: &eframe::egui::IconData, x: f32, y: f32) -> [u8; 4] {
        let n = i.width as f32;
        let at = ((y / 100.0 * n) as usize * i.width as usize + (x / 100.0 * n) as usize) * 4;
        i.rgba[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn tear_mark_at_every_size() {
        for n in [16, 24, 32, 64, 256] {
            let i = super::icon_sized(n);
            assert_eq!(px(&i, 1.0, 1.0)[3], 0, "{n}: corner outside the rounded tile is transparent");
            assert_eq!(px(&i, 40.0, 40.0), [200, 105, 58, 255], "{n}: head is rufous");
            assert_eq!(px(&i, 64.0, 38.0), [30, 30, 30, 255], "{n}: eye is dark");
            assert_eq!(px(&i, 12.0, 50.0), [30, 30, 30, 255], "{n}: tile behind the head");
            // Sidebar mark: only the bird, with the eye cut out.
            let bird = eframe::egui::IconData { rgba: super::logo::rgba(n, false), width: n as u32, height: n as u32 };
            assert_eq!(px(&bird, 12.0, 50.0)[3], 0, "{n}: no tile");
            assert_eq!(px(&bird, 64.0, 38.0)[3], 0, "{n}: eye is a hole");
            assert_eq!(px(&bird, 40.0, 40.0), [200, 105, 58, 255], "{n}: head is rufous");
        }
    }

    /// Writes the icon at each size to KESTREL_ICON_DIR for review.
    #[test]
    #[ignore]
    fn dump() {
        let dir = std::path::PathBuf::from(std::env::var_os("KESTREL_ICON_DIR").unwrap());
        for n in [16, 24, 32, 64, 256] {
            let i = super::icon_sized(n);
            let img = eframe::egui::ColorImage::from_rgba_unmultiplied([n, n], &i.rgba);
            std::fs::write(dir.join(format!("icon-{n}.png")), crate::app::devshots::png(&img)).unwrap();
        }
    }
}

#[cfg(test)]
mod live {
    /// Read-only pass over the real system: `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn read_only_smoke() {
        for i in crate::cleaner::scan_all() {
            println!("{:32} {:>8} files {:>12} {:?}", i.cat.name, i.found.count, crate::ui::fmt_bytes(i.found.bytes), i.found.notes);
        }
        let (apps, note) = crate::uninstaller::list();
        println!(
            "apps: {} ({} store, {} optional, {} protected) {note:?}",
            apps.len(),
            apps.iter().filter(|a| matches!(a.source, crate::uninstaller::AppSource::Store { .. })).count(),
            apps.iter().filter(|a| a.optional).count(),
            apps.iter().filter(|a| a.protected.is_some()).count()
        );
        for a in apps.iter().filter(|a| matches!(a.source, crate::uninstaller::AppSource::Store { .. })) {
            println!("store: {} | {} | {:?}", a.name, a.publisher, a.size.map(crate::ui::fmt_bytes));
        }
        for e in crate::startup::list() {
            println!("startup: {} | {} | enabled={} | {}", e.name, e.publisher, e.enabled, e.location);
        }
        for t in crate::optimizer::Tweak::ALL {
            println!("tweak {:?}: applied={}", t, t.is_applied());
        }
        for c in crate::optimizer::health::run() {
            println!("check {}: attention={} | {}", c.title, c.attention, c.detail);
        }
        for g in crate::optimizer::games::find_all() {
            println!("game {} [{}] {} candidates, using {} | {:?}", g.name, g.source, g.exes.len(), g.exe(), g.status);
        }
    }
}
