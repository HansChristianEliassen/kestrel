//! Optional notification-area icon, so automatic Game Mode keeps working after the window is closed.
//! It exists only while the user has turned it on; clicking it reopens Kestrel, its menu can exit.
//! Messages arrive on the UI thread's own message loop, through a hidden window that is never shown.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use eframe::egui;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow,
    GetCursorPos, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, HICON, ICONINFO, MF_SEPARATOR, MF_STRING,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
};

use super::wide;

const CALLBACK: u32 = WM_APP + 1;
const CMD_OPEN: usize = 1;
const CMD_EXIT: usize = 2;

/// What the user asked for through the icon: 0 nothing, 1 open, 2 exit.
static REQUEST: AtomicU8 = AtomicU8::new(0);
static WAKE: OnceLock<egui::Context> = OnceLock::new();
/// Explorer broadcasts this after it restarts; the icon has to be added again.
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

pub enum Request {
    Open,
    Exit,
}

pub struct Tray {
    data: NOTIFYICONDATAW,
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == CALLBACK {
        let event = (lp.0 & 0xFFFF) as u32;
        let request = match event {
            WM_LBUTTONUP => CMD_OPEN,
            WM_RBUTTONUP | WM_CONTEXTMENU => menu(hwnd),
            _ => 0,
        };
        if request != 0 {
            REQUEST.store(request as u8, Ordering::Relaxed);
            if let Some(ctx) = WAKE.get() {
                ctx.request_repaint();
            }
        }
        return LRESULT(0);
    }
    if Some(&msg) == TASKBAR_CREATED.get() {
        if let Some(ctx) = WAKE.get() {
            REQUEST.store(3, Ordering::Relaxed); // re-add; handled by `take`
            ctx.request_repaint();
        }
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// Shows Open / Exit at the cursor and returns the chosen command (0 when dismissed).
unsafe fn menu(hwnd: HWND) -> usize {
    let Ok(m) = CreatePopupMenu() else { return 0 };
    let _ = AppendMenuW(m, MF_STRING, CMD_OPEN, w!("Open Kestrel"));
    let _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(m, MF_STRING, CMD_EXIT, w!("Exit (ends Game Mode)"));
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    // Required so the menu closes when the user clicks elsewhere.
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None).0 as usize;
    let _ = DestroyMenu(m);
    cmd
}

/// HICON from RGBA pixels (straight alpha), using a 32-bit color bitmap.
fn make_icon(rgba: &[u8], size: u32) -> Option<HICON> {
    let bgra: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    let mask = vec![0u8; (size * size / 8) as usize];
    unsafe {
        let color = CreateBitmap(size as i32, size as i32, 1, 32, Some(bgra.as_ptr().cast()));
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, Some(mask.as_ptr().cast()));
        let icon = CreateIconIndirect(&ICONINFO { fIcon: true.into(), hbmMask: mask, hbmColor: color, ..Default::default() }).ok();
        let _ = DeleteObject(color);
        let _ = DeleteObject(mask);
        icon
    }
}

impl Tray {
    /// Adds the icon. Must run on the UI thread, whose message loop delivers the clicks.
    pub fn add(ctx: &egui::Context, icon: &egui::IconData) -> Option<Tray> {
        let _ = WAKE.set(ctx.clone());
        unsafe {
            let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
            let instance = GetModuleHandleW(None).ok()?.into();
            let class = w!("Kestrel.TrayWindow");
            // Fails harmlessly when the class already exists (icon turned off and on again).
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance, lpszClassName: class, ..Default::default() });
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                // A hidden top-level window rather than a message-only one: only top-level windows get TaskbarCreated.
                None,
                None,
                instance,
                None,
            )
            .ok()?;
            let mut data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: CALLBACK,
                hIcon: make_icon(&icon.rgba, icon.width)?,
                ..Default::default()
            };
            let tip = wide("Kestrel: watching for games");
            data.szTip[..tip.len()].copy_from_slice(&tip);
            if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
                let _ = DestroyIcon(data.hIcon);
                let _ = DestroyWindow(hwnd);
                return None;
            }
            Some(Tray { data })
        }
    }

    /// The user's latest click on the icon, if any.
    pub fn take(&self) -> Option<Request> {
        match REQUEST.swap(0, Ordering::Relaxed) as usize {
            CMD_OPEN => Some(Request::Open),
            CMD_EXIT => Some(Request::Exit),
            3 => {
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_ADD, &self.data);
                }
                None
            }
            _ => None,
        }
    }
}

/// Kestrel's main window: this process's visible top-level window titled "Kestrel".
fn main_window() -> Option<HWND> {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};
    unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> BOOL {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut title = [0u16; 16];
        let n = GetWindowTextW(hwnd, &mut title) as usize;
        if pid == std::process::id() && IsWindowVisible(hwnd).as_bool() && String::from_utf16_lossy(&title[..n]) == "Kestrel" {
            *(out.0 as *mut Option<HWND>) = Some(hwnd);
            return false.into();
        }
        true.into()
    }
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

/// Adds or removes the main window's taskbar button. Kestrel "closes to the tray" by minimizing without a
/// taskbar button: a hidden (SW_HIDE) eframe window keeps repainting and uses CPU, a minimized one doesn't.
pub fn taskbar_button(show: bool) {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};
    let Some(hwnd) = main_window() else { return };
    super::com_init();
    unsafe {
        let Ok(list) = CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_INPROC_SERVER) else { return };
        if list.HrInit().is_ok() {
            let _ = if show { list.AddTab(hwnd) } else { list.DeleteTab(hwnd) };
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
            let _ = DestroyIcon(self.data.hIcon);
            let _ = DestroyWindow(self.data.hWnd);
        }
    }
}
