//! In-game graphics presets (Quality, Balanced, Performance), written to the game's own settings file where its
//! values are documented or self-describing, and explained step by step where they aren't.
//! - Unreal Engine games: `[ScalabilityGroups]` in GameUserSettings.ini, levels 0-4 as documented by Epic.
//! - Red Dead Redemption 2: system.xml, only the settings whose values name themselves (Low/Medium/High/Ultra,
//!   true/false) plus Water Physics, whose number is the menu slider's position.
//! - Overwatch and Call of Duty: exact menu steps (their files' values aren't documented).
//!
//! Presets only ever lower a setting that is above its target, never raise one. The game must be closed (games
//! rewrite these files when they exit), the original file is saved first, and Undo puts it back.

use std::path::{Path, PathBuf};

use crate::system::env_path;
use crate::utils::{data_dir, log, write_atomic};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Preset {
    Quality,
    Balanced,
    Performance,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Quality, Preset::Balanced, Preset::Performance];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Quality => "Quality",
            Preset::Balanced => "Balanced",
            Preset::Performance => "Performance",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            Preset::Quality => "Lowers only the most expensive effects. Looks almost the same, runs a bit faster.",
            Preset::Balanced => "Lowers the settings that cost a lot of FPS for little visible difference. The best trade for most people.",
            Preset::Performance => "As many FPS as possible, for competitive play or slower PCs. Plainer graphics.",
        }
    }
}

/// Where a game's settings come from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Unreal(PathBuf),
    Rdr2(PathBuf),
    Guide(&'static str),
}

/// One setting change: (name as shown in the game, current value, new value).
pub type Change = (String, String, String);

/// What Kestrel can do for this game (exe path and name), if anything.
pub fn find(name: &str, exe: &str) -> Option<Source> {
    let file = Path::new(exe).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
    match file.as_str() {
        "rdr2.exe" => {
            let p = env_path("USERPROFILE").join(r"Documents\Rockstar Games\Red Dead Redemption 2\Settings\system.xml");
            return p.is_file().then_some(Source::Rdr2(p));
        }
        "overwatch.exe" => return Some(Source::Guide("overwatch")),
        "cod.exe" | "cod24.exe" | "cod25.exe" | "blackops7.exe" => return Some(Source::Guide("cod")),
        _ => {}
    }
    unreal_file(name, exe).map(Source::Unreal)
}

// ---- Unreal Engine ------------------------------------------------------------------------------

/// Letters and digits only, lower case, for matching folder names to games.
fn norm(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_lowercase()
}

/// The Unreal project name for an exe: the folder above "Binaries" (…\FortniteGame\Binaries\Win64\…), or the exe
/// name before "-Win64".
pub fn unreal_project(exe: &str) -> Option<String> {
    let p = Path::new(exe);
    let by_folder =
        p.ancestors().find(|a| a.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Binaries"))).and_then(|b| b.parent()?.file_name());
    by_folder
        .map(|n| n.to_string_lossy().into_owned())
        .or_else(|| p.file_stem()?.to_string_lossy().split("-Win64").next().filter(|s| !s.is_empty()).map(String::from))
}

/// The game's GameUserSettings.ini (with a [ScalabilityGroups] section) under %LOCALAPPDATA%.
fn unreal_file(name: &str, exe: &str) -> Option<PathBuf> {
    let project = norm(&unreal_project(exe)?);
    let game = norm(name);
    let local = env_path("LOCALAPPDATA");
    let folder = std::fs::read_dir(&local).ok()?.flatten().find(|e| {
        let n = norm(&e.file_name().to_string_lossy());
        n.len() >= 4 && (n == project || n == game || (project.len() >= 6 && n.starts_with(&project[..6]) && game.starts_with(&n[..4])))
    })?;
    ["Windows", "WindowsNoEditor", "WindowsClient"]
        .iter()
        .map(|w| folder.path().join(format!(r"Saved\Config\{w}\GameUserSettings.ini")))
        .find(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains("[ScalabilityGroups]")))
}

/// (key, name in menus, Quality, Balanced, Performance targets). Textures are left alone: they cost memory, not FPS.
const UNREAL: &[(&str, &str, u8, u8, u8)] = &[
    ("sg.ShadowQuality", "Shadows", 2, 1, 0),
    ("sg.GlobalIlluminationQuality", "Global illumination", 2, 1, 0),
    ("sg.ReflectionQuality", "Reflections", 2, 1, 0),
    ("sg.EffectsQuality", "Effects", 2, 1, 0),
    ("sg.PostProcessQuality", "Post-processing", 2, 1, 0),
    ("sg.FoliageQuality", "Foliage", 2, 1, 0),
    ("sg.ShadingQuality", "Shading", 3, 2, 1),
    ("sg.ViewDistanceQuality", "View distance", 3, 2, 1),
    ("sg.AntiAliasingQuality", "Anti-aliasing", 3, 2, 1),
];

fn level(n: u8) -> &'static str {
    ["Low", "Medium", "High", "Epic", "Cinematic"].get(n as usize).copied().unwrap_or("Custom")
}

/// Changes for an Unreal GameUserSettings.ini, and the edited text.
pub fn unreal_edit(text: &str, preset: Preset) -> (Vec<Change>, String) {
    let mut changes = Vec::new();
    let mut in_groups = false;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        if body.starts_with('[') {
            in_groups = body == "[ScalabilityGroups]";
        }
        let edited = in_groups.then(|| body.split_once('=')).flatten().and_then(|(key, val)| {
            let (_, name, q, b, p) = UNREAL.iter().find(|u| u.0 == key.trim())?;
            let cur: u8 = val.trim().parse().ok()?;
            let target = match preset {
                Preset::Quality => *q,
                Preset::Balanced => *b,
                Preset::Performance => *p,
            };
            (cur > target).then(|| {
                changes.push((name.to_string(), level(cur).to_string(), level(target).to_string()));
                format!("{}={target}{}", key, &line[body.len()..])
            })
        });
        out.push_str(edited.as_deref().unwrap_or(line));
    }
    (changes, out)
}

// ---- Red Dead Redemption 2 ----------------------------------------------------------------------

const LEVELS: [&str; 4] = ["kSettingLevel_Low", "kSettingLevel_Medium", "kSettingLevel_High", "kSettingLevel_Ultra"];

/// (element, name in the game's menu, Quality, Balanced, Performance as indexes into LEVELS).
const RDR2_LEVELS: &[(&str, &str, usize, usize, usize)] = &[
    ("volumetricsRaymarchQuality", "Volumetric Raymarch Resolution", 2, 1, 0),
    ("volumetricsLightingQuality", "Volumetric Lighting Quality", 2, 1, 0),
    ("reflectionQuality", "Reflection Quality", 2, 1, 0),
    ("shadowSoftShadows", "Soft Shadows", 2, 1, 0),
    ("treeQuality", "Tree Quality", 2, 1, 0),
    ("particleQuality", "Particle Quality", 2, 1, 0),
    ("ssao", "Screen Space Ambient Occlusion", 2, 1, 0),
    ("tessellation", "Tessellation Quality", 2, 1, 0),
    ("shadowQuality", "Shadow Quality", 3, 2, 1),
    ("POMQuality", "Parallax Occlusion Mapping Quality", 2, 0, 0),
];

/// Water Physics Quality slider position (Quality, Balanced, Performance targets); the file stores the position.
const RDR2_WATER: (usize, usize, usize) = (2, 1, 0);

fn pick<T: Copy>(p: Preset, q: T, b: T, f: T) -> T {
    match p {
        Preset::Quality => q,
        Preset::Balanced => b,
        Preset::Performance => f,
    }
}

/// Changes for RDR2's system.xml, and the edited text. Unknown or Custom values are left alone.
pub fn rdr2_edit(text: &str, preset: Preset) -> (Vec<Change>, String) {
    let mut out = text.to_string();
    let mut changes = Vec::new();
    let short = |l: &str| l.trim_start_matches("kSettingLevel_").to_string();
    for (el, name, q, b, p) in RDR2_LEVELS {
        let (open, close) = (format!("<{el}>"), format!("</{el}>"));
        let Some(start) = out.find(&open).map(|i| i + open.len()) else { continue };
        let Some(len) = out[start..].find(&close) else { continue };
        let cur = out[start..start + len].to_string();
        let target = pick(preset, *q, *b, *p);
        if let Some(ci) = LEVELS.iter().position(|l| *l == cur) {
            if ci > target {
                out.replace_range(start..start + len, LEVELS[target]);
                changes.push((name.to_string(), short(&cur), short(LEVELS[target])));
            }
        }
    }
    let mut attr = |el: &str, name: &str, want: &dyn Fn(&str) -> Option<String>| {
        let open = format!("<{el} value=\"");
        let Some(start) = out.find(&open).map(|i| i + open.len()) else { return };
        let Some(len) = out[start..].find('"') else { return };
        let cur = out[start..start + len].to_string();
        if let Some(new) = want(&cur) {
            out.replace_range(start..start + len, &new);
            changes.push((name.to_string(), cur, new));
        }
    };
    let water = pick(preset, RDR2_WATER.0, RDR2_WATER.1, RDR2_WATER.2);
    attr("waterSimulationQuality", "Water Physics Quality", &|cur| {
        cur.parse::<usize>().ok().filter(|c| *c > water).map(|_| water.to_string())
    });
    attr("motionBlur", "Motion Blur", &|cur| (cur == "true").then(|| "false".to_string()));
    (changes, out)
}

// ---- Guides (games whose files aren't documented) ----------------------------------------------

/// Step-by-step instructions with the game's own menu names.
pub fn guide(game: &str, preset: Preset) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match (game, preset) {
        ("overwatch", Preset::Quality) => s(&[
            "Options > Video > Graphics Quality: High",
            "Render Scale: 100%, Dynamic Render Scale: Off",
            "NVIDIA Reflex: Enabled; V-Sync: Off; Triple Buffering: Off; Reduce Buffering: On",
            "Frame Rate: Custom, at your screen's refresh rate",
        ]),
        ("overwatch", Preset::Balanced) => s(&[
            "Options > Video > Graphics Quality: Medium",
            "Shadow Detail: Low; Local Fog Detail: Low; Dynamic Reflections: Off; Ambient Occlusion: Off",
            "Render Scale: 100%, Dynamic Render Scale: Off",
            "NVIDIA Reflex: Enabled; V-Sync: Off; Triple Buffering: Off; Reduce Buffering: On",
        ]),
        ("overwatch", Preset::Performance) => s(&[
            "Options > Video > Graphics Quality: Low (texture quality can stay High if your graphics card has 6 GB or more)",
            "Render Scale: 100% (lower to 75% only if you still need FPS), Dynamic Render Scale: Off",
            "NVIDIA Reflex: Enabled + Boost; V-Sync: Off; Triple Buffering: Off; Reduce Buffering: On",
            "Frame Rate: Custom, a little above your screen's refresh rate",
        ]),
        ("cod", Preset::Quality) => s(&[
            "Settings > Graphics > Quality: Graphic Preset: Custom",
            "Shader Quality: Medium (High costs up to a quarter of your FPS)",
            "Ray Tracing (RT Reflections): Off",
            "Screen Space Reflections: Low; Volumetric Quality: Low",
            "Upscaling: DLSS Quality (RTX) or FSR/XeSS Quality",
            "View: World Motion Blur Off, Weapon Motion Blur Off",
        ]),
        ("cod", Preset::Balanced) => s(&[
            "Settings > Graphics > Quality: Graphic Preset: Custom",
            "Shader Quality: Low; Shadow Quality: Normal; Volumetric Quality: Low",
            "Ray Tracing (RT Reflections): Off; Screen Space Reflections: Off",
            "Occlusion and Screen Space Lighting: Low; Particle Resolution: Very Low",
            "Upscaling: DLSS Balanced (RTX) or FSR/XeSS Balanced",
            "Display: NVIDIA Reflex Low Latency On, V-Sync (Gameplay) Off",
        ]),
        ("cod", Preset::Performance) => s(&[
            "Settings > Graphics > Quality: Shader Quality Low, Shadow Quality Low, Detail Quality Low, Terrain Quality Low",
            "Ray Tracing Off; Screen Space Reflections Off; Occlusion and Screen Space Lighting Off; Volumetric Quality Low",
            "Texture Resolution: Normal (keep higher on 10 GB+ graphics cards); VRAM Scale Target: 80",
            "Upscaling: DLSS Performance (RTX) or FSR Performance",
            "Display: NVIDIA Reflex Low Latency On + Boost; V-Sync Off; Custom Frame Rate Limit at your refresh rate",
            "View: World and Weapon Motion Blur Off",
        ]),
        _ => Vec::new(),
    }
}

// ---- Applying, with backup and Undo -------------------------------------------------------------

/// Computes a preset's changes and the edited file text.
type Editor = fn(&str, Preset) -> (Vec<Change>, String);

/// The settings file and its editor, for sources Kestrel writes.
fn editor(source: &Source) -> Option<(&Path, Editor)> {
    match source {
        Source::Unreal(p) => Some((p, unreal_edit)),
        Source::Rdr2(p) => Some((p, rdr2_edit)),
        Source::Guide(_) => None,
    }
}

/// What a preset would change (empty when nothing is above the preset's targets).
pub fn preview(source: &Source, preset: Preset) -> Result<Vec<Change>, String> {
    let Some((path, edit)) = editor(source) else { return Ok(Vec::new()) };
    let text = std::fs::read_to_string(path).map_err(|e| format!("Couldn't read the game's settings: {e}"))?;
    Ok(edit(&text, preset).0)
}

fn backup_path(file: &Path) -> PathBuf {
    let name: String = file.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    data_dir().join("game-settings").join(format!("{name}.original"))
}

/// Writes the preset. The game (exe file name) must be closed. The first original is kept for Undo.
pub fn apply(source: &Source, preset: Preset, exe_file: &str) -> Result<Vec<Change>, String> {
    let Some((path, edit)) = editor(source) else { return Err("This game's settings are changed in its own menu.".into()) };
    if crate::system::running_processes().contains(&exe_file.to_lowercase()) {
        return Err("Close the game first: it saves its settings when it exits and would undo these.".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("Couldn't read the game's settings: {e}"))?;
    let (changes, new) = edit(&text, preset);
    if changes.is_empty() {
        return Ok(changes);
    }
    let backup = backup_path(path);
    if !backup.exists() {
        let _ = std::fs::create_dir_all(backup.parent().unwrap_or(Path::new(".")));
        std::fs::write(&backup, &text).map_err(|e| format!("Couldn't save the original settings, so nothing was changed: {e}"))?;
    }
    write_atomic(path, new.as_bytes()).map_err(|e| format!("Couldn't write the game's settings: {e}"))?;
    // Read back so a file the game or a sync tool changed in between is noticed.
    if std::fs::read_to_string(path).ok().as_deref() != Some(new.as_str()) {
        return Err("The settings file didn't keep the change. Is it read-only or open in another program?".into());
    }
    log(&format!("Game settings: {} preset written to {} ({} changes)", preset.label(), path.display(), changes.len()));
    Ok(changes)
}

pub fn has_backup(source: &Source) -> bool {
    match source {
        Source::Unreal(p) | Source::Rdr2(p) => backup_path(p).exists(),
        Source::Guide(_) => false,
    }
}

/// Puts the original file back (the game must be closed).
pub fn undo(source: &Source, exe_file: &str) -> Result<(), String> {
    let (Source::Unreal(path) | Source::Rdr2(path)) = source else { return Err("Nothing to undo.".into()) };
    if crate::system::running_processes().contains(&exe_file.to_lowercase()) {
        return Err("Close the game first.".into());
    }
    let backup = backup_path(path);
    let original = std::fs::read(&backup).map_err(|_| "There's no saved original to put back.".to_string())?;
    write_atomic(path, &original).map_err(|e| format!("Couldn't restore the settings: {e}"))?;
    let _ = std::fs::remove_file(&backup);
    log(&format!("Game settings: original restored to {}", path.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RDR2: &str = "<graphics>\r\n    <tessellation>kSettingLevel_Ultra</tessellation>\r\n    <shadowQuality>kSettingLevel_High</shadowQuality>\r\n    <reflectionQuality>kSettingLevel_Medium</reflectionQuality>\r\n    <particleQuality>kSettingLevel_Custom</particleQuality>\r\n  </graphics>\r\n  <advancedGraphics>\r\n    <motionBlur value=\"true\" />\r\n    <waterSimulationQuality value=\"3\" />\r\n    <shadowSoftShadows>kSettingLevel_Ultra</shadowSoftShadows>\r\n  </advancedGraphics>\r\n";

    #[test]
    fn rdr2_only_lowers_known_values() {
        let (changes, out) = rdr2_edit(RDR2, Preset::Balanced);
        assert!(out.contains("<tessellation>kSettingLevel_Medium</tessellation>"));
        assert!(out.contains("<shadowQuality>kSettingLevel_High</shadowQuality>")); // already at Balanced's target
        assert!(out.contains("<reflectionQuality>kSettingLevel_Medium</reflectionQuality>")); // not raised or lowered
        assert!(out.contains("<particleQuality>kSettingLevel_Custom</particleQuality>")); // unknown value untouched
        assert!(out.contains("<motionBlur value=\"false\" />") && out.contains("<waterSimulationQuality value=\"1\" />"));
        assert!(out.contains("<shadowSoftShadows>kSettingLevel_Medium</shadowSoftShadows>") && out.contains("\r\n"));
        assert_eq!(changes.len(), 4);
        assert!(changes.contains(&("Water Physics Quality".into(), "3".into(), "1".into())));
        // Quality preset leaves a High shadow setting and lower values alone, but still trims Ultra.
        let (q, _) = rdr2_edit(RDR2, Preset::Quality);
        assert!(q.iter().any(|c| c.0.starts_with("Tessellation")) && !q.iter().any(|c| c.0 == "Shadow Quality"));
    }

    #[test]
    fn unreal_scalability_groups() {
        let ini = "[ScalabilityGroups]\r\nsg.ResolutionQuality=100\r\nsg.ShadowQuality=3\r\nsg.TextureQuality=3\r\nsg.ViewDistanceQuality=1\r\n\r\n[Other]\r\nsg.ShadowQuality=3\r\n";
        let (changes, out) = unreal_edit(ini, Preset::Performance);
        assert_eq!(changes, [("Shadows".to_string(), "Epic".to_string(), "Low".to_string())]);
        assert!(out.contains(
            "[ScalabilityGroups]\r\nsg.ResolutionQuality=100\r\nsg.ShadowQuality=0\r\nsg.TextureQuality=3\r\nsg.ViewDistanceQuality=1\r\n"
        ));
        assert!(out.ends_with("[Other]\r\nsg.ShadowQuality=3\r\n")); // other sections untouched
    }

    #[test]
    fn unreal_project_names() {
        assert_eq!(
            unreal_project(r"D:\Epic\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe").as_deref(),
            Some("FortniteGame")
        );
        assert_eq!(unreal_project(r"C:\Games\Palworld\Pal-Win64-Shipping.exe").as_deref(), Some("Pal"));
        assert_eq!(norm("Hogwarts Legacy"), "hogwartslegacy");
    }

    #[test]
    #[ignore = "reads this PC's game settings (read-only)"]
    fn live_preview() {
        for (name, exe) in
            [("Red Dead Redemption 2", r"E:\RDR2\Red Dead Redemption 2\RDR2.exe"), ("Overwatch", r"E:\Overwatch\_retail_\Overwatch.exe")]
        {
            let src = find(name, exe);
            eprintln!("{name}: {src:?}");
            if let Some(s) = &src {
                for p in Preset::ALL {
                    eprintln!("  {}: {:?}", p.label(), preview(s, p).unwrap_or_default());
                }
            }
        }
    }

    #[test]
    fn guides_exist_for_every_preset() {
        for g in ["overwatch", "cod"] {
            assert!(Preset::ALL.iter().all(|p| !guide(g, *p).is_empty()));
        }
    }
}
