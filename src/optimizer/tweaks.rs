//! Reversible settings. Each tweak only changes documented per-user settings
//! (the same ones the Settings app toggles), powercfg, or SystemParametersInfo.
//! The original value is saved to backup.txt before the first change.

use std::ffi::c_void;

use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, ANIMATIONINFO, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETANIMATION, SPI_GETMOUSE, SPI_SETANIMATION,
    SPI_SETMOUSE, SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::system::{broadcast_setting_change, run_hidden};
use crate::utils::{backup, log};

const HIGH_PERFORMANCE: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
const SUB_USB: &str = "2a737441-1930-4402-8d77-b2bebba308a3";
const USB_SELECTIVE_SUSPEND: &str = "48e6b7a6-50f5-4782-a5d4-53bb8f07e226";
const DX_PREFS: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
const DX_GLOBAL: &str = "DirectXUserGlobalSettings";

/// Declaration order = `ALL` order = display order (grouped by category).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tweak {
    GameMode,
    GameDvrOff,
    NoBackgroundApps,
    NoTransparency,
    NoAnimations,
    WindowedGames,
    HighPerfPower,
    UsbSuspendOff,
    NoMouseAccel,
}

pub struct Info {
    pub title: &'static str,
    pub purpose: &'static str,
    pub benefit: &'static str,
    pub drawback: &'static str,
}

impl Tweak {
    pub const ALL: [Tweak; 9] = [
        Tweak::GameMode,
        Tweak::GameDvrOff,
        Tweak::NoBackgroundApps,
        Tweak::NoTransparency,
        Tweak::NoAnimations,
        Tweak::WindowedGames,
        Tweak::HighPerfPower,
        Tweak::UsbSuspendOff,
        Tweak::NoMouseAccel,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Why this tweak has no effect on the given Windows build, if it doesn't.
    pub fn unsupported(self, build: u32) -> Option<&'static str> {
        const WIN11: u32 = 22000;
        match self {
            Tweak::WindowedGames if build < WIN11 => Some("Windows 11 only."),
            // Kept only so Revert can undo changes made by older versions.
            Tweak::NoBackgroundApps => Some(
                "No longer offered: it only worked on Windows 10, which Microsoft stopped supporting. On Windows 11, set background access per app in Settings > Apps > Installed apps.",
            ),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Tweak::GameMode => "game_mode",
            Tweak::GameDvrOff => "background_recording",
            Tweak::HighPerfPower => "power_plan",
            Tweak::NoTransparency => "transparency",
            Tweak::NoAnimations => "animations",
            Tweak::NoBackgroundApps => "background_apps",
            Tweak::NoMouseAccel => "mouse_accel",
            Tweak::WindowedGames => "windowed_games",
            Tweak::UsbSuspendOff => "usb_suspend",
        }
    }

    pub fn info(self) -> Info {
        let (title, purpose, benefit, drawback) = match self {
            Tweak::GameMode => (
                "Enable Game Mode",
                "Windows gives the game you're playing priority and holds back Windows Update installs and restart notifications while it runs.",
                "Steadier frame times when other apps are busy.",
                "Little difference on powerful PCs.",
            ),
            Tweak::GameDvrOff => (
                "Turn off background game recording",
                "Stops Xbox Game Bar from constantly recording the last few minutes of gameplay (\"Record what happened\").",
                "Frees GPU encoder time and disk writes while you play.",
                "You can't save a clip of something that just happened. Manual recording still works.",
            ),
            Tweak::HighPerfPower => (
                "High performance power plan",
                "Switches to Windows' built-in High performance plan, which keeps the CPU ready at higher clock speeds.",
                "Faster clock ramp-up and fewer power-saving dips in demanding games.",
                "More power draw, heat and fan noise. Drains laptop batteries quickly, so use it when plugged in. Some laptops don't offer this plan.",
            ),
            Tweak::NoTransparency => (
                "Turn off transparency effects",
                "Makes the taskbar, Start menu and app backgrounds solid instead of see-through.",
                "Less desktop compositing work, mostly noticeable on integrated graphics.",
                "Mostly cosmetic: Windows looks a little flatter, and dedicated graphics cards see no measurable gain.",
            ),
            Tweak::NoAnimations => (
                "Turn off window animations",
                "Disables the animation when windows are minimised and maximised.",
                "Alt-tabbing and window switching feel snappier.",
                "Cosmetic only.",
            ),
            Tweak::NoBackgroundApps => (
                "Limit background Store apps",
                "Stops Microsoft Store apps from running in the background (Windows 10).",
                "Less background CPU, memory and network use.",
                "Apps like Mail, Calendar or Phone Link may not show notifications until you open them.",
            ),
            Tweak::NoMouseAccel => (
                "Turn off \"Enhance pointer precision\"",
                "Disables Windows mouse acceleration so the cursor moves exactly as far as your mouse does.",
                "Consistent aim in games that use the Windows cursor.",
                "Personal preference: the desktop cursor may feel slower. Most modern games use raw input and are unaffected.",
            ),
            Tweak::WindowedGames => (
                "Optimizations for windowed games",
                "Windows 11 setting that lets DirectX 10/11 games in windowed or borderless mode present frames the same efficient way as fullscreen games. Kestrel sets the same per-user value as Settings > Graphics.",
                "Lower input latency in borderless and windowed games, plus G-SYNC/FreeSync support for them.",
                "Windows 11 only. Takes effect the next time each game starts. Rarely, an older game shows glitches; you can exclude a single game on the Per-game tab.",
            ),
            Tweak::UsbSuspendOff => (
                "Turn off USB selective suspend",
                "Stops Windows from putting idle USB devices into low-power mode while plugged in, on the active power plan.",
                "Prevents wake-up delays and dropouts on controllers, headsets and other USB devices.",
                "Slightly higher idle power use. Applies to the current power plan only; a mouse you're using never sleeps anyway.",
            ),
        };
        Info { title, purpose, benefit, drawback }
    }

    /// Per-user registry values this tweak sets: (key, value, target).
    fn registry(self) -> &'static [(&'static str, &'static str, u32)] {
        match self {
            Tweak::GameMode => &[(r"Software\Microsoft\GameBar", "AutoGameModeEnabled", 1)],
            // "Record what happened" only; Win+Alt+R and screenshots keep working.
            Tweak::GameDvrOff => &[(r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "HistoricalCaptureEnabled", 0)],
            Tweak::NoTransparency => &[(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "EnableTransparency", 0)],
            Tweak::NoBackgroundApps => &[
                (r"Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications", "GlobalUserDisabled", 1),
                (r"Software\Microsoft\Windows\CurrentVersion\Search", "BackgroundAppGlobalToggle", 0),
            ],
            _ => &[],
        }
    }

    /// Current state, serialized so it can be written back by `write`.
    pub(crate) fn snapshot(self) -> Result<String, String> {
        Ok(match self {
            Tweak::HighPerfPower => active_power_scheme()?,
            Tweak::NoAnimations => get_min_animate()?.to_string(),
            Tweak::NoMouseAccel => get_mouse()?.map(|v| v.to_string()).join(","),
            Tweak::WindowedGames => read_sz(DX_PREFS, DX_GLOBAL).unwrap_or_else(|| "-".into()),
            Tweak::UsbSuspendOff => usb_suspend()?.to_string(),
            _ => self
                .registry()
                .iter()
                .map(|(k, v, _)| read_dword(k, v).map_or("-".to_string(), |d| d.to_string()))
                .collect::<Vec<_>>()
                .join(","),
        })
    }

    /// Writes the optimized state (`None`) or a saved snapshot.
    pub(crate) fn write(self, saved: Option<&str>) -> Result<(), String> {
        match self {
            Tweak::HighPerfPower => {
                let guid = match saved {
                    Some(g) => g.to_string(),
                    None => {
                        if !run_hidden("powercfg", &["/list"])?.contains(HIGH_PERFORMANCE) {
                            return Err("This PC doesn't offer the High performance power plan.".into());
                        }
                        HIGH_PERFORMANCE.to_string()
                    }
                };
                if !is_guid(&guid) {
                    return Err("Saved power plan is invalid.".into());
                }
                run_hidden("powercfg", &["/setactive", &guid]).map(|_| ())
            }
            Tweak::NoAnimations => set_min_animate(saved.map_or(Ok(0), parse_i32)?),
            Tweak::NoMouseAccel => {
                let v = match saved {
                    Some(s) => {
                        let p: Vec<i32> = s.split(',').map(parse_i32).collect::<Result<_, _>>()?;
                        <[i32; 3]>::try_from(p).map_err(|_| "Saved mouse setting is invalid.".to_string())?
                    }
                    None => [0, 0, 0],
                };
                set_mouse(v)
            }
            Tweak::WindowedGames => match saved {
                Some("-") => write_sz(DX_PREFS, DX_GLOBAL, None),
                Some(s) => write_sz(DX_PREFS, DX_GLOBAL, Some(s)),
                None => {
                    let current = read_sz(DX_PREFS, DX_GLOBAL).unwrap_or_default();
                    write_sz(DX_PREFS, DX_GLOBAL, Some(&set_kv(&current, "SwapEffectUpgradeEnable", Some("1"))))
                }
            },
            // shortcut: writes to whichever plan is active; restore-all runs in reverse so the plan switch is undone last.
            Tweak::UsbSuspendOff => {
                let value = saved.map_or(Ok(0), |s| s.parse::<u32>().map_err(|_| "Saved value is invalid.".to_string()))?;
                run_hidden("powercfg", &["/setacvalueindex", "SCHEME_CURRENT", SUB_USB, USB_SELECTIVE_SUSPEND, &value.to_string()])?;
                run_hidden("powercfg", &["/setactive", "SCHEME_CURRENT"]).map(|_| ())
            }
            _ => {
                write_registry(self.registry(), saved)?;
                if self == Tweak::NoTransparency {
                    broadcast_setting_change("ImmersiveColorSet");
                }
                Ok(())
            }
        }
    }

    pub fn is_applied(self) -> bool {
        match self {
            // Missing value = Windows' default, which is off.
            Tweak::GameDvrOff => self.registry().iter().all(|(k, v, t)| read_dword(k, v).unwrap_or(0) == *t),
            Tweak::HighPerfPower => active_power_scheme().is_ok_and(|g| g == HIGH_PERFORMANCE),
            Tweak::NoAnimations => get_min_animate().is_ok_and(|v| v == 0),
            Tweak::NoMouseAccel => get_mouse().is_ok_and(|m| m[2] == 0),
            Tweak::WindowedGames => read_sz(DX_PREFS, DX_GLOBAL).is_some_and(|s| get_kv(&s, "SwapEffectUpgradeEnable") == Some("1")),
            Tweak::UsbSuspendOff => usb_suspend().is_ok_and(|v| v == 0),
            _ => self.registry().iter().all(|(k, v, t)| read_dword(k, v) == Some(*t)),
        }
    }

    pub fn has_backup(self) -> bool {
        backup::has(self.key()) || (self == Tweak::GameDvrOff && backup::has(LEGACY_DVR_KEY))
    }

    pub fn apply(self) -> Result<(), String> {
        backup::save_original(self.key(), || self.snapshot())?;
        self.write(None)?;
        log(&format!("Optimizer: applied '{}'", self.info().title));
        Ok(())
    }

    /// Restores the saved original. Returns false when there was nothing to restore.
    pub fn restore(self) -> Result<bool, String> {
        // Versions before 0.7 turned off all capture; put those values back too.
        if self == Tweak::GameDvrOff {
            if let Some(saved) = backup::get(LEGACY_DVR_KEY) {
                write_registry(LEGACY_DVR, Some(&saved))?;
                backup::remove(LEGACY_DVR_KEY)?;
                if !backup::has(self.key()) {
                    log("Optimizer: restored original Game DVR settings saved by an older version");
                    return Ok(true);
                }
            }
        }
        let Some(saved) = backup::get(self.key()) else { return Ok(false) };
        self.write(Some(&saved))?;
        backup::remove(self.key())?;
        log(&format!("Optimizer: restored original setting for '{}'", self.info().title));
        Ok(true)
    }
}

const LEGACY_DVR_KEY: &str = "game_dvr";
const LEGACY_DVR: &[(&str, &str, u32)] =
    &[(r"System\GameConfigStore", "GameDVR_Enabled", 0), (r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "AppCaptureEnabled", 0)];

/// Writes the targets (`None`) or a saved "a,b" snapshot, where "-" means the value didn't exist.
fn write_registry(entries: &[(&str, &str, u32)], saved: Option<&str>) -> Result<(), String> {
    let saved: Vec<&str> = saved.map(|s| s.split(',').collect()).unwrap_or_default();
    for (i, (k, v, target)) in entries.iter().enumerate() {
        let value = match saved.get(i) {
            None if saved.is_empty() => Some(*target),
            None | Some(&"-") => None,
            Some(s) => Some(s.parse().map_err(|_| "Saved value is invalid.".to_string())?),
        };
        write_dword(k, v, value)?;
    }
    Ok(())
}

fn parse_i32(s: &str) -> Result<i32, String> {
    s.trim().parse().map_err(|_| "Saved value is invalid.".to_string())
}

fn is_guid(s: &str) -> bool {
    s.len() == 36 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn read_dword(key: &str, name: &str) -> Option<u32> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(key).ok()?.get_value(name).ok()
}

fn write_dword(key: &str, name: &str, value: Option<u32>) -> Result<(), String> {
    let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(key).map_err(|e| e.to_string())?;
    match value {
        Some(v) => k.set_value(name, &v),
        None => match k.delete_value(name) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        },
    }
    .map_err(|e| e.to_string())
}

pub(crate) fn read_sz(key: &str, name: &str) -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(key).ok()?.get_value(name).ok()
}

pub(crate) fn write_sz(key: &str, name: &str, value: Option<&str>) -> Result<(), String> {
    let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(key).map_err(|e| e.to_string())?;
    match value {
        Some(v) => k.set_value(name, &v),
        None => match k.delete_value(name) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        },
    }
    .map_err(|e| e.to_string())
}

/// Value of `key` in a DirectX preference string like "GpuPreference=2;SwapEffectUpgradeEnable=1;".
pub(crate) fn get_kv<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    s.split(';').find_map(|p| p.split_once('=').filter(|(k, _)| k.trim() == key).map(|(_, v)| v.trim()))
}

/// Sets (or with `None` removes) one entry, keeping every other entry as it was.
pub(crate) fn set_kv(s: &str, key: &str, value: Option<&str>) -> String {
    let mut out: String =
        s.split(';').filter(|p| !p.trim().is_empty() && p.split('=').next().map(str::trim) != Some(key)).map(|p| format!("{p};")).collect();
    if let Some(v) = value {
        out.push_str(&format!("{key}={v};"));
    }
    out
}

/// USB selective suspend AC value on the active plan (1 = enabled, 0 = disabled).
fn usb_suspend() -> Result<u32, String> {
    let out = run_hidden("powercfg", &["/q", "SCHEME_CURRENT", SUB_USB, USB_SELECTIVE_SUSPEND])?;
    // The labels are localized; the first hex value is the AC setting.
    out.split_whitespace()
        .find_map(|t| t.strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok()))
        .ok_or_else(|| "This power plan has no USB selective suspend setting.".into())
}

fn active_power_scheme() -> Result<String, String> {
    let out = run_hidden("powercfg", &["/getactivescheme"])?;
    out.split_whitespace().find(|t| is_guid(t)).map(str::to_lowercase).ok_or_else(|| "Could not read the active power plan.".into())
}

fn spi<T>(action: SYSTEM_PARAMETERS_INFO_ACTION, ui_param: u32, value: &mut T, persist: bool) -> Result<(), String> {
    let flags = if persist { SPIF_UPDATEINIFILE | SPIF_SENDCHANGE } else { SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0) };
    unsafe { SystemParametersInfoW(action, ui_param, Some(value as *mut T as *mut c_void), flags) }.map_err(|e| e.message().to_string())
}

fn get_min_animate() -> Result<i32, String> {
    let mut a = ANIMATIONINFO { cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32, iMinAnimate: 0 };
    spi(SPI_GETANIMATION, a.cbSize, &mut a, false)?;
    Ok(a.iMinAnimate)
}

fn set_min_animate(v: i32) -> Result<(), String> {
    let mut a = ANIMATIONINFO { cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32, iMinAnimate: v };
    spi(SPI_SETANIMATION, a.cbSize, &mut a, true)
}

/// [threshold1, threshold2, acceleration]; acceleration 0 = "Enhance pointer precision" off.
fn get_mouse() -> Result<[i32; 3], String> {
    let mut m = [0i32; 3];
    spi(SPI_GETMOUSE, 0, &mut m, false)?;
    Ok(m)
}

fn set_mouse(mut m: [i32; 3]) -> Result<(), String> {
    spi(SPI_SETMOUSE, 0, &mut m, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_strings() {
        assert_eq!(set_kv("", "SwapEffectUpgradeEnable", Some("1")), "SwapEffectUpgradeEnable=1;");
        assert_eq!(
            set_kv("VRROptimizeEnable=0;SwapEffectUpgradeEnable=0;", "SwapEffectUpgradeEnable", Some("1")),
            "VRROptimizeEnable=0;SwapEffectUpgradeEnable=1;"
        );
        assert_eq!(set_kv("GpuPreference=2;", "GpuPreference", None), "");
        assert_eq!(get_kv("A=1;GpuPreference=2;", "GpuPreference"), Some("2"));
        assert_eq!(get_kv("A=1;", "GpuPreference"), None);
    }
}
