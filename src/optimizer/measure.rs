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
    /// Peak use of the graphics card's own memory while measuring, and how much it has: (used, total) in bytes.
    /// None when Windows doesn't report it, or for results loaded from history.
    pub vram: Option<(u64, u64)>,
    /// NVIDIA cards: how often the card slowed itself down because of heat while measuring (percent of samples)
    /// and the highest temperature (°C). None for other cards and for results loaded from history.
    pub heat: Option<(u32, u32)>,
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
        vram: None,
        heat: None,
    })
}

/// The dedicated graphics card's memory use, from Windows' "GPU Adapter Memory" counters (system-wide, so
/// apps next to the game count too: what matters is whether the card is full).
struct Vram {
    query: isize,
    counter: isize,
    /// The counter instance prefix for this card, like "luid_0x00000000_0x0001411e".
    card: String,
    total: u64,
}

impl Vram {
    fn open() -> Option<Vram> {
        use windows::Win32::System::Performance::{PdhAddEnglishCounterW, PdhOpenQueryW};
        let (card, total) = dedicated_card()?;
        let mut v = Vram { query: 0, counter: 0, card, total };
        unsafe {
            if PdhOpenQueryW(windows::core::PCWSTR::null(), 0, &mut v.query) != 0 {
                return None;
            }
            // The English name works on every Windows language.
            (PdhAddEnglishCounterW(v.query, windows::core::w!(r"\GPU Adapter Memory(*)\Dedicated Usage"), 0, &mut v.counter) == 0)
                .then_some(v)
        }
    }

    /// Bytes in use on the card now.
    fn sample(&self) -> Option<u64> {
        use windows::Win32::System::Performance::*;
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_LARGE, &mut size, &mut count, None);
            // The items are followed by their names in the same buffer; u64 keeps it aligned for the items.
            let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_LARGE, &mut size, &mut count, Some(items)) != 0 {
                return None;
            }
            std::slice::from_raw_parts(items, count as usize)
                .iter()
                .filter(|i| i.szName.to_string().is_ok_and(|n| n.to_lowercase().starts_with(&self.card)))
                .map(|i| i.FmtValue.Anonymous.largeValue.max(0) as u64)
                .max()
        }
    }
}

impl Drop for Vram {
    fn drop(&mut self) {
        if self.query != 0 {
            unsafe { windows::Win32::System::Performance::PdhCloseQuery(self.query) };
        }
    }
}

/// Why an NVIDIA card is slowing down, through NVML (nvml.dll, which comes with NVIDIA's driver).
struct Nvml {
    device: *mut std::ffi::c_void,
    reasons: unsafe extern "C" fn(*mut std::ffi::c_void, *mut u64) -> i32,
    temperature: unsafe extern "C" fn(*mut std::ffi::c_void, u32, *mut u32) -> i32,
    shutdown: unsafe extern "C" fn() -> i32,
}

/// nvmlClocksEventReasons that mean the card is slowing down because of heat: software thermal slowdown,
/// hardware thermal slowdown, and hardware slowdown (clocks at least halved). The power-limit reason isn't one:
/// every card hits it under full load, which is how boost works.
const HEAT_REASONS: u64 = 0x20 | 0x40 | 0x08;

impl Nvml {
    fn open() -> Option<Nvml> {
        use windows::core::{s, w};
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
        unsafe {
            let lib = LoadLibraryExW(w!("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
            let init: unsafe extern "C" fn() -> i32 = std::mem::transmute(GetProcAddress(lib, s!("nvmlInit_v2"))?);
            let handle: unsafe extern "C" fn(u32, *mut *mut std::ffi::c_void) -> i32 =
                std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetHandleByIndex_v2"))?);
            // The newer name first; drivers before 535 only have the older one.
            let reasons = GetProcAddress(lib, s!("nvmlDeviceGetCurrentClocksEventReasons"))
                .or_else(|| GetProcAddress(lib, s!("nvmlDeviceGetCurrentClocksThrottleReasons")))?;
            type Reasons = unsafe extern "C" fn(*mut std::ffi::c_void, *mut u64) -> i32;
            type Temperature = unsafe extern "C" fn(*mut std::ffi::c_void, u32, *mut u32) -> i32;
            type Shutdown = unsafe extern "C" fn() -> i32;
            let reasons: Reasons = std::mem::transmute(reasons);
            let temperature: Temperature = std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetTemperature"))?);
            let shutdown: Shutdown = std::mem::transmute(GetProcAddress(lib, s!("nvmlShutdown"))?);
            if init() != 0 {
                return None;
            }
            let mut device = std::ptr::null_mut();
            if handle(0, &mut device) != 0 {
                shutdown();
                return None;
            }
            // Built only now: dropping an Nvml shuts NVML down.
            Some(Nvml { device, reasons, temperature, shutdown })
        }
    }

    /// (slowing down because of heat right now, temperature in °C).
    fn sample(&self) -> Option<(bool, u32)> {
        let (mut reasons, mut temp) = (0u64, 0u32);
        unsafe {
            if (self.reasons)(self.device, &mut reasons) != 0 || (self.temperature)(self.device, 0, &mut temp) != 0 {
                return None;
            }
        }
        Some((reasons & HEAT_REASONS != 0, temp))
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        unsafe { (self.shutdown)() };
    }
}

/// The card with the most dedicated memory (at least 1 GB, so not a processor's graphics): (counter instance
/// prefix, bytes).
fn dedicated_card() -> Option<(String, u64)> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    (0..)
        .map_while(|i| unsafe { factory.EnumAdapters1(i) }.ok())
        .filter_map(|a| unsafe { a.GetDesc1() }.ok())
        .filter(|d| d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 && d.DedicatedVideoMemory >= 1 << 30)
        .max_by_key(|d| d.DedicatedVideoMemory)
        .map(|d| (luid_prefix(d.AdapterLuid.HighPart as u32, d.AdapterLuid.LowPart), d.DedicatedVideoMemory as u64))
}

fn luid_prefix(high: u32, low: u32) -> String {
    format!("luid_0x{high:08x}_0x{low:08x}")
}

/// Records `exe` (file name of a running game) for `SECONDS`. Needs administrator rights (or the Performance Log
/// Users group), which is how Windows protects the event tracing PresentMon uses.
pub fn capture(exe: &str) -> Result<Stats, String> {
    use std::os::windows::process::CommandExt;
    let tool = presentmon().ok_or("PresentMon isn't installed.")?;
    let csv = data_dir().join("measure.csv");
    let _ = std::fs::remove_file(&csv);
    let secs = SECONDS.to_string();
    use std::process::Stdio;
    let mut child = std::process::Command::new(tool)
        .args(["--process_name", exe, "--output_file"])
        .arg(&csv)
        .args(["--timed", &secs, "--terminate_after_timed", "--no_console_stats", "--session_name", "Kestrel", "--stop_existing_session"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start PresentMon: {e}"))?;
    // Graphics memory and (NVIDIA) heat slowdown are sampled while PresentMon records, so they match the frames.
    let vram = Vram::open();
    let nvml = Nvml::open();
    let mut peak = None;
    let (mut samples, mut hot, mut max_temp) = (0u32, 0u32, 0u32);
    while matches!(child.try_wait(), Ok(None)) {
        if let Some(used) = vram.as_ref().and_then(Vram::sample) {
            peak = Some(peak.unwrap_or(0).max(used));
        }
        if let Some((slowed, temp)) = nvml.as_ref().and_then(Nvml::sample) {
            samples += 1;
            hot += u32::from(slowed);
            max_temp = max_temp.max(temp);
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    let heat = (samples > 0).then(|| (100 * hot / samples, max_temp));
    let out = child.wait_with_output().map_err(|e| format!("PresentMon failed: {e}"))?;
    let text = std::fs::read_to_string(&csv).unwrap_or_default();
    let _ = std::fs::remove_file(&csv);
    stats(&text).map(|s| Stats { vram: peak.zip(vram.map(|v| v.total)), heat, ..s }).ok_or_else(|| {
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
                    vram: None,
                    heat: None,
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
        assert_eq!(
            h[0].1,
            Stats {
                frames: 8512,
                avg_fps: 143.0,
                low_fps: 97.0,
                stutter_pct: 0.4,
                gpu_busy_pct: None,
                capped_at: None,
                vram: None,
                heat: None
            }
        );
        let now = Stats {
            frames: 9000,
            avg_fps: 157.3,
            low_fps: 107.0,
            stutter_pct: 0.2,
            gpu_busy_pct: None,
            capped_at: None,
            vram: None,
            heat: None,
        };
        assert_eq!(
            describe(&now, Some(&h[0].1)),
            "157 FPS average, 107 FPS 1% lows, 0.2% stutter (last time 143 / 97 FPS: +10% average, +10% 1% lows)"
        );
    }

    #[test]
    fn luid_matches_counter_names() {
        assert!("luid_0x00000000_0x0001411e_phys_0".starts_with(&luid_prefix(0, 0x1411E)));
    }

    #[test]
    #[ignore = "reads this PC's NVIDIA card through NVML"]
    fn live_nvml() {
        let n = Nvml::open().expect("an NVIDIA driver");
        let (mut r, mut t) = (0u64, 0u32);
        let codes = unsafe { ((n.reasons)(n.device, &mut r), (n.temperature)(n.device, 0, &mut t)) };
        println!("return codes {codes:?}, reasons {r:#x}, temperature {t}; sample {:?}", n.sample());
    }

    #[test]
    #[ignore]
    fn live_vram() {
        let v = Vram::open().expect("no dedicated card or counter");
        let used = v.sample().expect("no sample for the card");
        eprintln!("{} : {used} of {} bytes", v.card, v.total);
        assert!(used > 0 && used <= v.total + (1 << 30));
    }
}
