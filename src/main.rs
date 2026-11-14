#![windows_subsystem = "windows"]

mod app;
mod cleaner;
mod gamemode;
mod optimizer;
mod startup;
mod system;
mod ui;
mod uninstaller;
mod update;
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

/// Window, taskbar and tray icon, drawn in code: white wings (a kestrel in flight) on the accent-blue rounded square.
pub(crate) fn icon() -> egui::IconData {
    const N: usize = 64;
    let seg = |px: f32, py: f32, (ax, ay): (f32, f32), (bx, by): (f32, f32)| {
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
    };
    let mut rgba = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded square, corner radius 14.
            let (cx, cy) = ((px - 32.0).abs() - 18.0, (py - 32.0).abs() - 18.0);
            let corner = cx.max(0.0).hypot(cy.max(0.0)) + cx.max(cy).min(0.0) - 14.0;
            let inside = (0.5 - corner).clamp(0.0, 1.0);
            let wing = seg(px, py, (14.0, 26.0), (32.0, 40.0)).min(seg(px, py, (32.0, 40.0), (50.0, 26.0)));
            let white = (4.5 - wing).clamp(0.0, 1.0);
            let mix = |accent: f32| (accent + (255.0 - accent) * white) as u8;
            rgba.extend([mix(0x4C as f32), mix(0x8B as f32), mix(0xE8 as f32), (inside * 255.0) as u8]);
        }
    }
    egui::IconData { rgba, width: N as u32, height: N as u32 }
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
