//! Game detection and closing apps, without polling and without touching game processes.
//!
//! Detection listens for Windows' "foreground window changed" event (SetWinEventHook), which costs
//! nothing until you switch windows. Each event triggers one process snapshot (Toolhelp), which reads
//! a list kept by Windows and never opens a process, so anti-cheat software has nothing to object to.

use std::collections::HashSet;
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
    use super::protected;

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
