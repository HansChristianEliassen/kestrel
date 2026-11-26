//! Finds installed games (Steam, Epic, GOG, Battle.net, EA, Riot, Ubisoft, Rockstar)
//! plus programs that already have per-game settings.
//! Read-only: it only reads launcher manifests, the registry and folder listings.

use std::collections::BTreeMap;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

use super::gaming::{configured, GameSetting};
use crate::cleaner::scanner::REPARSE_POINT;
use crate::system::env_path;
use crate::utils::{backup, json_str};

pub struct Game {
    pub name: String,
    pub source: &'static str,
    /// Candidate executables, best guess first.
    pub exes: Vec<String>,
    pub chosen: usize,
    /// (on, changed by Kestrel) per `GameSetting::ALL`, for the chosen exe.
    pub status: [(bool, bool); GameSetting::ALL.len()],
}

impl Game {
    pub fn exe(&self) -> &str {
        &self.exes[self.chosen]
    }

    pub fn refresh(&mut self) {
        let saved = backup::all();
        self.status = GameSetting::ALL.map(|s| s.status(self.exe(), &saved));
    }

    pub fn from_exe(exe: String, source: &'static str) -> Game {
        let name = Path::new(&exe).file_stem().map_or_else(|| exe.clone(), |s| s.to_string_lossy().into_owned());
        let mut g = Game { name, source, exes: vec![exe], chosen: 0, status: Default::default() };
        g.refresh();
        g
    }
}

/// Folders inside a game install that never hold the game itself.
const SKIP_DIRS: &[&str] = &[
    "_commonredist",
    "__installer",
    "redist",
    "redistributables",
    "directx",
    "support",
    "engine",
    "easyanticheat",
    "battleye",
    "prereqs",
    "installers",
    "dotnet",
    "vcredist",
];

/// File-name fragments of helpers that ship next to games.
const NOT_GAME: &[&str] = &[
    "unins",
    "setup",
    "install",
    "redist",
    "crash",
    "report",
    "helper",
    "updater",
    "prereq",
    "dxsetup",
    "dotnet",
    "cefprocess",
    "webhelper",
    "touchup",
    "cleanup",
    "uploader",
    "easyanticheat",
    "start_protected_game",
    "be_service",
    "server",
    "editor",
    "vc_redist",
    "oalinst",
];

fn alnum(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// Likely game executables under `dir`, best first: "*-Shipping.exe", then names like the game, then size.
fn find_exes(dir: &Path, hint: &str) -> Vec<String> {
    let hint = alnum(hint);
    let mut found: Vec<(u8, u64, PathBuf)> = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    let mut visited = 0;
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            visited += 1;
            if visited > 20_000 {
                break; // shortcut: stops looking in huge installs; the user can add the exe with the file picker
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.file_attributes() & REPARSE_POINT != 0 {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if meta.is_dir() {
                if depth < 4 && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push((e.path(), depth + 1));
                }
            } else if name.ends_with(".exe") && !NOT_GAME.iter().any(|n| name.contains(n)) {
                let stem = alnum(name.trim_end_matches(".exe"));
                let score =
                    2 * u8::from(name.contains("shipping")) + u8::from(!hint.is_empty() && (stem.contains(&hint) || hint.contains(&stem)));
                found.push((score, meta.len(), e.path()));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    found.into_iter().take(8).map(|(_, _, p)| p.to_string_lossy().into_owned()).collect()
}

/// Quoted values for `key` in Valve's KeyValues text ("key"  "value" on one line).
fn vdf_values(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            // shortcut: doesn't handle \" inside values; game names with quotes keep a cut-off name.
            let tokens: Vec<&str> = l.split('"').skip(1).step_by(2).collect();
            (tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case(key)).then(|| tokens[1].replace("\\\\", "\\"))
        })
        .collect()
}

fn steam(out: &mut Vec<Game>, locate: Locate) {
    let Ok(root) =
        RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").and_then(|k| k.get_value::<String, _>("SteamPath"))
    else {
        return;
    };
    let root = PathBuf::from(root.replace('/', "\\"));
    let vdf = fs::read_to_string(root.join(r"steamapps\libraryfolders.vdf")).unwrap_or_default();
    let mut libraries: Vec<PathBuf> = vdf_values(&vdf, "path").into_iter().map(PathBuf::from).collect();
    if libraries.is_empty() {
        libraries.push(root);
    }
    for lib in libraries {
        let Ok(rd) = fs::read_dir(lib.join("steamapps")) else { continue };
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            if !(file.starts_with("appmanifest_") && file.ends_with(".acf")) {
                continue;
            }
            let text = fs::read_to_string(e.path()).unwrap_or_default();
            let (Some(name), Some(dir)) =
                (vdf_values(&text, "name").into_iter().next(), vdf_values(&text, "installdir").into_iter().next())
            else {
                continue;
            };
            if ["Redistributable", "Steamworks", "Proton", "Steam Linux Runtime"].iter().any(|t| name.contains(t)) {
                continue;
            }
            push(out, name, "Steam", locate(&lib.join(r"steamapps\common").join(&dir), &dir));
        }
    }
}

fn epic(out: &mut Vec<Game>, locate: Locate) {
    let Ok(rd) = fs::read_dir(env_path("ProgramData").join(r"Epic\EpicGamesLauncher\Data\Manifests")) else { return };
    for e in rd.flatten() {
        if !e.file_name().to_string_lossy().ends_with(".item") {
            continue;
        }
        let text = fs::read_to_string(e.path()).unwrap_or_default();
        let (Some(name), Some(dir), Some(exe)) =
            (json_str(&text, "DisplayName"), json_str(&text, "InstallLocation"), json_str(&text, "LaunchExecutable"))
        else {
            continue;
        };
        let exe = Path::new(&dir).join(exe.replace('/', "\\"));
        let mut exes = vec![exe.to_string_lossy().into_owned()];
        exes.retain(|p| Path::new(p).is_file());
        if exes.is_empty() {
            exes = locate(Path::new(&dir), &name);
        }
        push(out, name, "Epic", exes);
    }
}

fn gog(out: &mut Vec<Game>, locate: Locate) {
    let Ok(games) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\WOW6432Node\GOG.com\Games") else { return };
    for id in games.enum_keys().flatten() {
        let Ok(k) = games.open_subkey(&id) else { continue };
        let (Ok(name), Ok(dir)) = (k.get_value::<String, _>("gameName"), k.get_value::<String, _>("path")) else { continue };
        let exe = k.get_value::<String, _>("exe").map(|e| Path::new(&dir).join(e)).ok().filter(|p| p.is_file());
        let exes = match exe {
            Some(p) => vec![p.to_string_lossy().into_owned()],
            None => locate(Path::new(&dir), &name),
        };
        push(out, name, "GOG", exes);
    }
}

/// Games from launchers that register each game in Windows' uninstall list (Battle.net, EA, Riot, Ubisoft, Rockstar).
fn launcher_games(out: &mut Vec<Game>, locate: Locate) {
    const PUBLISHERS: &[(&str, &str)] = &[
        ("Blizzard Entertainment", "Battle.net"),
        ("Activision", "Battle.net"),
        ("Electronic Arts", "EA"),
        ("Riot Games", "Riot"),
        ("Ubisoft", "Ubisoft"),
        ("Rockstar Games", "Rockstar"),
    ];
    const LAUNCHERS: &[&str] =
        &["battle.net", "ea app", "origin", "riot client", "ubisoft connect", "uplay", "rockstar games launcher", "social club"];
    let uninstall = r"Microsoft\Windows\CurrentVersion\Uninstall";
    for (hive, path) in [
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\{uninstall}")),
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\WOW6432Node\{uninstall}")),
        (HKEY_CURRENT_USER, format!(r"Software\{uninstall}")),
    ] {
        let Ok(root) = RegKey::predef(hive).open_subkey(&path) else { continue };
        for sub in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&sub) else { continue };
            let get = |v: &str| k.get_value::<String, _>(v).unwrap_or_default();
            let (name, publisher, dir) = (get("DisplayName"), get("Publisher"), get("InstallLocation"));
            let Some(&(_, source)) = PUBLISHERS.iter().find(|(p, _)| publisher.contains(p)) else { continue };
            if dir.is_empty() || LAUNCHERS.iter().any(|l| name.to_lowercase().starts_with(l)) {
                continue;
            }
            // DisplayIcon usually points at the game's own exe ("E:\Game\game.exe,0").
            let icon = get("DisplayIcon");
            let icon = icon.split(',').next().unwrap_or_default().trim_matches('"').to_string();
            let exes = if icon.to_ascii_lowercase().ends_with(".exe") && Path::new(&icon).is_file() {
                vec![icon]
            } else {
                locate(Path::new(&dir), &name)
            };
            push(out, name, source, exes);
        }
    }
}

fn push(out: &mut Vec<Game>, name: String, source: &'static str, exes: Vec<String>) {
    if !exes.is_empty() {
        out.push(Game { name, source, exes, chosen: 0, status: Default::default() });
    }
}

/// Turns an install folder into candidate exes.
type Locate = fn(&Path, &str) -> Vec<String>;

fn from_launchers(locate: Locate) -> Vec<Game> {
    let mut games = Vec::new();
    steam(&mut games, locate);
    epic(&mut games, locate);
    gog(&mut games, locate);
    launcher_games(&mut games, locate);
    games
}

/// Drive letters that hold launcher games, from manifests only (no folder walks), for the health check.
pub fn install_drives() -> Vec<char> {
    from_launchers(|dir, _| vec![dir.to_string_lossy().into_owned()])
        .iter()
        .filter_map(|g| g.exe().chars().next().map(|c| c.to_ascii_uppercase()))
        .collect()
}

/// Installed games from Steam, Epic and GOG, then other programs that already have per-game settings.
pub fn find_all() -> Vec<Game> {
    let mut games = from_launchers(find_exes);
    games.sort_by_key(|g| g.name.to_lowercase());
    games.dedup_by(|a, b| a.exe().eq_ignore_ascii_case(b.exe()));

    // If a per-game setting already exists for one of a game's candidate exes, show that exe first.
    let others = configured();
    let lower: BTreeMap<String, &String> = others.iter().map(|e| (e.to_lowercase(), e)).collect();
    for g in &mut games {
        if let Some(i) = g.exes.iter().position(|e| lower.contains_key(&e.to_lowercase())) {
            g.chosen = i;
        }
    }
    let known: Vec<String> = games.iter().flat_map(|g| g.exes.iter().map(|e| e.to_lowercase())).collect();
    for exe in others {
        if !known.contains(&exe.to_lowercase()) {
            games.push(Game::from_exe(exe, "Other"));
        }
    }
    let saved = backup::all();
    for g in &mut games {
        g.status = GameSetting::ALL.map(|s| s.status(g.exe(), &saved));
    }
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vdf() {
        let text = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t\t\"label\"\t\t\"\"\n";
        assert_eq!(vdf_values(text, "path"), vec![r"D:\SteamLibrary".to_string()]);
        assert_eq!(vdf_values("\"name\"\t\t\"Counter-Strike 2\"", "name"), vec!["Counter-Strike 2".to_string()]);
    }

    #[test]
    fn parses_json() {
        let text = r#"{ "DisplayName": "Fortnite", "InstallLocation": "C:\\Program Files\\Epic Games\\Fortnite", "LaunchExecutable": "FortniteGame/Binaries/Win64/FortniteLauncher.exe", "Title": "Caf\u00e9 \"X\"" }"#;
        assert_eq!(json_str(text, "DisplayName").as_deref(), Some("Fortnite"));
        assert_eq!(json_str(text, "InstallLocation").as_deref(), Some(r"C:\Program Files\Epic Games\Fortnite"));
        assert_eq!(json_str(text, "Title").as_deref(), Some("Café \"X\""));
        assert_eq!(json_str(text, "Missing"), None);
    }

    #[test]
    fn ranks_exes() {
        let dir = std::env::temp_dir().join("kestrel_games_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(r"Game\Binaries\Win64")).unwrap();
        fs::create_dir_all(dir.join("_CommonRedist")).unwrap();
        fs::write(dir.join(r"Game\Binaries\Win64\Game-Win64-Shipping.exe"), b"x").unwrap();
        fs::write(dir.join("Game.exe"), vec![0u8; 100]).unwrap();
        fs::write(dir.join("unins000.exe"), b"x").unwrap();
        fs::write(dir.join("_CommonRedist").join("vc.exe"), b"x").unwrap();
        let exes = find_exes(&dir, "Game");
        assert_eq!(exes.len(), 2);
        assert!(exes[0].ends_with("Game-Win64-Shipping.exe"));
        let _ = fs::remove_dir_all(&dir);
    }
}
