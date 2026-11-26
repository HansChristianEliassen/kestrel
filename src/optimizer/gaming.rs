//! Presets and per-game settings.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use super::nvidia;
use super::tweaks::{get_kv, read_sz, set_kv, write_sz};
use super::Tweak;
use crate::utils::{backup, log};

#[derive(Clone, Copy, PartialEq)]
pub enum Preset {
    Balanced,
    Performance,
}

impl Preset {
    /// Cosmetic, preference and weak-evidence settings stay out of presets; pick them under Custom.
    pub fn includes(self, t: Tweak) -> bool {
        match self {
            Preset::Balanced => matches!(t, Tweak::GameMode | Tweak::GameDvrOff | Tweak::WindowedGames),
            // Power mode rather than the High performance plan: it's Windows 11's supported switch, and it exists on
            // Modern Standby laptops that don't offer the old plans.
            Preset::Performance => Preset::Balanced.includes(t) || t == Tweak::PowerMode,
        }
    }
}

// Per-app values Windows itself uses: the exe's Properties > Compatibility tab (Layers)
// and Settings > System > Display > Graphics (UserGpuPreferences).
const LAYERS: &str = r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers";
const FSO_FLAG: &str = "DISABLEDXMAXIMIZEDWINDOWEDMODE";
const GPU_PREFS: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";

fn has_flag(v: &str) -> bool {
    v.split_whitespace().any(|f| f == FSO_FLAG)
}

fn add_flag(existing: Option<&str>) -> String {
    match existing {
        Some(v) if has_flag(v) => v.to_string(),
        Some(v) if v.trim_start().starts_with('~') => format!("{} {FSO_FLAG}", v.trim_end()),
        Some(v) if !v.trim().is_empty() => format!("~ {} {FSO_FLAG}", v.trim()),
        _ => format!("~ {FSO_FLAG}"),
    }
}

/// Remaining flags, or None when nothing but the "~" marker is left.
fn remove_flag(existing: &str) -> Option<String> {
    let rest: Vec<&str> = existing.split_whitespace().filter(|f| *f != FSO_FLAG).collect();
    (rest.iter().any(|f| *f != "~")).then(|| rest.join(" "))
}

fn check_exe(exe: &str) -> Result<&str, String> {
    let exe = exe.trim().trim_matches('"');
    let p = Path::new(exe);
    if !p.is_absolute() || !p.is_file() || !exe.to_ascii_lowercase().ends_with(".exe") {
        return Err("Choose the game's .exe file.".into());
    }
    Ok(exe)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GameSetting {
    HighPerfGpu,
    FsoOff,
    WindowedOptOff,
    /// In the game's NVIDIA driver profile.
    NvMaxPerf,
    NvLowLatency,
}

impl GameSetting {
    pub const ALL: [GameSetting; 5] =
        [GameSetting::HighPerfGpu, GameSetting::FsoOff, GameSetting::WindowedOptOff, GameSetting::NvMaxPerf, GameSetting::NvLowLatency];
    /// Per-game NVIDIA settings from 0.26. Replaced by the global NVIDIA gaming profile; a game page shows them only
    /// while they're on, so they can still be turned off.
    pub const NVIDIA: [GameSetting; 2] = [GameSetting::NvMaxPerf, GameSetting::NvLowLatency];

    /// The NVIDIA driver settings behind it, with the values it sets (empty for Windows' own settings).
    fn nv(self) -> &'static [(u32, u32)] {
        match self {
            GameSetting::NvMaxPerf => &[(nvidia::POWER_MODE, 1)],
            GameSetting::NvLowLatency => &[(nvidia::PRERENDER_LIMIT, 1)],
            _ => &[],
        }
    }

    pub fn is_nvidia(self) -> bool {
        !self.nv().is_empty()
    }

    pub fn label(self) -> &'static str {
        match self {
            GameSetting::HighPerfGpu => "High-performance GPU",
            GameSetting::FsoOff => "Fullscreen optimizations off",
            GameSetting::WindowedOptOff => "Windowed-game optimizations off",
            GameSetting::NvMaxPerf => "NVIDIA: Prefer maximum performance",
            GameSetting::NvLowLatency => "NVIDIA: Low latency mode",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            GameSetting::HighPerfGpu => {
                "On laptops with two graphics chips, always runs the game on the dedicated GPU. No effect on single-GPU PCs."
            }
            GameSetting::FsoOff => {
                "Can smooth out some older DirectX games in fullscreen. Alt-Tab and overlays may be slower for this game, and Auto HDR or variable refresh rate may stop working for it. Test it per game."
            }
            GameSetting::WindowedOptOff => {
                "Leaves just this game out of Windows 11's windowed-game optimizations, for the rare game that glitches with them."
            }
            GameSetting::NvMaxPerf => {
                "Keeps the graphics card at full speed while this game runs, so light or processor-heavy games don't stutter when it clocks down. A small effect; uses more power while you play. NVIDIA Control Panel > Program settings > Power management mode."
            }
            GameSetting::NvLowLatency => {
                "Low Latency Mode = On in the NVIDIA Control Panel: the processor prepares at most one frame ahead, which cuts input lag when the graphics card is the limit. Works in DirectX 11 and older games; DirectX 12 and Vulkan games ignore it. If the game has NVIDIA Reflex, use Reflex in its settings instead."
            }
        }
    }

    pub fn unsupported(self, build: u32) -> Option<&'static str> {
        match self {
            GameSetting::WindowedOptOff if build < 22000 => Some("Windows 11 only."),
            s if s.is_nvidia() && !nvidia::available() => Some("Needs an NVIDIA graphics card and driver."),
            _ => None,
        }
    }

    fn backup_key(self, exe: &str) -> String {
        let prefix = match self {
            GameSetting::HighPerfGpu => "gpu:",
            GameSetting::FsoOff => "fso:",
            GameSetting::WindowedOptOff => "swap:",
            GameSetting::NvMaxPerf => "nvpower:",
            GameSetting::NvLowLatency => "nvlatency:",
        };
        format!("{prefix}{exe}")
    }

    /// Name of the DirectX preference entry, for the settings stored there.
    fn dx_entry(self) -> &'static str {
        if self == GameSetting::HighPerfGpu {
            "GpuPreference"
        } else {
            "SwapEffectUpgradeEnable"
        }
    }

    fn target(self) -> &'static str {
        match self {
            GameSetting::HighPerfGpu => "2",
            GameSetting::FsoOff => "1",
            GameSetting::WindowedOptOff => "0",
            GameSetting::NvMaxPerf | GameSetting::NvLowLatency => "1",
        }
    }

    /// Current raw value: the DirectX preference entry, or "1" when the compatibility flag is set.
    fn read(self, exe: &str) -> Option<String> {
        if self.is_nvidia() {
            // The game's own value per driver setting, "-" where it has none; None when it has none at all.
            let values: Vec<String> = self
                .nv()
                .iter()
                .map(|(id, _)| nvidia::app_setting(exe, *id).ok().flatten().map_or("-".into(), |v| v.to_string()))
                .collect();
            return values.iter().any(|v| v != "-").then(|| values.join(","));
        }
        match self {
            GameSetting::FsoOff => read_sz(LAYERS, exe).filter(|v| has_flag(v)).map(|_| "1".into()),
            _ => read_sz(GPU_PREFS, exe).and_then(|v| get_kv(&v, self.dx_entry()).map(String::from)),
        }
    }

    /// Writes a raw value (`None` = Windows default), leaving the program's other settings alone.
    fn write(self, exe: &str, value: Option<&str>) -> Result<(), String> {
        if self.is_nvidia() {
            let parts: Vec<&str> = value.map_or_else(Vec::new, |v| v.split(',').collect());
            for (i, (id, _)) in self.nv().iter().enumerate() {
                let v = match parts.get(i) {
                    None | Some(&"-") => None,
                    Some(p) => Some(p.parse::<u32>().map_err(|_| "Saved value is invalid.".to_string())?),
                };
                nvidia::set_app_setting(exe, *id, v)?;
            }
            return Ok(());
        }
        match self {
            GameSetting::FsoOff => {
                let current = read_sz(LAYERS, exe);
                let new = match value {
                    Some(_) => Some(add_flag(current.as_deref())),
                    None => current.as_deref().and_then(remove_flag),
                };
                write_sz(LAYERS, exe, new.as_deref())
            }
            _ => {
                let rest = set_kv(&read_sz(GPU_PREFS, exe).unwrap_or_default(), self.dx_entry(), value);
                write_sz(GPU_PREFS, exe, (!rest.is_empty()).then_some(rest.as_str()))
            }
        }
    }

    /// (on, changed by Kestrel), using an already-loaded backup map.
    pub fn status(self, exe: &str, saved: &BTreeMap<String, String>) -> (bool, bool) {
        (self.read(exe).as_deref() == Some(self.target()), saved.contains_key(&self.backup_key(exe)))
    }

    pub fn apply(self, exe: &str) -> Result<(), String> {
        let exe = check_exe(exe)?;
        backup::save_original(&self.backup_key(exe), || Ok(self.read(exe).unwrap_or_else(|| "-".into())))?;
        self.write(exe, Some(self.target()))?;
        log(&format!("Per-game: turned on '{}' for {exe}", self.label()));
        Ok(())
    }

    /// Back to the saved original if Kestrel made the change, otherwise back to the Windows default.
    pub fn undo(self, exe: &str) -> Result<(), String> {
        let key = self.backup_key(exe);
        match backup::get(&key) {
            Some(original) => {
                self.write(exe, (original != "-").then_some(original.as_str()))?;
                backup::remove(&key)?;
            }
            None => self.write(exe, None)?,
        }
        log(&format!("Per-game: undid '{}' for {exe}", self.label()));
        Ok(())
    }
}

/// Every program with at least one per-game setting on, from any source (Kestrel, Windows, other apps).
pub fn configured() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for key in [LAYERS, GPU_PREFS] {
        let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(key) else { continue };
        for (exe, _) in k.enum_values().flatten() {
            if exe.to_ascii_lowercase().ends_with(".exe")
                && GameSetting::ALL.iter().any(|s| !s.is_nvidia() && s.read(&exe).as_deref() == Some(s.target()))
            {
                out.insert(exe);
            }
        }
    }
    out
}

/// After a driver install: turns Kestrel's NVIDIA settings back on where the install reset them (a clean
/// installation clears every driver profile). Only settings Kestrel changed (they have a saved original), so a
/// value the user changed since is respected. One line per setting put back.
pub fn reapply_nvidia() -> Vec<String> {
    let mut lines = Vec::new();
    for s in GameSetting::NVIDIA {
        for (exe, _) in backup::with_prefix(&s.backup_key("")) {
            if s.read(&exe).as_deref() == Some(s.target()) {
                continue;
            }
            lines.push(match s.write(&exe, Some(s.target())) {
                Ok(()) => {
                    format!("✔ {} for {}", s.label(), Path::new(&exe).file_name().map_or(exe.clone(), |f| f.to_string_lossy().into_owned()))
                }
                Err(e) => format!("✖ {} for {exe}: {e}", s.label()),
            });
        }
    }
    let cache = Tweak::NvShaderCache;
    if backup::get(cache.key()).is_some() && !cache.is_applied() {
        lines.push(match cache.apply() {
            Ok(()) => format!("✔ {}", cache.info().title),
            Err(e) => format!("✖ {}: {e}", cache.info().title),
        });
    }
    lines
}

/// Puts back the original per-game values Kestrel changed. One line per setting.
pub fn restore_all_games() -> Vec<String> {
    let mut lines = Vec::new();
    for s in GameSetting::ALL {
        let prefix = s.backup_key("");
        for (exe, _) in backup::with_prefix(&prefix) {
            lines.push(match s.undo(&exe) {
                Ok(()) => format!("✔ Restored {}: {exe}", s.label()),
                Err(e) => format!("✖ {} for {exe}: {e}", s.label()),
            });
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_targets_match_their_settings() {
        for s in GameSetting::NVIDIA {
            let joined: Vec<String> = s.nv().iter().map(|(_, v)| v.to_string()).collect();
            assert_eq!(s.target(), joined.join(","), "{s:?}");
        }
    }

    #[test]
    fn flags_round_trip() {
        assert_eq!(add_flag(None), "~ DISABLEDXMAXIMIZEDWINDOWEDMODE");
        assert_eq!(add_flag(Some("~ RUNASADMIN")), "~ RUNASADMIN DISABLEDXMAXIMIZEDWINDOWEDMODE");
        assert_eq!(remove_flag("~ RUNASADMIN DISABLEDXMAXIMIZEDWINDOWEDMODE"), Some("~ RUNASADMIN".into()));
        assert_eq!(remove_flag("~ DISABLEDXMAXIMIZEDWINDOWEDMODE"), None);
    }
}
