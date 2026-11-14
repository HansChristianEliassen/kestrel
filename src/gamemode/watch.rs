//! Game detection and closing apps, without polling and without touching game processes.
//!
//! Detection listens for Windows' "foreground window changed" event (SetWinEventHook), which costs
//! nothing until you switch windows. Each event triggers one process snapshot (Toolhelp), which reads
//! a list kept by Windows and never opens a process, so anti-cheat software has nothing to object to.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use eframe::egui;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, EVENT_SYSTEM_FOREGROUND, GW_OWNER,
    WINEVENT_OUTOFCONTEXT, WM_CLOSE,
};

use crate::system::process_list;

/// Never closed, whatever the list says: Windows itself, security software, anti-cheat and Kestrel.
const PROTECTED: &[&str] = &[
    "system",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "svchost.exe",
    "dwm.exe",
    "explorer.exe",
    "sihost.exe",
    "fontdrvhost.exe",
    "taskhostw.exe",
    "ctfmon.exe",
    "audiodg.exe",
    "conhost.exe",
    "runtimebroker.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "textinputhost.exe",
    "applicationframehost.exe",
    "lockapp.exe",
    "kestrel.exe",
];
const PROTECTED_PARTS: &[&str] = &[
    "securityhealth",
    "msmpeng",
    "nissrv",
    "defender",
    "malwarebytes",
    "eset",
    "kaspersky",
    "bitdefender",
    "norton",
    "avast",
    "mcafee",
    "sophos",
    "avira",
    "anticheat",
    "easyanticheat",
    "battleye",
    "beservice",
    "vgc",
    "vgtray",
    "faceit",
    "ricochet",
    "javelin",
    "nprotect",
    "xigncode",
    "punkbuster",
];

/// Why an app may not be put on the close list, if it may not.
pub fn protected(exe: &str) -> Option<&'static str> {
    let e = exe.to_lowercase();
    if PROTECTED.contains(&e.as_str()) {
        Some("Windows needs it")
    } else if PROTECTED_PARTS.iter().any(|p| e.contains(p)) {
        Some("security or anti-cheat software")
    } else {
        None
    }
}

/// Visible, top-level (unowned) windows with their process ids.
fn top_windows() -> Vec<(HWND, u32)> {
    unsafe extern "system" fn each(hwnd: HWND, list: LPARAM) -> BOOL {
        let list = &mut *(list.0 as *mut Vec<(HWND, u32)>);
        if IsWindowVisible(hwnd).as_bool() && GetWindow(hwnd, GW_OWNER).map_or(true, |o| o.is_invalid()) {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            list.push((hwnd, pid));
        }
        true.into()
    }
    let mut list: Vec<(HWND, u32)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

/// Programs that currently have a visible window, for picking apps to close (protected ones left out).
pub fn windowed_apps() -> Vec<String> {
    let windows: HashSet<u32> = top_windows().into_iter().map(|(_, pid)| pid).collect();
    let mut apps: Vec<String> =
        process_list().into_iter().filter(|(pid, name)| windows.contains(pid) && protected(name).is_none()).map(|(_, n)| n).collect();
    apps.sort();
    apps.dedup();
    apps
}

/// One process put in efficiency mode: what's needed to identify it again and undo the change.
struct Quieted {
    pid: u32,
    /// Process start time, so a process ID Windows has since reused is never touched.
    created: u64,
    priority: u32,
    name: String,
}

fn parse_quieted(saved: &str) -> Vec<Quieted> {
    saved
        .split(';')
        .filter_map(|e| {
            let mut f = e.splitn(4, ':');
            Some(Quieted {
                pid: f.next()?.parse().ok()?,
                created: f.next()?.parse().ok()?,
                priority: f.next()?.parse().ok()?,
                name: f.next()?.to_string(),
            })
        })
        .collect()
}

fn open_for_priority(pid: u32) -> Option<windows::Win32::Foundation::HANDLE> {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION};
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION, false, pid) }.ok()
}

fn created(h: windows::Win32::Foundation::HANDLE) -> Option<u64> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetProcessTimes;
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) }.ok()?;
    Some((u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
}

/// Windows' EcoQoS switch: `on` asks for power-saving scheduling, otherwise Windows decides again.
fn set_ecoqos(h: windows::Win32::Foundation::HANDLE, on: bool) -> bool {
    use windows::Win32::System::Threading::{
        ProcessPowerThrottling, SetProcessInformation, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_STATE,
    };
    let mask = if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 };
    let state = PROCESS_POWER_THROTTLING_STATE { Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION, ControlMask: mask, StateMask: mask };
    unsafe {
        SetProcessInformation(
            h,
            ProcessPowerThrottling,
            &state as *const _ as *const c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
    .is_ok()
}

/// What to save before quieting the listed apps: one entry per running process ("pid:created:priority:name").
pub fn quiet_snapshot(names: &[String]) -> String {
    use windows::Win32::System::Threading::GetPriorityClass;
    let mut entries = Vec::new();
    for (pid, name) in process_list() {
        if !names.contains(&name.to_lowercase()) || protected(&name).is_some() || name.contains([':', ';']) {
            continue;
        }
        let Some(h) = open_for_priority(pid) else { continue };
        if let Some(created) = created(h) {
            let priority = unsafe { GetPriorityClass(h) };
            if priority != 0 {
                entries.push(format!("{pid}:{created}:{priority}:{name}"));
            }
        }
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(h) };
    }
    entries.join(";")
}

/// Windows 11 efficiency mode (as in Task Manager, but with below-normal rather than idle priority, so voice
/// chat and music keep up): the apps keep running with less CPU time and power-saving scheduling.
pub fn quiet(saved: &str, names: &[String]) -> Vec<String> {
    use windows::Win32::System::Threading::{SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS};
    if names.is_empty() {
        return vec!["Your list of background apps is empty.".into()];
    }
    let mut done: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for q in parse_quieted(saved) {
        let ok = open_for_priority(q.pid).is_some_and(|h| {
            let ok =
                created(h) == Some(q.created) && unsafe { SetPriorityClass(h, BELOW_NORMAL_PRIORITY_CLASS) }.is_ok() && set_ecoqos(h, true);
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(h) };
            ok
        });
        let list = if ok { &mut done } else { &mut failed };
        if !list.contains(&q.name) {
            list.push(q.name);
        }
    }
    let mut lines = Vec::new();
    if !done.is_empty() {
        lines.push(format!("✔ Efficiency mode: {}", done.join(", ")));
    }
    if !failed.is_empty() {
        lines.push(format!("✖ Couldn't put {} in efficiency mode (it may run as administrator).", failed.join(", ")));
    }
    if done.is_empty() && failed.is_empty() {
        lines.push("None of the apps in your list were running.".into());
    }
    lines
}

/// Puts back the saved priority and lets Windows manage scheduling again. Processes that have since closed
/// (or whose ID now belongs to another program) are skipped, so this never fails.
pub fn unquiet(saved: &str) -> String {
    use windows::Win32::System::Threading::{SetPriorityClass, PROCESS_CREATION_FLAGS};
    let mut names: Vec<String> = Vec::new();
    for q in parse_quieted(saved) {
        let Some(h) = open_for_priority(q.pid) else { continue };
        if created(h) == Some(q.created) {
            let _ = unsafe { SetPriorityClass(h, PROCESS_CREATION_FLAGS(q.priority)) };
            set_ecoqos(h, false);
            if !names.contains(&q.name) {
                names.push(q.name);
            }
        }
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(h) };
    }
    match names.len() {
        0 => "Background apps had already closed; nothing to restore.".into(),
        _ => format!("Normal priority again: {}", names.join(", ")),
    }
}

/// Memory used by each program that has a window (all its processes added up), largest first.
/// Only asks for limited query rights, and skips protected programs and the configured games, so a game and its
/// anti-cheat are never opened.
pub fn app_memory(skip: &[String]) -> Vec<(String, u64)> {
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let apps = windowed_apps();
    // Every app is listed, at 0 when its memory can't be read (for example one running as administrator).
    let mut totals: std::collections::HashMap<String, u64> =
        apps.iter().filter(|a| !skip.contains(&a.to_lowercase())).map(|a| (a.clone(), 0)).collect();
    for (pid, name) in process_list() {
        if !apps.contains(&name) || skip.contains(&name.to_lowercase()) {
            continue;
        }
        let Ok(h) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { continue };
        let mut c = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
        if unsafe { K32GetProcessMemoryInfo(h, &mut c, c.cb) }.as_bool() {
            *totals.entry(name).or_default() += c.WorkingSetSize as u64;
        }
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(h) };
    }
    let mut list: Vec<(String, u64)> = totals.into_iter().collect();
    list.sort_by_key(|(_, b)| std::cmp::Reverse(*b));
    list
}

/// Sends a normal close request (WM_CLOSE, the same as clicking X) to each listed app's windows.
/// Never force-closes: an app with unsaved work can still ask, and apps may choose to stay in the tray.
pub fn close_apps(names: &[String]) -> Vec<String> {
    if names.is_empty() {
        return vec!["Your list of apps to close is empty.".into()];
    }
    let procs = process_list();
    let windows = top_windows();
    names
        .iter()
        .map(|name| {
            if let Some(why) = protected(name) {
                return format!("✖ Didn't close {name}: {why}.");
            }
            let pids: HashSet<u32> = procs.iter().filter(|(_, n)| n == name).map(|(p, _)| *p).collect();
            if pids.is_empty() {
                return format!("✔ {name} wasn't running.");
            }
            let mut asked = 0;
            for (hwnd, pid) in &windows {
                if pids.contains(pid) && unsafe { PostMessageW(*hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok() {
                    asked += 1;
                }
            }
            if asked == 0 {
                format!("✖ {name} has no open window to close (it may be running in the tray).")
            } else {
                format!("✔ Asked {name} to close.")
            }
        })
        .collect()
}

static CHANGED: AtomicBool = AtomicBool::new(false);
static WAKE: OnceLock<egui::Context> = OnceLock::new();

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    CHANGED.store(true, Ordering::Relaxed);
    if let Some(ctx) = WAKE.get() {
        ctx.request_repaint();
    }
}

/// Starts listening for foreground-window changes. Must run on the UI thread, whose message
/// loop delivers the events.
pub fn start(ctx: &egui::Context) -> Option<HWINEVENTHOOK> {
    let _ = WAKE.set(ctx.clone());
    CHANGED.store(true, Ordering::Relaxed); // check once right away
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            // Kestrel's own window counts too: when a game closes, focus often lands back on Kestrel.
            WINEVENT_OUTOFCONTEXT,
        )
    };
    (!hook.is_invalid()).then_some(hook)
}

pub fn stop(hook: HWINEVENTHOOK) {
    unsafe {
        let _ = UnhookWinEvent(hook);
    }
}

/// True once after each foreground change since the last call.
pub fn take_change() -> bool {
    CHANGED.swap(false, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quieted_entries() {
        let q = parse_quieted("12:133000:32:discord.exe;bad;7:1:16384:chrome.exe");
        assert_eq!(q.len(), 2);
        assert_eq!((q[0].pid, q[0].created, q[0].priority, q[0].name.as_str()), (12, 133000, 32, "discord.exe"));
        assert_eq!(q[1].name, "chrome.exe");
        assert!(parse_quieted("").is_empty());
    }

    /// Efficiency mode on and off for a real process this test starts: `cargo test live_quiet -- --ignored`.
    #[test]
    #[ignore]
    fn live_quiet_round_trip() {
        use windows::Win32::System::Threading::GetPriorityClass;
        let mut child = std::process::Command::new("charmap.exe").spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(800));
        let names = vec!["charmap.exe".to_string()];
        let saved = quiet_snapshot(&names);
        println!("saved: {saved}");
        let prio = |pid: u32| open_for_priority(pid).map(|h| unsafe { GetPriorityClass(h) });
        println!("quiet: {:?}", quiet(&saved, &names));
        let first = &parse_quieted(&saved)[0];
        assert_eq!(prio(first.pid), Some(0x4000), "below normal while quiet");
        println!("{}", unquiet(&saved));
        assert_eq!(prio(first.pid), Some(first.priority), "original priority back");
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn protects_essentials() {
        assert!(protected("explorer.exe").is_some());
        assert!(protected("MsMpEng.exe").is_some());
        assert!(protected("EasyAntiCheat_EOS.exe").is_some());
        assert!(protected("vgc.exe").is_some());
        assert!(protected("discord.exe").is_none());
        assert!(protected("chrome.exe").is_none());
    }
}
