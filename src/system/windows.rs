//! Thin wrappers over the documented Win32 APIs the rest of the app needs.

use std::collections::HashSet;
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::WAIT_TIMEOUT;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    IsUserAnAdmin, SHEmptyRecycleBinW, SHQueryRecycleBinW, ShellExecuteExW, SEE_MASK_DOENVSUBST, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHQUERYRBINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, SW_SHOWNORMAL, WM_SETTINGCHANGE};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn env_path(name: &str) -> PathBuf {
    std::env::var_os(name).map(PathBuf::from).unwrap_or_default()
}

/// Windows build number (19045 = Windows 10 22H2, 22000+ = Windows 11); 0 if unknown.
pub fn windows_build() -> u32 {
    winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|b| b.trim().parse().ok())
        .unwrap_or(0)
}

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

/// Whether a Windows service is running right now (false when unknown).
pub fn service_state(name: &str) -> bool {
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
        SERVICE_STATUS,
    };
    let n = wide(name);
    unsafe {
        let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else { return false };
        let mut status = SERVICE_STATUS::default();
        let running = match OpenServiceW(scm, PCWSTR(n.as_ptr()), SERVICE_QUERY_STATUS) {
            Ok(svc) => {
                let ok = QueryServiceStatus(svc, &mut status).is_ok();
                let _ = CloseServiceHandle(svc);
                ok && status.dwCurrentState == SERVICE_RUNNING
            }
            Err(_) => false,
        };
        let _ = CloseServiceHandle(scm);
        running
    }
}

/// Whether a Windows service is doing anything other than standing stopped. Unknown counts as running.
pub fn service_running(name: &str) -> bool {
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS, SERVICE_STATUS,
        SERVICE_STOPPED,
    };
    let n = wide(name);
    unsafe {
        let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else { return true };
        let mut status = SERVICE_STATUS::default();
        let stopped = match OpenServiceW(scm, PCWSTR(n.as_ptr()), SERVICE_QUERY_STATUS) {
            Ok(svc) => {
                let ok = QueryServiceStatus(svc, &mut status).is_ok();
                let _ = CloseServiceHandle(svc);
                ok && status.dwCurrentState == SERVICE_STOPPED
            }
            Err(_) => false,
        };
        let _ = CloseServiceHandle(scm);
        !stopped
    }
}

/// Launches `file` through the shell (UAC prompts are handled by Windows).
/// With `wait`, waits for it to exit (or for the flag to be set) and returns the exit code.
pub fn shell_execute(file: &str, params: &str, elevate: bool, wait: Option<&AtomicBool>) -> Result<u32, String> {
    let (f, p) = (wide(file), wide(params));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_DOENVSUBST,
        lpVerb: if elevate { w!("runas") } else { w!("open") },
        lpFile: PCWSTR(f.as_ptr()),
        lpParameters: PCWSTR(p.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.message().to_string())?;
    let mut code = 0u32;
    if !info.hProcess.is_invalid() {
        unsafe {
            if let Some(stop) = wait {
                while WaitForSingleObject(info.hProcess, 500) == WAIT_TIMEOUT && !stop.load(Ordering::Relaxed) {}
                let _ = GetExitCodeProcess(info.hProcess, &mut code);
            }
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(code)
}

/// Sets up COM on a worker thread (needed for Task Scheduler, shortcuts and the package API).
/// Harmless when the thread already has COM.
pub fn com_init() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// The program a .lnk shortcut opens.
pub fn shortcut_target(lnk: &str) -> Option<String> {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    com_init();
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.cast::<IPersistFile>().ok()?.Load(&HSTRING::from(lnk), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        (len > 0).then(|| String::from_utf16_lossy(&buf[..len]))
    }
}

/// Windows' own Open dialog, filtered to programs. Blocks until the user picks a file or cancels.
pub fn pick_exe(title: &str) -> Option<String> {
    pick(title, false)
}

/// Windows' own folder picker. Blocks until the user picks a folder or cancels.
pub fn pick_folder(title: &str) -> Option<String> {
    pick(title, true)
}

fn pick(title: &str, folder: bool) -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_PICKFOLDERS, SIGDN_FILESYSPATH};
    unsafe {
        // The UI thread may already have COM set up; that's fine either way.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        if folder {
            dialog.SetOptions(dialog.GetOptions().ok()? | FOS_PICKFOLDERS).ok()?;
        } else {
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: w!("Programs"), pszSpec: w!("*.exe") }]).ok()?;
        }
        dialog.SetTitle(&HSTRING::from(title)).ok()?;
        dialog.Show(None).ok()?; // cancelled
        let path = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const c_void));
        s
    }
}

pub fn open_uri(uri: &str) {
    let _ = shell_execute(uri, "", false, None);
}

/// Opens File Explorer with the file selected.
pub fn show_in_folder(path: &std::path::Path) {
    let _ = shell_execute("explorer.exe", &format!("/select,\"{}\"", path.display()), false, None);
}

/// Starts a new elevated instance and exits this one.
/// `page` (a fixed name like "measure") is where the new window opens, so the user lands back where they were.
pub fn relaunch_as_admin(page: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    shell_execute(&exe.to_string_lossy(), &format!("{RELAUNCHED} {PAGE_ARG}{page}"), true, None)?;
    std::process::exit(0);
}

/// Prefix of the argument naming the page an elevated relaunch opens on.
pub const PAGE_ARG: &str = "--page=";

/// Argument an elevated relaunch passes, so it waits for the old window to close instead of exiting.
pub const RELAUNCHED: &str = "--relaunched";
/// Set by the "Start with Windows" entry: begin minimized (in the tray when it is on).
pub const START_ARG: &str = "--startup";

/// Makes this the only running Kestrel. If another one is open, brings it to the front and returns false,
/// so two windows can never change the same settings or backup file at once.
pub fn claim_single_instance(relaunched: bool) -> bool {
    use windows::Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE};
    unsafe {
        let Ok(mutex) = CreateMutexW(None, false, w!("Local\\Kestrel.SingleInstance")) else { return true };
        // An elevated relaunch waits for the old instance to exit (its mutex is then released or abandoned).
        match WaitForSingleObject(mutex, if relaunched { 10_000 } else { 0 }) {
            WAIT_OBJECT_0 | WAIT_ABANDONED => true, // keep the handle open for the life of the process
            _ => {
                if let Ok(hwnd) = FindWindowW(None, w!("Kestrel")) {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    let _ = SetForegroundWindow(hwnd);
                }
                false
            }
        }
    }
}

/// Runs a PowerShell snippet without a console window; returns stdout or the first error line.
pub fn powershell(script: &str) -> Result<String, String> {
    let script = format!("[Console]::OutputEncoding=[Text.Encoding]::UTF8; $ErrorActionPreference='Stop'; {script}");
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("PowerShell command failed").to_string())
    }
}

/// Creates a System Restore point (needs admin and System Protection on the system drive).
pub fn create_restore_point() -> Result<String, String> {
    if !is_admin() {
        return Err("Creating a restore point needs administrator rights (Settings > Restart as administrator).".into());
    }
    // Windows silently allows only one restore point per 24 hours, so compare the count before and after.
    let out = powershell(
        "$b=@(Get-ComputerRestorePoint).Count; \
         Checkpoint-Computer -Description 'Kestrel' -RestorePointType MODIFY_SETTINGS -WarningAction SilentlyContinue; \
         if (@(Get-ComputerRestorePoint).Count -gt $b) { 'created' } else { 'skipped' }",
    )?;
    let msg = if out.contains("created") {
        "Restore point created. You can roll back from System Properties > System Restore."
    } else {
        "Windows skipped it because a restore point was already made in the last 24 hours. That one is still available."
    };
    crate::utils::log(&format!("Restore point: {msg}"));
    Ok(msg.into())
}

/// Runs a console tool hidden and returns stdout.
pub fn run_hidden(exe: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("{exe} failed: {}", String::from_utf8_lossy(&out.stdout).trim()))
    }
}

/// Lower-cased executable names of all running processes.
pub fn running_processes() -> HashSet<String> {
    process_list().into_iter().map(|(_, name)| name).collect()
}

/// (process id, lower-cased executable name) for every running process. Uses a snapshot,
/// so no process (including anti-cheat-protected games) is ever opened.
pub fn process_list() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.push((e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase()));
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// CompanyName from an executable's version resource.
pub fn file_company(path: &str) -> Option<String> {
    let p = wide(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(p.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(p.as_ptr()), 0, size, buf.as_mut_ptr().cast()).ok()?;
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(buf.as_ptr().cast(), w!("\\VarFileInfo\\Translation"), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let tr = std::ptr::read_unaligned(ptr as *const [u16; 2]);
        let key = wide(&format!("\\StringFileInfo\\{:04x}{:04x}\\CompanyName", tr[0], tr[1]));
        if !VerQueryValueW(buf.as_ptr().cast(), PCWSTR(key.as_ptr()), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr as *const u16, len as usize));
        let s = s.trim_end_matches('\0').trim();
        (!s.is_empty()).then(|| s.to_string())
    }
}

pub fn expand_env(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let src = wide(s);
    let mut buf = vec![0u16; 2048];
    let n = unsafe { ExpandEnvironmentStringsW(PCWSTR(src.as_ptr()), Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return s.to_string();
    }
    String::from_utf16_lossy(&buf[..n - 1])
}

/// Splits a command line into (executable, arguments).
pub fn split_command(cmd: &str) -> (String, String) {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (rest[..end].to_string(), rest[end + 1..].trim().to_string());
        }
    }
    match cmd.to_ascii_lowercase().find(".exe") {
        Some(i) => (cmd[..i + 4].to_string(), cmd[i + 4..].trim().to_string()),
        None => (cmd.to_string(), String::new()),
    }
}

pub fn local_time() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute)
}

/// (bytes, items) in the Recycle Bins of all drives.
pub fn recycle_bin_info() -> Option<(u64, u64)> {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.ok()?;
    Some((info.i64Size as u64, info.i64NumItems as u64))
}

pub fn empty_recycle_bin() -> Result<(), String> {
    unsafe { SHEmptyRecycleBinW(HWND::default(), PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) }
        .map_err(|e| e.message().to_string())
}

/// Tells running apps (Explorer, the shell) that a setting changed.
pub fn broadcast_setting_change(area: &str) {
    let a = wide(area);
    unsafe {
        let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(a.as_ptr() as isize), SMTO_ABORTIFHUNG, 1000, None);
    }
}

#[cfg(test)]
mod tests {
    use super::split_command;

    #[test]
    fn splits_commands() {
        assert_eq!(split_command(r#""C:\A B\u.exe" /S"#), (r"C:\A B\u.exe".into(), "/S".into()));
        assert_eq!(split_command(r"C:\x\unins000.exe /SILENT"), (r"C:\x\unins000.exe".into(), "/SILENT".into()));
        assert_eq!(split_command("MsiExec.exe /X{ABC}"), ("MsiExec.exe".into(), "/X{ABC}".into()));
        assert_eq!(split_command("rundll32"), ("rundll32".into(), "".into()));
    }
}
