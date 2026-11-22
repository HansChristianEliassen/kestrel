//! "Measure a game": a timed frame-time capture with Intel PresentMon (MIT, installed through winget on request),
//! turned into average FPS, 1% lows and stutter, and kept per game so a change can be compared before and after.

use std::path::PathBuf;

use crate::system::env_path;
use crate::utils::data_dir;

/// The winget package Kestrel offers to install: (name, id).
pub const PRESENTMON: (&str, &str) = ("PresentMon Console (Intel)", "Intel.PresentMon.Console");
pub const SECONDS: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub frames: usize,
    pub avg_fps: f64,
    /// FPS over the slowest 1% of frames.
    pub low_fps: f64,
    /// Share of frames that took more than twice the median frame time, in percent.
    pub stutter_pct: f64,
    /// How much of the frame time the graphics card was busy, in percent (PresentMon's MsGPUBusy). None when
    /// the column is missing (older PresentMon) or for results loaded from history.
    pub gpu_busy_pct: Option<f64>,
    /// The frame rate is held at this limit (V-Sync or a frame limiter), if it is.
    pub capped_at: Option<u32>,
}

/// Common frame-rate limits: refresh rates and round limiter values.
const CAPS: [u32; 15] = [30, 50, 60, 75, 90, 100, 120, 144, 165, 170, 180, 200, 240, 280, 360];

/// winget's command link for the portable package (per-user install, or machine-wide when installed as admin).
pub fn presentmon() -> Option<PathBuf> {
    [env_path("LOCALAPPDATA").join(r"Microsoft\WinGet\Links\presentmon.exe"), env_path("ProgramFiles").join(r"WinGet\Links\presentmon.exe")]
        .into_iter()
        .find(|p| p.is_file())
}

/// Frame statistics from PresentMon's CSV, using the MsBetweenPresents column (found by name, not position).
pub fn stats(csv: &str) -> Option<Stats> {
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next()?.split(',').map(str::trim).collect();
    let col = header.iter().position(|h| *h == "MsBetweenPresents")?;
    let gpu_col = header.iter().position(|h| *h == "MsGPUBusy");
    let rows: Vec<(f64, Option<f64>)> = lines
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            let ms: f64 = f.get(col)?.trim().parse().ok()?;
            (ms > 0.0).then(|| (ms, gpu_col.and_then(|c| f.get(c)?.trim().parse().ok())))
        })
        .collect();
    if rows.len() < 10 {
        return None;
    }
    let mut ms: Vec<f64> = rows.iter().map(|r| r.0).collect();
    let frames = ms.len();
    let total: f64 = ms.iter().sum();
    ms.sort_by(|a, b| a.total_cmp(b));
    let median = ms[frames / 2];
    let slowest = &ms[frames - (frames / 100).max(1)..];
    let stutters = ms.iter().filter(|v| **v > 2.0 * median).count();
    // GPU busy as a share of the frame time, over the frames that report it.
    let (busy, timed) = rows.iter().filter_map(|(ms, busy)| busy.map(|b| (b, *ms))).fold((0.0, 0.0), |(b, t), (x, y)| (b + x, t + y));
    let gpu_busy_pct = (timed > 0.0).then(|| (100.0 * busy / timed).min(100.0));
    // Capped: the median sits on a common limit and nearly all frames are close to it.
    let fps = 1000.0 / median;
    let tight = ms.iter().filter(|v| (**v - median).abs() <= 0.08 * median).count() as f64 / frames as f64;
    let capped_at = CAPS.into_iter().find(|c| (fps - *c as f64).abs() <= 0.015 * *c as f64).filter(|_| tight >= 0.85);
    Some(Stats {
        frames,
        avg_fps: 1000.0 * frames as f64 / total,
        low_fps: 1000.0 * slowest.len() as f64 / slowest.iter().sum::<f64>(),
        stutter_pct: 100.0 * stutters as f64 / frames as f64,
        gpu_busy_pct,
        capped_at,
    })
}

/// Records `exe` (file name of a running game) for `SECONDS`. Needs administrator rights (or the Performance Log
/// Users group), which is how Windows protects the event tracing PresentMon uses.
pub fn capture(exe: &str) -> Result<Stats, String> {
    use std::os::windows::process::CommandExt;
    let tool = presentmon().ok_or("PresentMon isn't installed.")?;
    let csv = data_dir().join("measure.csv");
    let _ = std::fs::remove_file(&csv);
    let secs = SECONDS.to_string();
    let out = std::process::Command::new(tool)
        .args(["--process_name", exe, "--output_file"])
        .arg(&csv)
        .args(["--timed", &secs, "--terminate_after_timed", "--no_console_stats", "--session_name", "Kestrel", "--stop_existing_session"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| format!("Couldn't start PresentMon: {e}"))?;
    let text = std::fs::read_to_string(&csv).unwrap_or_default();
    let _ = std::fs::remove_file(&csv);
    stats(&text).ok_or_else(|| {
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase() + &String::from_utf8_lossy(&out.stdout).to_lowercase();
        if err.contains("access denied") {
            "Measuring needs administrator rights (Settings > Restart as administrator).".to_string()
        } else {
            "No frames were recorded. Keep the game running in the foreground (not minimized or on a loading screen) while it measures."
                .to_string()
        }
    })
}

pub fn history_file() -> PathBuf {
    data_dir().join("measurements.txt")
}

/// Earlier results for this game, oldest first: (unix time, stats).
pub fn history(exe: &str) -> Vec<(u64, Stats)> {
    parse_history(&std::fs::read_to_string(history_file()).unwrap_or_default(), exe)
}

pub fn parse_history(text: &str, exe: &str) -> Vec<(u64, Stats)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() != 6 || !f[0].eq_ignore_ascii_case(exe) {
                return None;
            }
            let n = |i: usize| f[i].parse::<f64>().ok();
            Some((
                f[1].parse().ok()?,
                Stats {
                    avg_fps: n(2)?,
                    low_fps: n(3)?,
                    stutter_pct: n(4)?,
                    frames: f[5].parse().ok()?,
                    gpu_busy_pct: None,
                    capped_at: None,
                },
            ))
        })
        .collect()
}

pub fn save(exe: &str, s: &Stats) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut text = std::fs::read_to_string(history_file()).unwrap_or_default();
    text.push_str(&format!("{}\t{now}\t{:.1}\t{:.1}\t{:.2}\t{}\n", exe.to_lowercase(), s.avg_fps, s.low_fps, s.stutter_pct, s.frames));
    let _ = crate::utils::write_atomic(&history_file(), text.as_bytes());
}

/// "143 FPS average, 97 FPS 1% lows, 0.4% stutter", plus the change from `before` if given.
pub fn describe(s: &Stats, before: Option<&Stats>) -> String {
    let mut text = format!("{:.0} FPS average, {:.0} FPS 1% lows, {:.1}% stutter", s.avg_fps, s.low_fps, s.stutter_pct);
    if let Some(b) = before {
        let pct = |now: f64, then: f64| if then > 0.0 { format!("{:+.0}%", 100.0 * (now - then) / then) } else { "n/a".into() };
        text.push_str(&format!(
            " (last time {:.0} / {:.0} FPS: {} average, {} 1% lows)",
            b.avg_fps,
            b.low_fps,
            pct(s.avg_fps, b.avg_fps),
            pct(s.low_fps, b.low_fps)
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_stats() {
        // 99 frames at 10 ms (100 FPS) and one 50 ms hitch; extra columns and an NA row are ignored.
        let mut csv = String::from("Application,ProcessID,MsBetweenPresents,DisplayedTime\n");
        for _ in 0..99 {
            csv.push_str("game.exe,1,10.0,10.0\n");
        }
        csv.push_str("game.exe,1,50.0,NA\ngame.exe,1,NA,NA\n");
        let s = stats(&csv).unwrap();
        assert_eq!(s.frames, 100);
        assert!((s.avg_fps - 1000.0 * 100.0 / 1040.0).abs() < 0.01);
        assert!((s.low_fps - 20.0).abs() < 0.01); // the slowest 1% is the 50 ms frame
        assert!((s.stutter_pct - 1.0).abs() < 0.01);
        assert!(stats("Application,MsBetweenPresents\n").is_none());
        assert!(stats("no,such,column\n1,2,3\n").is_none());
    }

    fn csv(rows: &[(f64, f64)]) -> String {
        let mut s = String::from("Application,MsGPUBusy,MsBetweenPresents\n");
        for (busy, ms) in rows {
            s.push_str(&format!("game.exe,{busy},{ms}\n"));
        }
        s
    }

    #[test]
    fn gpu_busy_and_caps() {
        // GPU-bound: the card is busy for nearly the whole frame (uneven times, so no cap).
        let gpu: Vec<(f64, f64)> = (0..200).map(|i| (9.5 + (i % 7) as f64 * 0.4, 10.0 + (i % 7) as f64 * 0.4)).collect();
        let s = stats(&csv(&gpu)).unwrap();
        assert!(s.gpu_busy_pct.unwrap() > 90.0 && s.capped_at.is_none());
        // Processor-bound: the card idles half the frame.
        let cpu: Vec<(f64, f64)> = (0..200).map(|i| (5.0, 10.0 + (i % 9) as f64 * 0.7)).collect();
        assert!(stats(&csv(&cpu)).unwrap().gpu_busy_pct.unwrap() < 60.0);
        // Held at 165 FPS (6.06 ms) by V-Sync or a limiter.
        let capped: Vec<(f64, f64)> = (0..200).map(|i| (3.0, 6.06 + if i % 2 == 0 { 0.05 } else { -0.05 })).collect();
        assert_eq!(stats(&csv(&capped)).unwrap().capped_at, Some(165));
        // No MsGPUBusy column (older PresentMon): no share.
        assert!(stats("Application,MsBetweenPresents\n".to_string().as_str()).is_none());
    }

    #[test]
    fn history_round_trip() {
        let text = "game.exe\t100\t143.0\t97.0\t0.40\t8512\nother.exe\t101\t60.0\t50.0\t1.00\t3600\nbroken line\n";
        let h = parse_history(text, "GAME.EXE");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].1, Stats { frames: 8512, avg_fps: 143.0, low_fps: 97.0, stutter_pct: 0.4, gpu_busy_pct: None, capped_at: None });
        let now = Stats { frames: 9000, avg_fps: 157.3, low_fps: 107.0, stutter_pct: 0.2, gpu_busy_pct: None, capped_at: None };
        assert_eq!(
            describe(&now, Some(&h[0].1)),
            "157 FPS average, 107 FPS 1% lows, 0.2% stutter (last time 143 / 97 FPS: +10% average, +10% 1% lows)"
        );
    }
}
