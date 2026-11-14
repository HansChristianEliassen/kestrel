//! Game Mode: temporary, user-approved changes while you play, undone afterwards.
//!
//! Before anything changes, the original value is appended to a session snapshot
//! (`gamemode-session.txt`) and a RunOnce entry is registered, so a crash, power cut or
//! restart can't leave settings changed: Kestrel restores them at the next sign-in or launch.

pub mod watch;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::optimizer::Tweak;
use crate::utils::{data_dir, log};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Balanced,
    Performance,
    Custom,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::Balanced, Profile::Performance, Profile::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Profile::Balanced => "Balanced",
            Profile::Performance => "Performance",
            Profile::Custom => "Custom",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            Profile::Balanced => {
                "Makes sure Windows Game Mode is on, pauses background recording and switches to the Best performance power mode."
            }
            Profile::Performance => "Adds keeping USB devices awake and closing the apps in your list, which frees memory for the game.",
            Profile::Custom => "Only the actions you tick below.",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Profile::Balanced => "balanced",
            Profile::Performance => "performance",
            Profile::Custom => "custom",
        }
    }

    fn from_key(s: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.key() == s)
    }

    /// The actions this profile runs; Custom uses the saved custom set.
    pub fn actions(self, config: &Config) -> Vec<Action> {
        match self {
            Profile::Balanced => vec![Action::WindowsGameMode, Action::RecordingOff, Action::PowerMode],
            // The High performance plan stays Custom-only: power mode is Windows 11's supported switch.
            Profile::Performance => Action::ALL.into_iter().filter(|a| *a != Action::HighPerformance).collect(),
            Profile::Custom => Action::ALL.into_iter().filter(|a| config.custom.contains(a)).collect(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    WindowsGameMode,
    RecordingOff,
    PowerMode,
    HighPerformance,
    UsbAwake,
    CloseApps,
}

impl Action {
    /// Order matters: the power plan switches before the per-plan USB setting, and restore runs in reverse.
    pub const ALL: [Action; 6] =
        [Action::WindowsGameMode, Action::RecordingOff, Action::PowerMode, Action::HighPerformance, Action::UsbAwake, Action::CloseApps];

    /// The reversible setting behind this action (all but closing apps).
    fn tweak(self) -> Option<Tweak> {
        match self {
            Action::WindowsGameMode => Some(Tweak::GameMode),
            Action::RecordingOff => Some(Tweak::GameDvrOff),
            Action::PowerMode => Some(Tweak::PowerMode),
            Action::HighPerformance => Some(Tweak::HighPerfPower),
            Action::UsbAwake => Some(Tweak::UsbSuspendOff),
            Action::CloseApps => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::WindowsGameMode => "Make sure Windows Game Mode is on",
            Action::RecordingOff => "Pause background game recording",
            Action::PowerMode => "Best performance power mode",
            Action::HighPerformance => "High performance power plan",
            Action::UsbAwake => "Keep USB devices awake",
            Action::CloseApps => "Close the apps in your list",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            Action::WindowsGameMode => "Windows gives the game priority and holds back Windows Update installs while you play.",
            Action::RecordingOff => "Stops Xbox Game Bar's always-on recording of the last few minutes for this session.",
            Action::PowerMode => "Switches Windows 11's power mode to Best performance while you play, so the CPU boosts sooner. Most useful on laptops; skipped when your power plan isn't Balanced.",
            Action::HighPerformance => "Keeps the CPU at higher clock speeds while you play. Your previous power plan comes back afterwards.",
            Action::UsbAwake => "Turns off USB selective suspend for the session, avoiding wake-up delays on controllers and headsets.",
            Action::CloseApps => "Politely asks each app in your list to close, like clicking its X. Nothing is force-closed, so apps with unsaved work can still ask you to save.",
        }
    }

    pub fn warning(self) -> Option<&'static str> {
        match self {
            Action::HighPerformance => Some("Uses more power and runs warmer. On a laptop, play plugged in."),
            Action::CloseApps => {
                Some("Closed apps are not reopened afterwards. Some apps (Discord, Steam) only hide in the tray when closed.")
            }
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Action::WindowsGameMode => "windows_game_mode",
            Action::RecordingOff => "recording_off",
            Action::PowerMode => "power_mode",
            Action::HighPerformance => "high_performance",
            Action::UsbAwake => "usb_awake",
            Action::CloseApps => "close_apps",
        }
    }

    fn from_key(s: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.key() == s)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameEntry {
    /// Full path of the game's executable (or just its file name).
    pub exe: String,
    pub profile: Profile,
}

impl GameEntry {
    /// Lower-case file name, which is what detection matches on.
    pub fn file_name(&self) -> String {
        Path::new(&self.exe).file_name().map_or_else(|| self.exe.clone(), |f| f.to_string_lossy().into_owned()).to_lowercase()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub auto: bool,
    /// Settings > Minimize to tray when closed. Kept here because automatic Game Mode is what it's for.
    pub tray: bool,
    /// Profile used when Game Mode is turned on by hand.
    pub manual_profile: Profile,
    pub custom: Vec<Action>,
    /// Lower-case executable names of apps to close.
    pub close: Vec<String>,
    pub games: Vec<GameEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            auto: false,
            tray: false,
            manual_profile: Profile::Balanced,
            custom: vec![Action::WindowsGameMode, Action::RecordingOff],
            close: Vec::new(),
            games: Vec::new(),
        }
    }
}

impl Config {
    fn parse(text: &str) -> Config {
        let mut c = Config::default();
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            match f[..] {
                ["auto", v] => c.auto = v == "1",
                ["tray", v] => c.tray = v == "1",
                ["profile", p] => c.manual_profile = Profile::from_key(p).unwrap_or(Profile::Balanced),
                ["custom", list] => c.custom = list.split(',').filter_map(Action::from_key).collect(),
                ["close", exe] if !exe.is_empty() => c.close.push(exe.to_lowercase()),
                ["game", p, exe] if !exe.is_empty() => {
                    c.games.push(GameEntry { exe: exe.to_string(), profile: Profile::from_key(p).unwrap_or(Profile::Balanced) })
                }
                _ => {}
            }
        }
        c
    }

    fn to_text(&self) -> String {
        let mut s = format!(
            "auto\t{}\ntray\t{}\nprofile\t{}\ncustom\t{}\n",
            u8::from(self.auto),
            u8::from(self.tray),
            self.manual_profile.key(),
            self.custom.iter().map(|a| a.key()).collect::<Vec<_>>().join(",")
        );
        for exe in &self.close {
            s.push_str(&format!("close\t{exe}\n"));
        }
        for g in &self.games {
            s.push_str(&format!("game\t{}\t{}\n", g.profile.key(), g.exe));
        }
        s
    }

    pub fn load() -> Config {
        Config::parse(&fs::read_to_string(data_dir().join("gamemode.txt")).unwrap_or_default())
    }

    pub fn save(&self) -> Result<(), String> {
        fs::write(data_dir().join("gamemode.txt"), self.to_text()).map_err(|e| format!("Couldn't save Game Mode settings: {e}"))
    }

    /// The configured game whose executable is running, if any (first match wins).
    pub fn running_game(&self, running: &std::collections::HashSet<String>) -> Option<&GameEntry> {
        self.games.iter().find(|g| running.contains(&g.file_name()))
    }
}

fn session_path() -> PathBuf {
    data_dir().join("gamemode-session.txt")
}

/// True while Game Mode changes are applied (or left over from a crash and not yet restored).
pub fn is_active() -> bool {
    session_path().exists()
}

const RUN_ONCE: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
const RUN_ONCE_VALUE: &str = "KestrelGameModeRestore";
/// Command-line flag the RunOnce entry uses: restore and exit without showing a window.
pub const RESTORE_FLAG: &str = "--restore-gamemode";

/// Windows runs this once at the next sign-in, so a restart during Game Mode still gets everything restored.
fn set_recovery(on: bool) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_ONCE).map_err(|e| e.to_string())?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value(RUN_ONCE_VALUE, &format!("\"{}\" {RESTORE_FLAG}", exe.display())).map_err(|e| e.to_string())
    } else {
        match key.delete_value(RUN_ONCE_VALUE) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

fn append_session(line: &str) -> Result<(), String> {
    let mut f = OpenOptions::new().create(true).append(true).open(session_path()).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").and_then(|()| f.sync_all()).map_err(|e| format!("Couldn't save the Game Mode snapshot: {e}"))
}

/// Applies `actions`, saving each original value before changing it. Returns one line per action.
pub fn activate(profile: Profile, actions: &[Action], close: &[String], trigger: &str) -> Vec<String> {
    // A second snapshot would record Game Mode's own values as the "originals".
    if is_active() {
        return vec!["Game Mode is already on. Turn it off (or restore) first.".into()];
    }
    let mut lines = Vec::new();
    if let Err(e) = set_recovery(true).and_then(|()| append_session(&format!("start\t{}\t{trigger}", profile.key()))) {
        let _ = set_recovery(false);
        return vec![format!("✖ Game Mode didn't start: {e}")];
    }
    for &a in actions {
        match a.tweak() {
            // Nothing to change (already set, or no effect on this PC): no snapshot, so nothing to undo either.
            Some(t) if t.not_needed().is_some() || t.is_applied() => {
                let why = t.not_needed().unwrap_or_else(|| "already set".into());
                lines.push(format!("Skipped {}: {why}", a.label()));
            }
            Some(t) => {
                // The snapshot is on disk before the setting changes, so a crash right after can still be undone.
                let result = t.snapshot().and_then(|s| append_session(&format!("undo\t{}\t{s}", a.key()))).and_then(|()| t.write(None));
                lines.push(match result {
                    Ok(()) => format!("✔ {}", a.label()),
                    Err(e) => format!("✖ {}: {e}", a.label()),
                });
            }
            None => lines.extend(watch::close_apps(close)),
        }
    }
    log(&format!("Game Mode on ({}, {trigger}): {}", profile.label(), lines.join("; ")));
    lines
}

/// Undo entries from a session file, oldest first: (action, saved original).
fn parse_session(text: &str) -> Vec<(Action, String)> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            (f.next()? == "undo").then_some(())?;
            Some((Action::from_key(f.next()?)?, f.next()?.to_string()))
        })
        .collect()
}

/// Puts back every saved original, newest first. Entries that fail stay in the snapshot for
/// another try, and Game Mode only counts as off once everything was restored.
pub fn restore() -> (Vec<String>, bool) {
    let Ok(text) = fs::read_to_string(session_path()) else {
        let _ = set_recovery(false);
        return (vec!["Game Mode wasn't on, so there was nothing to restore.".into()], true);
    };
    let mut lines = Vec::new();
    let mut failed = Vec::new();
    for (action, original) in parse_session(&text).into_iter().rev() {
        let Some(t) = action.tweak() else { continue };
        match t.write(Some(&original)) {
            Ok(()) => lines.push(format!("✔ Restored: {}", action.label())),
            Err(e) => {
                lines.push(format!("✖ Couldn't restore {}: {e}", action.label()));
                failed.push((action, original));
            }
        }
    }
    let all_ok = failed.is_empty();
    if all_ok {
        let _ = fs::remove_file(session_path());
        let _ = set_recovery(false);
    } else {
        // Keep only what still needs restoring, oldest first, so the next try runs in the right order.
        let keep: String = failed.iter().rev().map(|(a, o)| format!("undo\t{}\t{o}\n", a.key())).collect();
        let _ = fs::write(session_path(), format!("start\tretry\trestore\n{keep}"));
    }
    if lines.is_empty() {
        lines.push("No settings needed restoring.".into());
    }
    log(&format!("Game Mode off: {}", lines.join("; ")));
    (lines, all_ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip() {
        let c = Config {
            auto: true,
            tray: true,
            manual_profile: Profile::Performance,
            custom: vec![Action::HighPerformance, Action::CloseApps],
            close: vec!["discord.exe".into()],
            games: vec![GameEntry { exe: r"E:\Call of Duty\_retail_\cod.exe".into(), profile: Profile::Custom }],
        };
        assert_eq!(Config::parse(&c.to_text()), c);
        assert_eq!(Config::parse(""), Config::default());
        assert_eq!(c.games[0].file_name(), "cod.exe");
    }

    #[test]
    fn profiles() {
        let c = Config { custom: vec![Action::UsbAwake], ..Config::default() };
        assert_eq!(Profile::Balanced.actions(&c), vec![Action::WindowsGameMode, Action::RecordingOff, Action::PowerMode]);
        assert_eq!(Profile::Performance.actions(&c).len(), 5);
        assert!(!Profile::Performance.actions(&c).contains(&Action::HighPerformance));
        assert_eq!(Profile::Custom.actions(&c), vec![Action::UsbAwake]);
    }

    fn run_once_set() -> bool {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_ONCE).and_then(|k| k.get_value::<String, _>(RUN_ONCE_VALUE)).is_ok()
    }

    /// Real on/off cycle plus a failed restore: `cargo test live_ -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_on_off_and_failed_restore() {
        assert!(!is_active(), "a real Game Mode session is active; not touching it");
        let actions = [Action::WindowsGameMode, Action::RecordingOff, Action::HighPerformance, Action::UsbAwake];
        let before: Vec<String> = actions.iter().map(|a| a.tweak().unwrap().snapshot().unwrap()).collect();
        let on = activate(Profile::Performance, &actions, &[], "live test");
        println!("on: {on:?}");
        assert!(is_active() && run_once_set());
        let (off, ok) = restore();
        println!("off: {off:?}");
        assert!(ok && !is_active() && !run_once_set());
        let after: Vec<String> = actions.iter().map(|a| a.tweak().unwrap().snapshot().unwrap()).collect();
        assert_eq!(before, after, "settings differ after restore");

        // A restore that can't succeed keeps its entry and reports failure instead of claiming success.
        fs::write(session_path(), "start\tperformance\ttest\nundo\thigh_performance\tnot-a-guid\n").unwrap();
        let (lines, ok) = restore();
        println!("failed restore: {lines:?}");
        assert!(!ok && is_active());
        assert_eq!(parse_session(&fs::read_to_string(session_path()).unwrap()).len(), 1);
        fs::remove_file(session_path()).unwrap();
        set_recovery(false).unwrap();
    }

    #[test]
    #[ignore]
    fn live_close_app() {
        let running = || crate::system::running_processes().contains("charmap.exe");
        assert!(!running(), "Character Map is already open; not closing your window");
        let mut child = std::process::Command::new("charmap.exe").spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let lines = watch::close_apps(&["charmap.exe".into(), "explorer.exe".into()]);
        println!("{lines:?}");
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert!(child.try_wait().unwrap().is_some(), "Character Map didn't close");
        assert!(lines[1].starts_with('✖'), "explorer.exe must be refused");
    }

    #[test]
    fn session_entries() {
        let text = "start\tperformance\tmanual\nundo\thigh_performance\t381b4222-f694-41f0-9685-ff5bb260df2e\nundo\tusb_awake\t1\nbogus\n";
        let s = parse_session(text);
        assert_eq!(s, vec![(Action::HighPerformance, "381b4222-f694-41f0-9685-ff5bb260df2e".into()), (Action::UsbAwake, "1".into())]);
    }
}
