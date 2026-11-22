//! What a measurement means, in plain words, and what to do about it. Fixes Kestrel can make itself (reversible
//! settings, the refresh rate, the fast GPU for this game) are applied with one click; the rest are explained,
//! with a button to the right place where there is one. Built from the existing health checks and tweaks.

use super::gaming::GameSetting;
use super::health::{self, Check};
use super::measure::Stats;
use super::Tweak;

#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    Tweak(Tweak),
    /// Run this game (exe path) on the high-performance GPU.
    GameGpu(String),
    /// Displays below their maximum refresh rate: (device, current Hz, max Hz). Uses the keep-or-revert dialog.
    Refresh(Vec<(String, u32, u32)>),
}

/// A button to the right place: (label, URI).
pub type Link = Option<(&'static str, &'static str)>;

#[derive(Clone, Debug)]
pub struct Advice {
    /// Fixes Kestrel can apply: (title, why it helps, fix).
    pub auto: Vec<(String, String, Fix)>,
    /// Things only the user can do: (title, what and why, optional button (label, URI)).
    pub manual: Vec<(String, String, Link)>,
}

/// Whether the result is fine, and a plain-language verdict.
pub fn verdict(s: &Stats) -> (bool, String) {
    let low = s.avg_fps < 60.0;
    let uneven = s.low_fps < 0.5 * s.avg_fps || s.stutter_pct > 2.0;
    let (avg, lows) = (s.avg_fps.round(), s.low_fps.round());
    let text = match (low, uneven) {
        (false, false) => format!("Runs smoothly: {avg} FPS on average, and even the slowest moments stay at {lows} FPS."),
        (true, false) => format!("The frame rate is low: {avg} FPS on average. Under about 60 FPS, games feel less smooth and responsive."),
        (false, true) => {
            format!("The game stutters: it averages {avg} FPS, but the slowest moments drop to {lows} FPS, which you feel as hitches.")
        }
        (true, true) => format!("The frame rate is low and uneven: {avg} FPS on average, dropping to {lows} FPS in the slowest moments."),
    };
    (!low && !uneven, text)
}

/// Looks for causes on this PC (reads settings and runs the health checks; takes a few seconds).
pub fn advise(exe: &str, build: u32) -> Advice {
    let mut auto = Vec::new();
    for t in [Tweak::GameMode, Tweak::GameDvrOff, Tweak::WindowedGames, Tweak::PowerMode] {
        if t.unsupported(build).is_none() && !t.is_applied() && t.not_needed().is_none() {
            let info = t.info();
            auto.push((info.title.to_string(), info.benefit.to_string(), Fix::Tweak(t)));
        }
    }
    // Only PCs with two graphics chips (most gaming laptops) can run a game on the wrong one.
    let saved = crate::utils::backup::all();
    if health::gpu_drivers().len() >= 2 && !GameSetting::HighPerfGpu.status(exe, &saved).0 {
        auto.push((
            "Use the fast graphics card for this game".into(),
            "This PC has two graphics chips, and Windows may be running the game on the slower, power-saving one.".into(),
            Fix::GameGpu(exe.to_string()),
        ));
    }
    let checks = health::run();
    let mut manual = Vec::new();
    for c in checks {
        if !c.refresh_fix.is_empty() {
            let (_, cur, max) = c.refresh_fix[0];
            auto.push((
                format!("Raise the screen's refresh rate to {max} Hz"),
                format!("Your screen runs at {cur} Hz but supports {max} Hz, so you can't see more than {cur} FPS."),
                Fix::Refresh(c.refresh_fix.clone()),
            ));
        } else if wanted(&c) {
            manual.push((c.title.to_string(), c.detail, c.link));
        }
    }
    manual.push((
        "Lower the game's graphics settings".into(),
        "In the game's video or graphics options, choose a lower preset, or turn down resolution scale, shadows and effects. This helps more than any Windows setting when the graphics card is the limit.".into(),
        None,
    ));
    Advice { auto, manual }
}

/// Health findings that can cost frame rate or cause stutter (not the network or security status).
fn wanted(c: &Check) -> bool {
    c.attention && !matches!(c.title, "Network connection" | "Memory integrity (Core isolation)")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(avg: f64, low: f64, stutter: f64) -> Stats {
        Stats { frames: 1000, avg_fps: avg, low_fps: low, stutter_pct: stutter }
    }

    #[test]
    fn verdicts() {
        assert!(verdict(&s(144.0, 110.0, 0.3)).0);
        let (ok, text) = verdict(&s(45.0, 38.0, 0.5));
        assert!(!ok && text.starts_with("The frame rate is low:"));
        let (ok, text) = verdict(&s(120.0, 40.0, 0.8));
        assert!(!ok && text.starts_with("The game stutters"));
        assert!(!verdict(&s(120.0, 90.0, 3.0)).0); // many hitches even with good lows
        assert!(verdict(&s(45.0, 20.0, 5.0)).1.starts_with("The frame rate is low and uneven"));
    }
}
