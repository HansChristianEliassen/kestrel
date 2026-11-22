//! Health checks. They report and link to the right Windows page; the only change any of them
//! can make is the refresh-rate fix, which the user starts and which reverts unless they keep it.

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsW, CDS_TEST, CDS_UPDATEREGISTRY, DEVMODEW, DISPLAY_DEVICEW,
    DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::AF_UNSPEC;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceSeekPenaltyProperty, DEVICE_SEEK_PENALTY_DESCRIPTOR, IOCTL_STORAGE_QUERY_PROPERTY,
    STORAGE_PROPERTY_QUERY,
};
use windows::Win32::System::SystemInformation::{GetSystemFirmwareTable, FIRMWARE_TABLE_PROVIDER};
use windows::Win32::System::WindowsProgramming::DRIVE_FIXED;
use windows::Win32::System::IO::DeviceIoControl;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use super::games;
use super::tweaks::{get_kv, read_sz};
use crate::system::{env_path, local_time, running_processes, wide};
use crate::ui::fmt_bytes;
use crate::utils::{backup, log};

pub struct Check {
    pub title: &'static str,
    pub detail: String,
    /// Something the user should probably fix.
    pub attention: bool,
    /// (button label, URI)
    pub link: Option<(&'static str, &'static str)>,
    /// Displays below their maximum refresh rate: (device, current Hz, max Hz).
    pub refresh_fix: Vec<(String, u32, u32)>,
}

pub fn run() -> Vec<Check> {
    vec![
        refresh_rate(),
        gpu_driver(),
        memory_speed(),
        memory_in_use(),
        storage(),
        drive_health(),
        network(),
        overlays(),
        gpu_scheduling(),
        graphics_features(),
        memory_integrity(),
        restart_pending(),
    ]
}

/// "M-D-YYYY" (the display-driver registry format) as (year, month).
fn parse_driver_date(s: &str) -> Option<(i32, i32)> {
    let mut p = s.split('-').map(|n| n.trim().parse::<i32>().ok());
    let (m, _d, y) = (p.next()??, p.next()??, p.next()??);
    (1..=12).contains(&m).then_some((y, m))
}

/// Display adapters from the display device class (readable without admin): (name, version, date).
fn gpus() -> Vec<(String, String, String)> {
    let Ok(class) =
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}")
    else {
        return vec![];
    };
    class
        .enum_keys()
        .flatten()
        .filter_map(|sub| {
            let k = class.open_subkey(&sub).ok()?;
            let get = |v: &str| k.get_value::<String, _>(v).ok();
            let name = get("DriverDesc")?;
            // Placeholder adapters, not real GPUs.
            if name.contains("Basic Display") || name.contains("Basic Render") || name.contains("Remote Display") {
                return None;
            }
            Some((name, get("DriverVersion").unwrap_or_default(), get("DriverDate").unwrap_or_default()))
        })
        .collect()
}

/// Each graphics adapter: (name, driver version, driver age in months if known).
pub(crate) fn gpu_drivers() -> Vec<(String, String, Option<i32>)> {
    let now = local_time();
    let (y, m) = (now[..4].parse::<i32>().unwrap_or(0), now[5..7].parse::<i32>().unwrap_or(0));
    gpus()
        .into_iter()
        .map(|(name, version, date)| {
            let months = parse_driver_date(&date).map(|(dy, dm)| ((y * 12 + m) - (dy * 12 + dm)).max(0));
            (name, version, months)
        })
        .collect()
}

/// "this month" / "N month(s) old".
pub(crate) fn driver_age(months: i32) -> String {
    if months == 0 {
        "this month".into()
    } else {
        format!("{months} month(s) old")
    }
}

/// The GPU maker's official driver page.
pub(crate) fn gpu_vendor(name: &str) -> (&'static str, &'static str) {
    let n = name.to_lowercase();
    if n.contains("nvidia") {
        ("NVIDIA drivers", "https://www.nvidia.com/en-us/drivers/")
    } else if n.contains("amd") || n.contains("radeon") {
        ("AMD drivers", "https://www.amd.com/en/support/download/drivers.html")
    } else {
        ("Intel drivers", "https://www.intel.com/content/www/us/en/download-center/home.html")
    }
}

fn gpu_driver() -> Check {
    let list = gpu_drivers();
    let old = list.iter().any(|(.., months)| months.is_some_and(|m| m > 12));
    let lines: Vec<String> = list
        .iter()
        .map(|(name, version, months)| match months {
            Some(m) => format!("{name}: driver {version}, {}", driver_age(*m)),
            None => format!("{name}: driver {version}"),
        })
        .collect();
    let link = list.first().map(|(name, ..)| gpu_vendor(name));
    let mut detail = if lines.is_empty() { "Couldn't read graphics driver details.".to_string() } else { lines.join(". ") + "." };
    if old {
        detail.push_str(" Drivers over a year old often miss performance fixes for newer games; get the current one from your GPU maker.");
    }
    check("Graphics driver", detail, old, link)
}

/// Running programs that hook into games (overlays) or tune the PC like Kestrel does (boosters).
const OVERLAYS: &[(&str, &str)] = &[
    ("rtss.exe", "RivaTuner Statistics Server (on-screen display and frame limiter)"),
    ("msiafterburner.exe", "MSI Afterburner"),
    ("overwolf.exe", "Overwolf"),
    ("nvidia overlay.exe", "NVIDIA overlay"),
    ("discord.exe", "Discord (its in-game overlay is on by default)"),
    ("medal.exe", "Medal"),
    ("outplayed.exe", "Outplayed"),
    ("gamebar.exe", "Xbox Game Bar"),
    ("gameoverlayui.exe", "Steam overlay"),
];
const BOOSTERS: &[(&str, &str)] = &[
    ("razercortex.exe", "Razer Cortex"),
    ("processlasso.exe", "Process Lasso"),
    ("hone.exe", "Hone"),
    ("wisegamebooster.exe", "Wise Game Booster"),
    ("asc.exe", "Advanced SystemCare"),
    ("ccleaner64.exe", "CCleaner"),
];

fn overlays() -> Check {
    let running = running_processes();
    let found = |list: &[(&str, &'static str)]| -> Vec<&'static str> {
        list.iter().filter(|(exe, _)| running.contains(*exe)).map(|(_, n)| *n).collect()
    };
    let (overlays, boosters) = (found(OVERLAYS), found(BOOSTERS));
    let mut detail = String::new();
    if overlays.is_empty() && boosters.is_empty() {
        detail.push_str("No known overlays or other optimizers are running.");
    }
    if !overlays.is_empty() {
        detail.push_str(&format!(
            "Overlays running: {}. Each one hooks into games; a few can cause stutter or clash with anti-cheat. Turn off the ones you don't use in their own settings. ",
            overlays.join(", ")
        ));
    }
    if !boosters.is_empty() {
        detail.push_str(&format!(
            "Other optimizers running: {}. Two tools changing the same settings can undo each other; use one.",
            boosters.join(", ")
        ));
    }
    check("Overlays and other optimizers", detail.trim().to_string(), !boosters.is_empty(), None)
}

/// (drive letter, solid-state, free bytes, total bytes) for each fixed drive.
pub(crate) fn drives() -> Vec<(char, Option<bool>, u64, u64)> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .filter_map(|i| {
            let letter = (b'A' + i) as char;
            let root = wide(&format!("{letter}:\\"));
            if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } != DRIVE_FIXED {
                return None;
            }
            let (mut free, mut total) = (0u64, 0u64);
            unsafe { GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), None, Some(&mut total), Some(&mut free)) }.ok()?;
            Some((letter, is_ssd(letter), free, total))
        })
        .collect()
}

/// Whether the drive's disk has no seek penalty (SSD/NVMe). None when Windows can't tell.
fn is_ssd(letter: char) -> Option<bool> {
    let path = wide(&format!(r"\\.\{letter}:"));
    unsafe {
        let h = CreateFileW(
            PCWSTR(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .ok()?;
        let query =
            STORAGE_PROPERTY_QUERY { PropertyId: StorageDeviceSeekPenaltyProperty, QueryType: PropertyStandardQuery, ..Default::default() };
        let mut desc = DEVICE_SEEK_PENALTY_DESCRIPTOR::default();
        let ok = DeviceIoControl(
            h,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const c_void),
            std::mem::size_of_val(&query) as u32,
            Some(&mut desc as *mut _ as *mut c_void),
            std::mem::size_of_val(&desc) as u32,
            None,
            None,
        )
        .is_ok();
        let _ = CloseHandle(h);
        ok.then(|| !desc.IncursSeekPenalty.as_bool())
    }
}

fn storage() -> Check {
    let game_drives = games::install_drives();
    let system = env_path("SystemDrive").to_string_lossy().chars().next().unwrap_or('C').to_ascii_uppercase();
    let mut attention = false;
    let mut notes = Vec::new();
    let lines: Vec<String> = drives()
        .into_iter()
        .map(|(letter, ssd, free, total)| {
            let pct = (free * 100).checked_div(total).unwrap_or(0);
            let has_games = game_drives.contains(&letter);
            let kind = match ssd {
                Some(true) => "SSD",
                Some(false) => "hard drive",
                None => "unknown type",
            };
            if has_games && ssd == Some(false) {
                attention = true;
                notes.push(format!("Games on {letter}: are on a hard drive, which makes loading and texture streaming slower and can cause stutter. Moving them to an SSD helps most."));
            }
            if (has_games || letter == system) && pct < 10 {
                attention = true;
                notes.push(format!("{letter}: is almost full. Windows and games slow down when a drive has under about 10% free; try the System Cleaner or move some files."));
            }
            format!("{letter}: {kind}, {} free of {} ({pct}%){}", fmt_bytes(free), fmt_bytes(total), if has_games { ", has games" } else { "" })
        })
        .collect();
    let mut detail = lines.join(". ") + ".";
    for n in notes {
        detail.push(' ');
        detail.push_str(&n);
    }
    check("Storage", detail, attention, None)
}

fn drive_health() -> Check {
    let list = super::drive_health::drives();
    let attention = list.iter().any(|d| d.2);
    let detail = if list.is_empty() {
        "Couldn't read the drives' health.".to_string()
    } else {
        list.iter().map(|(name, text, _)| format!("{name}: {text}")).collect::<Vec<_>>().join(". ") + "."
    };
    check("Drive health", detail, attention, Some(("Disks & volumes", "ms-settings:disksandvolumes")))
}

fn network() -> Check {
    let mut wired = Vec::new();
    let mut wifi = Vec::new();
    unsafe {
        let mut size = 0u32;
        let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let _ = GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut size);
        let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
        let first = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        if GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(first), &mut size) == 0 {
            let mut a = first;
            while !a.is_null() {
                let ad = &*a;
                // Only adapters that are up and have a gateway, i.e. the ones actually reaching the internet.
                if ad.OperStatus == IfOperStatusUp && !ad.FirstGatewayAddress.is_null() {
                    let speed = ad.ReceiveLinkSpeed.max(ad.TransmitLinkSpeed);
                    let speed = if speed >= 1_000_000_000 {
                        format!("{} Gbps", speed / 1_000_000_000)
                    } else {
                        format!("{} Mbps", speed / 1_000_000)
                    };
                    match ad.IfType {
                        IF_TYPE_IEEE80211 => wifi.push(speed),
                        IF_TYPE_ETHERNET_CSMACD => wired.push(speed),
                        _ => {}
                    }
                }
                a = ad.Next;
            }
        }
    }
    let detail = match (wired.first(), wifi.first()) {
        (Some(s), _) => format!("Connected by Ethernet at {s}. A wired connection gives the steadiest ping."),
        (None, Some(s)) => format!(
            "Connected by Wi-Fi (link speed {s}). Wi-Fi works, but Ethernet gives steadier ping in online games if you can run a cable."
        ),
        (None, None) => "No active Ethernet or Wi-Fi connection found.".to_string(),
    };
    check("Network connection", detail, false, None)
}

/// Status only: entry names as Windows' Graphics settings store them; Kestrel doesn't change these.
fn graphics_features() -> Check {
    let s = read_sz(r"Software\Microsoft\DirectX\UserGpuPreferences", "DirectXUserGlobalSettings").unwrap_or_default();
    let state = |entry| match get_kv(&s, entry) {
        Some("1") => "on",
        Some("0") => "off",
        _ => "Windows default",
    };
    check(
        "Variable refresh rate and Auto HDR",
        format!(
            "Variable refresh rate setting: {}. Auto HDR: {}. If you have a G-SYNC/FreeSync or HDR monitor, check these in Graphics settings.",
            state("VRROptimizeEnable"),
            state("AutoHDREnable")
        ),
        false,
        Some(("Graphics settings", "ms-settings:display-advancedgraphics")),
    )
}

/// How full memory is right now, and which open apps use the most. A one-off reading, not monitoring.
fn memory_in_use() -> Check {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_err() {
        return check("Memory in use", "Couldn't read memory use.".into(), false, None);
    }
    let games: Vec<String> = crate::gamemode::Config::load().games.iter().map(|g| g.file_name()).collect();
    let top: Vec<String> = crate::gamemode::watch::app_memory(&games)
        .into_iter()
        .filter(|(_, bytes)| *bytes > 0)
        .take(4)
        .map(|(name, bytes)| format!("{} {}", name.trim_end_matches(".exe"), fmt_bytes(bytes)))
        .collect();
    let attention = m.dwMemoryLoad >= 80;
    let mut detail =
        format!("{} of {} in use ({}%).", fmt_bytes(m.ullTotalPhys - m.ullAvailPhys), fmt_bytes(m.ullTotalPhys), m.dwMemoryLoad);
    if !top.is_empty() {
        detail.push_str(&format!(" Largest open apps: {}.", top.join(", ")));
    }
    if attention {
        detail.push_str(" With this little free, games can stutter while Windows swaps to disk. Close apps you don't need before playing; Game Mode can close a list of apps for you.");
    }
    check("Memory in use", detail, attention, None)
}

/// A restart Windows is waiting for (installed updates or servicing), which can interrupt a game session.
fn restart_pending() -> Check {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let pending = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending",
    ]
    .iter()
    .any(|k| hklm.open_subkey(k).is_ok());
    let detail = if pending {
        "Windows has installed updates and is waiting to restart. Restart before a long session so it doesn't happen mid-game."
    } else {
        "No restart is waiting for Windows Update."
    };
    check("Pending restart", detail.into(), pending, pending.then_some(("Windows Update", "ms-settings:windowsupdate")))
}

/// "About this PC": one line per part, for Settings and for pasting into support forums. Read-only, fast.
pub fn system_summary() -> Vec<(&'static str, String)> {
    let cv = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").ok();
    let reg = |name: &str| cv.as_ref().and_then(|k| k.get_value::<String, _>(name).ok()).unwrap_or_default();
    let ubr = cv.as_ref().and_then(|k| k.get_value::<u32, _>("UBR").ok()).unwrap_or(0);
    // EditionID uses internal names; the common ones get the names people know.
    let edition = match reg("EditionID").as_str() {
        "Core" | "CoreSingleLanguage" => "Home".to_string(),
        "Professional" => "Pro".to_string(),
        other => other.to_string(),
    };
    let windows = format!("Windows 11 {edition} {} (build {}.{ubr})", reg("DisplayVersion"), reg("CurrentBuild"));
    let cpu = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
        .and_then(|k| k.get_value::<String, _>("ProcessorNameString"))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    let memory = {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        let total = if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() { fmt_bytes(m.ullTotalPhys) } else { "Unknown".into() };
        match smbios_table().as_deref().and_then(parse_memory) {
            Some((cur, rated)) if cur < rated => format!("{total} at {cur} MT/s (modules rated {rated} MT/s)"),
            Some((cur, _)) => format!("{total} at {cur} MT/s"),
            None => total,
        }
    };
    let join = |v: Vec<String>| if v.is_empty() { "Unknown".to_string() } else { v.join("; ") };
    let graphics = join(gpus().into_iter().map(|(name, version, _)| format!("{name}, driver {version}")).collect());
    let screens = join(
        displays()
            .into_iter()
            .map(|(device, cur, max)| format!("{} at {cur} Hz (max {max} Hz)", device.trim_start_matches(r"\\.\")))
            .collect(),
    );
    let disks = join(super::drive_health::drives().into_iter().map(|(name, ..)| name).collect());
    vec![
        ("Windows", windows),
        ("Processor", format!("{cpu}, {threads} threads")),
        ("Memory", memory),
        ("Graphics", graphics),
        ("Displays", screens),
        ("Drives", disks),
    ]
}

fn check(title: &'static str, detail: String, attention: bool, link: Option<(&'static str, &'static str)>) -> Check {
    Check { title, detail, attention, link, refresh_fix: Vec::new() }
}

/// Switches a display to `hz` at its current resolution. Windows tests the mode first.
pub fn set_refresh_rate(device: &str, hz: u32) -> Result<(), String> {
    let name = wide(device);
    let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
    if !unsafe { EnumDisplaySettingsW(PCWSTR(name.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm) }.as_bool() {
        return Err(format!("Couldn't read the current mode of {device}."));
    }
    dm.dmDisplayFrequency = hz;
    dm.dmFields = DM_DISPLAYFREQUENCY;
    for flags in [CDS_TEST, CDS_UPDATEREGISTRY] {
        let r = unsafe { ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), Some(&dm), HWND::default(), flags, None) };
        if r != DISP_CHANGE_SUCCESSFUL {
            return Err(format!("Windows refused {hz} Hz on {device} (code {}).", r.0));
        }
    }
    log(&format!("Display: set {device} to {hz} Hz"));
    Ok(())
}

/// Remembers a display's refresh rate from before Kestrel's change, so Restore all can put it back.
pub fn keep_refresh_rate(device: &str, old_hz: u32) -> Result<(), String> {
    backup::save_original(&format!("display:{device}"), || Ok(old_hz.to_string()))?;
    log(&format!("Display: kept the new refresh rate on {device} (was {old_hz} Hz)"));
    Ok(())
}

/// Puts back refresh rates Kestrel changed. One line per display.
pub fn restore_displays() -> Vec<String> {
    backup::with_prefix("display:")
        .into_iter()
        .map(|(device, hz)| {
            let result = hz
                .parse()
                .map_err(|_| "saved value is invalid".to_string())
                .and_then(|hz| set_refresh_rate(&device, hz))
                .and_then(|()| backup::remove(&format!("display:{device}")));
            match result {
                Ok(()) => format!("✔ Restored refresh rate: {} ({hz} Hz)", device.trim_start_matches(r"\\.\")),
                Err(e) => format!("✖ Refresh rate for {device}: {e}"),
            }
        })
        .collect()
}

/// (device, current Hz, highest Hz at the current resolution) per active display.
fn displays() -> Vec<(String, u32, u32)> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut d = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut d, 0) }.as_bool() {
            break;
        }
        if d.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0 {
            continue;
        }
        let name = PCWSTR(d.DeviceName.as_ptr());
        let new_mode = || DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        let mut cur = new_mode();
        if !unsafe { EnumDisplaySettingsW(name, ENUM_CURRENT_SETTINGS, &mut cur) }.as_bool() {
            continue;
        }
        let mut max = cur.dmDisplayFrequency;
        let mut m = new_mode();
        let mut n = 0;
        while unsafe { EnumDisplaySettingsW(name, ENUM_DISPLAY_SETTINGS_MODE(n), &mut m) }.as_bool() {
            if m.dmPelsWidth == cur.dmPelsWidth && m.dmPelsHeight == cur.dmPelsHeight {
                max = max.max(m.dmDisplayFrequency);
            }
            n += 1;
        }
        let device = String::from_utf16_lossy(&d.DeviceName).trim_end_matches('\0').to_string();
        out.push((device, cur.dmDisplayFrequency, max));
    }
    out
}

fn refresh_rate() -> Check {
    let shown = displays();
    // 59 vs 60 Hz is the same mode (59.94 Hz rounding), so ignore a 1 Hz gap.
    let slow: Vec<_> = shown.iter().filter(|(_, c, m)| c + 1 < *m).cloned().collect();
    let detail = if shown.is_empty() {
        "Couldn't read display modes.".to_string()
    } else if slow.is_empty() {
        let list: Vec<String> = shown.iter().map(|(_, c, _)| format!("{c} Hz")).collect();
        format!("Every display runs at its highest refresh rate ({}).", list.join(", "))
    } else {
        let list: Vec<String> = slow.iter().map(|(_, c, m)| format!("{c} Hz (supports {m} Hz)")).collect();
        format!("A display runs below its maximum: {}.", list.join(", "))
    };
    let attention = !slow.is_empty();
    Check {
        refresh_fix: slow,
        ..check("Display refresh rate", detail, attention, Some(("Display settings", "ms-settings:display-advanced")))
    }
}

/// (lowest configured speed, highest speed the modules report) in MT/s, from SMBIOS memory-device
/// structures (type 17): speed at offset 0x15, configured speed at 0x20, 32-bit versions at 0x54/0x58.
fn parse_memory(table: &[u8]) -> Option<(u32, u32)> {
    let word = |s: &[u8], at: usize| s.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u32);
    let dword = |s: &[u8], at: usize| s.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let speed = |s: &[u8], short: usize, long: usize| match word(s, short)? {
        0xFFFF => dword(s, long),
        v => Some(v),
    };
    let (mut configured, mut rated) = (u32::MAX, 0);
    let mut i = 0;
    while i + 4 <= table.len() {
        let (kind, len) = (table[i], table[i + 1] as usize);
        if len < 4 || i + len > table.len() {
            break;
        }
        let s = &table[i..i + len];
        // Skip empty slots (size 0).
        if kind == 17 && word(s, 0x0C).is_some_and(|size| size != 0) {
            if let (Some(r), Some(c)) = (speed(s, 0x15, 0x54), speed(s, 0x20, 0x58)) {
                if r > 0 && c > 0 {
                    rated = rated.max(r);
                    configured = configured.min(c);
                }
            }
        }
        if kind == 127 {
            break; // end-of-table marker
        }
        // Each structure is followed by its strings, ending in a double NUL.
        let mut j = i + len;
        while j + 1 < table.len() && !(table[j] == 0 && table[j + 1] == 0) {
            j += 1;
        }
        i = j + 2;
    }
    (rated > 0).then_some((configured, rated))
}

/// The raw SMBIOS structure table, read through the documented firmware-table API.
fn smbios_table() -> Option<Vec<u8>> {
    let rsmb = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    let size = unsafe { GetSystemFirmwareTable(rsmb, 0, None) } as usize;
    if size <= 8 {
        return None;
    }
    let mut buf = vec![0u8; size];
    if unsafe { GetSystemFirmwareTable(rsmb, 0, Some(&mut buf)) } as usize != size {
        return None;
    }
    // RawSMBIOSData: 4 header bytes, a u32 length, then the table.
    let len = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    buf.get(8..8 + len).map(<[u8]>::to_vec)
}

fn memory_speed() -> Check {
    let (detail, attention) = match smbios_table().as_deref().and_then(parse_memory) {
        Some((cur, rated)) if cur < rated => {
            (format!("Memory runs at {cur} MT/s, but the modules report {rated} MT/s. Enabling XMP/EXPO in your BIOS usually fixes this."), true)
        }
        Some((cur, _)) => (
            format!("Memory runs at {cur} MT/s, the highest speed Windows sees for your modules. If your kit is sold as faster, check that XMP/EXPO is on in your BIOS."),
            false,
        ),
        None => ("Couldn't read memory speed.".into(), false),
    };
    check("Memory speed (XMP/EXPO)", detail, attention, None)
}

fn hklm_dword(key: &str, name: &str) -> Option<u32> {
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(key).ok()?.get_value(name).ok()
}

fn gpu_scheduling() -> Check {
    let state = match hklm_dword(r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode") {
        Some(2) => "On",
        Some(1) => "Off",
        _ => "Not set (Windows default)",
    };
    check(
        "Hardware-accelerated GPU scheduling",
        format!("{state}. Results vary by game and GPU, and frame generation (DLSS 3+) needs it on. Microsoft only supports changing it in Settings, and it needs a restart."),
        false,
        Some(("Graphics settings", "ms-settings:display-advancedgraphics")),
    )
}

fn memory_integrity() -> Check {
    let on = hklm_dword(r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity", "Enabled") == Some(1);
    let detail = if on {
        "On. It blocks malicious kernel drivers. Microsoft notes it can lower gaming performance slightly; that trade-off is yours to make in Windows Security. Kestrel never turns security features off."
    } else {
        "Off. Microsoft recommends turning it on for protection against malicious drivers, if your drivers are compatible."
    };
    check("Memory integrity (Core isolation)", detail.into(), false, Some(("Windows Security", "windowsdefender://coreisolation")))
}

#[cfg(test)]
mod tests {
    use super::{parse_driver_date, parse_memory};

    #[test]
    #[ignore = "reads this PC's hardware"]
    fn live_system_summary() {
        for (part, value) in super::system_summary() {
            eprintln!("{part}: {value}");
        }
    }

    #[test]
    fn driver_dates() {
        assert_eq!(parse_driver_date("11-7-2026"), Some((2026, 11)));
        assert_eq!(parse_driver_date("1-31-2024"), Some((2024, 1)));
        assert_eq!(parse_driver_date("13-1-2024"), None);
        assert_eq!(parse_driver_date(""), None);
    }

    /// A type-17 structure of 0x5C bytes with the given size/speed fields, followed by an empty string set.
    fn module(size: u16, speed: u16, configured: u16) -> Vec<u8> {
        let mut s = vec![0u8; 0x5C];
        s[0] = 17;
        s[1] = 0x5C;
        s[0x0C..0x0E].copy_from_slice(&size.to_le_bytes());
        s[0x15..0x17].copy_from_slice(&speed.to_le_bytes());
        s[0x20..0x22].copy_from_slice(&configured.to_le_bytes());
        s.extend([0, 0]);
        s
    }

    #[test]
    fn memory_from_smbios() {
        let mut table = vec![0u8, 4, 0, 0, b'x', 0, 0]; // a type-0 structure with one string
        table.extend(module(16384, 3200, 2133));
        table.extend(module(0, 0, 0)); // empty slot
        table.extend(module(16384, 3200, 2133));
        table.extend([127, 4, 0, 0, 0, 0]);
        assert_eq!(parse_memory(&table), Some((2133, 3200)));
        assert_eq!(parse_memory(&module(8192, 6000, 6000)), Some((6000, 6000)));
        assert_eq!(parse_memory(&[]), None);
    }
}
