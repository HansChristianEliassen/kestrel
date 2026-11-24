//! Hardware setup mistakes that cost FPS and that people rarely know about: the screen plugged into the
//! motherboard, one memory stick (single channel), a laptop on battery or Energy saver, and the scheduling
//! requirements of AMD's dual-CCD X3D processors. Read-only; each finding says what to do.

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

use super::health::Check;

fn check(title: &'static str, detail: String, attention: bool, link: Option<(&'static str, &'static str)>) -> Check {
    Check { title, detail, attention, link, refresh_fix: Vec::new() }
}

/// The checks that apply to this PC (laptop and X3D checks only appear where they matter).
pub fn checks(smbios: Option<&[u8]>, cpu: &str) -> Vec<Check> {
    let power = power_status();
    let laptop = power.is_some_and(|p| p.BatteryFlag != 128 && p.BatteryFlag != 255);
    let mut out = Vec::new();
    if !laptop {
        out.extend(screen_port());
        out.extend(smbios.and_then(memory_channels));
    }
    if let (true, Some(p)) = (laptop, power) {
        out.push(laptop_power(&p));
    }
    out.extend(x3d(cpu));
    out
}

/// A battery is installed (laptop, tablet or handheld): settings that cost battery life are left out of profiles.
pub fn has_battery() -> bool {
    power_status().is_some_and(|p| p.BatteryFlag != 128 && p.BatteryFlag != 255)
}

fn power_status() -> Option<SYSTEM_POWER_STATUS> {
    let mut p = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut p) }.ok().map(|_| p)
}

// ---- Screen plugged into the motherboard -------------------------------------------------------

/// Integrated graphics (part of the processor), by adapter name.
pub fn integrated(name: &str) -> bool {
    let n = name.to_lowercase();
    let intel_igpu =
        n.contains("intel") && (n.contains("uhd") || n.contains("iris") || n.contains("hd graphics") || n.contains("arc(tm) graphics"));
    let amd_igpu = (n.contains("radeon(tm) graphics") || n.contains("radeon graphics") || n.contains("vega")) && !n.contains(" rx ");
    intel_igpu || amd_igpu
}

/// Some screen runs on integrated graphics while a dedicated graphics card is installed.
pub fn wrong_port(displays_on: &[String], gpus: &[String]) -> bool {
    gpus.iter().any(|g| !integrated(g)) && displays_on.iter().any(|d| integrated(d))
}

fn screen_port() -> Option<Check> {
    let mut on = Vec::new();
    for i in 0.. {
        let mut d = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut d, 0) }.as_bool() {
            break;
        }
        if d.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP != 0 {
            on.push(String::from_utf16_lossy(&d.DeviceString).trim_end_matches('\0').to_string());
        }
    }
    let gpus: Vec<String> = super::health::gpu_drivers().into_iter().map(|g| g.0).collect();
    wrong_port(&on, &gpus).then(|| {
        check(
            "Screen plugged into the motherboard",
            "A screen is connected to the motherboard's video port, so games may run on the processor's built-in graphics instead of your graphics card. Plug the screen's cable into the graphics card (the ports lower down on the back of the PC, usually in a horizontal row), then restart the game. This often doubles the frame rate or more.".into(),
            true,
            None,
        )
    })
}

// ---- Memory channels (SMBIOS type 17) -----------------------------------------------------------

/// Installed memory modules: (device locator, bank locator), from the SMBIOS table.
pub fn modules(table: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= table.len() {
        let (kind, len) = (table[i], table[i + 1] as usize);
        if len < 4 || i + len > table.len() {
            break;
        }
        // Strings follow the structure, NUL-terminated, ending in a double NUL; numbered from 1.
        let mut j = i + len;
        let strings_start = j;
        while j + 1 < table.len() && !(table[j] == 0 && table[j + 1] == 0) {
            j += 1;
        }
        let strings: Vec<String> =
            table[strings_start..j.min(table.len())].split(|b| *b == 0).map(|s| String::from_utf8_lossy(s).trim().to_string()).collect();
        let s = &table[i..i + len];
        let size = s.get(0x0C..0x0E).map(|b| u16::from_le_bytes([b[0], b[1]]));
        if kind == 17 && size.is_some_and(|v| v != 0 && v != 0xFFFF) {
            let get =
                |at: usize| s.get(at).and_then(|&n| (n > 0).then(|| strings.get(n as usize - 1).cloned()).flatten()).unwrap_or_default();
            out.push((get(0x10), get(0x11)));
        }
        if kind == 127 {
            break;
        }
        i = j + 2;
    }
    out
}

/// The memory channel letter a module's labels name, if they name one ("P0 CHANNEL A", "ChannelB-DIMM0", "DIMM_A1").
fn channel(device: &str, bank: &str) -> Option<char> {
    let text = format!("{} {}", bank, device).to_uppercase();
    let after = |key: &str| {
        let at = text.find(key)? + key.len();
        text[at..].chars().find(|c| !matches!(c, ' ' | '_' | '-')).filter(char::is_ascii_alphabetic)
    };
    after("CHANNEL").or_else(|| after("DIMM").filter(|c| *c != 'M'))
}

/// True when the memory certainly runs in single channel: one module, or all modules on the same channel.
pub fn single_channel(mods: &[(String, String)]) -> bool {
    match mods.len() {
        0 => false,
        1 => true,
        _ => {
            let ch: Vec<Option<char>> = mods.iter().map(|(d, b)| channel(d, b)).collect();
            ch.iter().all(Option::is_some) && ch.iter().all(|c| *c == ch[0])
        }
    }
}

fn memory_channels(table: &[u8]) -> Option<Check> {
    let mods = modules(table);
    single_channel(&mods).then(|| {
        let what = if mods.len() == 1 { "Only one memory stick is installed" } else { "All memory sticks are on the same channel" };
        check(
            "Memory in single channel",
            format!("{what}, so the processor gets half the memory bandwidth. Games then lose some FPS, mostly in their slowest moments and in processor-heavy games, and a lot with built-in graphics. {}", if mods.len() == 1 { "Adding a second, matching stick fixes this." } else { "Move one stick to the other channel's slot (see the motherboard manual, usually slots 2 and 4)." }),
            true,
            None,
        )
    })
}

// ---- Laptop on battery / Energy saver -----------------------------------------------------------

fn laptop_power(p: &SYSTEM_POWER_STATUS) -> Check {
    let on_battery = p.ACLineStatus == 0;
    let saver = p.SystemStatusFlag == 1;
    let detail = match (on_battery, saver) {
        (true, true) => "Running on battery with Energy saver on. Both lower the processor and graphics speed a lot: plug in the charger and turn Energy saver off before playing.",
        (true, false) => "Running on battery. Laptops slow down the processor and graphics card on battery: plug in the charger before playing.",
        (false, true) => "Energy saver is on, which slows the PC down to save power. Turn it off in Settings > System > Power & battery before playing.",
        (false, false) => "Plugged in, and Energy saver is off.",
    };
    check("Laptop power", detail.into(), on_battery || saver, Some(("Power settings", "ms-settings:batterysaver")))
}

// ---- AMD dual-CCD X3D ------------------------------------------------------------------------------

/// Ryzen X3D processors with two core groups, where only one group has the extra cache.
pub fn dual_ccd_x3d(cpu: &str) -> bool {
    let c = cpu.to_uppercase();
    c.contains("X3D") && ["7900X3D", "7950X3D", "9900X3D", "9950X3D"].iter().any(|m| c.contains(m))
}

fn x3d(cpu: &str) -> Option<Check> {
    if !dual_ccd_x3d(cpu) {
        return None;
    }
    let game_bar = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages")
        .is_ok_and(|k| k.enum_keys().flatten().any(|p| p.starts_with("Microsoft.XboxGamingOverlay_")));
    let driver = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SYSTEM\CurrentControlSet\Services\amd3dvcache").is_ok();
    let mut missing = Vec::new();
    if !game_bar {
        missing.push("Xbox Game Bar (install it from the Microsoft Store)");
    }
    if !driver {
        missing.push("the AMD chipset driver");
    }
    let detail = if missing.is_empty() {
        "Your processor has two core groups, and only one has the extra game cache. Xbox Game Bar and the AMD chipset driver are installed, so Windows can put games on the right cores.".to_string()
    } else {
        format!("Your processor has two core groups, and only one has the extra game cache. To put games on those cores, Windows needs {}. Without it, games can run noticeably slower.", missing.join(" and "))
    };
    Some(check(
        "Ryzen X3D game cores",
        detail,
        !missing.is_empty(),
        Some(("AMD chipset drivers", "https://www.amd.com/en/support/download/drivers.html")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrated_and_wrong_port() {
        assert!(integrated("Intel(R) UHD Graphics 770") && integrated("AMD Radeon(TM) Graphics"));
        assert!(
            !integrated("NVIDIA GeForce RTX 2070 SUPER")
                && !integrated("AMD Radeon RX 7800 XT")
                && !integrated("Intel(R) Arc(TM) A770 Graphics")
        );
        let gpus = vec!["Intel(R) UHD Graphics 770".to_string(), "NVIDIA GeForce RTX 4070".to_string()];
        assert!(wrong_port(&["Intel(R) UHD Graphics 770".into()], &gpus));
        assert!(!wrong_port(&["NVIDIA GeForce RTX 4070".into()], &gpus));
        // Only integrated graphics: nothing to move.
        assert!(!wrong_port(&["Intel(R) UHD Graphics 770".into()], &gpus[..1]));
    }

    fn table(mods: &[(&str, &str)]) -> Vec<u8> {
        let mut t = Vec::new();
        for (dev, bank) in mods {
            let mut s = vec![0u8; 0x22];
            s[0] = 17;
            s[1] = 0x22;
            s[0x0C..0x0E].copy_from_slice(&8192u16.to_le_bytes());
            // A missing label is string number 0 (SMBIOS has no empty strings).
            s[0x10] = 1;
            s[0x11] = if bank.is_empty() { 0 } else { 2 };
            t.extend(s);
            t.extend(dev.as_bytes());
            t.push(0);
            if !bank.is_empty() {
                t.extend(bank.as_bytes());
                t.push(0);
            }
            t.push(0);
        }
        t.extend([127, 4, 0, 0, 0, 0]);
        t
    }

    #[test]
    fn memory_channels_only_when_certain() {
        let one = modules(&table(&[("DIMM 1", "P0 CHANNEL A")]));
        assert_eq!(one, [("DIMM 1".to_string(), "P0 CHANNEL A".to_string())]);
        assert!(single_channel(&one));
        assert!(!single_channel(&modules(&table(&[("DIMM 1", "P0 CHANNEL A"), ("DIMM 1", "P0 CHANNEL B")]))));
        assert!(single_channel(&modules(&table(&[("ChannelA-DIMM0", "BANK 0"), ("ChannelA-DIMM1", "BANK 1")]))));
        assert!(!single_channel(&modules(&table(&[("DIMM_A1", ""), ("DIMM_B1", "")]))));
        // Labels that name no channel (like this PC's "DIMM 1" twice): not certain, so no warning.
        assert!(!single_channel(&modules(&table(&[("DIMM 1", ""), ("DIMM 1", "")]))));
    }

    #[test]
    #[ignore = "reads this PC's hardware"]
    fn live_hardware() {
        let table = super::super::health::smbios_table();
        eprintln!("modules: {:?}", table.as_deref().map(modules));
        for c in checks(table.as_deref(), "") {
            eprintln!("{} (attention {}): {}", c.title, c.attention, c.detail);
        }
        eprintln!("done");
    }

    #[test]
    fn x3d_models() {
        assert!(dual_ccd_x3d("AMD Ryzen 9 7950X3D 16-Core Processor") && dual_ccd_x3d("AMD Ryzen 9 9900X3D"));
        assert!(!dual_ccd_x3d("AMD Ryzen 7 7800X3D 8-Core Processor") && !dual_ccd_x3d("AMD Ryzen 5 7500F 6-Core Processor"));
    }
}
