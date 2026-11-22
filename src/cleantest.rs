//! Clean test: a temporary, reviewed version of Microsoft's "clean boot". Other programs' services that start with
//! Windows and the startup apps are turned off for a restart, so the user can measure a game without them, then
//! everything is put back. Windows' own services and drivers are never touched, and neither is anything games
//! need (anti-cheat, launchers) or hardware software (graphics, audio, network, input) or security software.
//! The originals are kept in their own record file, so ending the test never undoes the user's own earlier choices.

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::services::{self, Start};
use crate::utils::{data_dir, log, write_atomic};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// A service, with its start type before the test.
    Service(Start),
    /// A startup app: (location, command), to find it again.
    Startup(String, String),
}

/// Something the test turns off.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Service key name, or the startup app's name.
    pub name: String,
    /// What it is, as shown: display name or publisher.
    pub detail: String,
    pub kind: Kind,
}

impl Item {
    pub fn id(&self) -> String {
        match &self.kind {
            Kind::Service(_) => format!("service:{}", self.name),
            Kind::Startup(location, command) => format!("startup:{}|{location}|{command}", self.name),
        }
    }
}

#[derive(Clone, Default)]
pub struct Plan {
    pub services: Vec<Item>,
    pub startup: Vec<Item>,
    /// (name, why it stays on).
    pub kept: Vec<(String, &'static str)>,
}

/// (words matched in the lower-case name, display name and company, why it stays on, services only).
const KEEP: &[(&[&str], &str, bool)] = &[
    (
        &[
            "anticheat",
            "anti-cheat",
            "battleye",
            "beservice",
            "easyanticheat",
            "vanguard",
            "vgc",
            "faceit",
            "denuvo",
            "pnkbstr",
            "punkbuster",
            "xigncode",
            "nprotect",
            "gameguard",
            "ricochet",
        ],
        "Anti-cheat: games won't start without it.",
        false,
    ),
    (
        &[
            "nvidia",
            "advanced micro devices",
            "amd ",
            "intel",
            "realtek",
            "qualcomm",
            "mediatek",
            "broadcom",
            "killer",
            "rivet",
            "nahimic",
            "a-volute",
            "waves",
            "dolby",
            "synaptics",
            "elan",
            "logitech",
            "razer",
            "corsair",
            "steelseries",
            "hyperx",
            "asus",
            "msi",
            "gigabyte",
            "input",
            "xbox",
            "gamepad",
            "controller",
            "ddk provider",
        ],
        "Hardware software (graphics, audio, network, Bluetooth, input or the motherboard).",
        false,
    ),
    (
        &[
            "kaspersky",
            "eset",
            "bitdefender",
            "norton",
            "mcafee",
            "avast",
            "avg ",
            "malwarebytes",
            "sophos",
            "trend micro",
            "f-secure",
            "avira",
            "webroot",
            "gen digital",
        ],
        "Security software.",
        false,
    ),
    (
        &[
            "steam",
            "valve",
            "rockstar",
            "electronic arts",
            "eabackground",
            "battle.net",
            "blizzard",
            "epic games",
            "epiconlineservices",
            "ubisoft",
            "riot",
        ],
        "Game launcher: some games need it to start.",
        true,
    ),
];

/// Why this stays on during the test, if it does. `service` is false for startup apps, where launchers may go off.
pub fn keep_reason(name: &str, display: &str, company: &str, service: bool) -> Option<&'static str> {
    let text = format!(" {} {} {} ", name.to_lowercase(), display.to_lowercase(), company.to_lowercase());
    if text.contains("kestrel") {
        return Some("Kestrel itself.");
    }
    KEEP.iter()
        .find(|(words, _, only_services)| (service || !only_services) && words.iter().any(|w| text.contains(w)))
        .map(|(_, why, _)| *why)
}

/// What a clean test would turn off on this PC, and what it keeps. Read-only.
pub fn plan() -> Plan {
    let mut p = Plan::default();
    for (name, display, image, start) in services::autostart() {
        // Only programs with a known, non-Microsoft company: Windows' own services, and anything unidentified, stay.
        let Some(company) = crate::system::file_company(&image) else { continue };
        if company.to_lowercase().contains("microsoft") {
            continue;
        }
        match keep_reason(&name, &display, &company, true) {
            Some(why) => p.kept.push((display, why)),
            None => p.services.push(Item { name, detail: format!("{display} · {company}"), kind: Kind::Service(start) }),
        }
    }
    for e in crate::startup::list().into_iter().filter(|e| e.enabled) {
        let why = if e.protected { Some("Security software.") } else { keep_reason(&e.name, &e.command, &e.publisher, false) };
        match why {
            Some(why) => p.kept.push((e.name, why)),
            None => {
                let publisher = if e.publisher.is_empty() { "Unknown publisher".to_string() } else { e.publisher.clone() };
                p.startup.push(Item {
                    name: e.name,
                    detail: format!("{publisher} · {}", e.location),
                    kind: Kind::Startup(e.location.to_string(), e.command),
                })
            }
        }
    }
    p.services.sort_by_key(|i| i.detail.to_lowercase());
    p.startup.sort_by_key(|i| i.name.to_lowercase());
    p
}

fn record_path() -> std::path::PathBuf {
    data_dir().join("clean-test.txt")
}

const RUN_ONCE: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
const RUN_ONCE_VALUE: &str = "KestrelCleanTest";

fn to_line(i: &Item) -> String {
    match &i.kind {
        Kind::Service(start) => format!("service\t{}\t{}", i.name, start.code()),
        Kind::Startup(location, command) => format!("startup\t{}\t{location}\t{command}", i.name),
    }
}

fn from_line(line: &str) -> Option<Item> {
    let f: Vec<&str> = line.split('\t').collect();
    match f.as_slice() {
        ["service", name, code] => {
            Some(Item { name: name.to_string(), detail: String::new(), kind: Kind::Service(Start::from_code(code)?) })
        }
        ["startup", name, location, command] => {
            Some(Item { name: name.to_string(), detail: String::new(), kind: Kind::Startup(location.to_string(), command.to_string()) })
        }
        _ => None,
    }
}

/// The items of the running test, if one is on.
pub fn active() -> Option<Vec<Item>> {
    let text = std::fs::read_to_string(record_path()).ok()?;
    Some(text.lines().filter_map(from_line).collect())
}

/// (services, startup apps) off while a test is on.
pub fn active_counts() -> Option<(usize, usize)> {
    let items = active()?;
    let services = items.iter().filter(|i| matches!(i.kind, Kind::Service(_))).count();
    Some((services, items.len() - services))
}

/// Starts a test: records every original first (so a crash midway can still be undone), turns the items off and
/// has Kestrel open once after the next sign-in. One line per item.
pub fn start(items: &[Item]) -> Vec<String> {
    if active().is_some() {
        return vec!["A clean test is already on. End it first.".into()];
    }
    let record: String = items.iter().map(|i| to_line(i) + "\n").collect();
    if let Err(e) = write_atomic(&record_path(), record.as_bytes()) {
        return vec![format!("✖ The clean test didn't start: couldn't save the record ({e}).")];
    }
    log(&format!("Clean test started: {} items off", items.len()));
    let mut lines: Vec<String> = items.iter().map(|i| switch(i, false)).collect();
    if let Err(e) = run_once(true) {
        lines.push(format!("Kestrel won't open by itself after the restart ({e}); open it yourself to end the test."));
    }
    lines
}

/// Ends the test: puts every recorded item back and deletes the record. Nothing to do when no test is on.
pub fn end() -> Vec<String> {
    let Some(items) = active() else { return Vec::new() };
    let lines: Vec<String> = items.iter().map(|i| switch(i, true)).collect();
    let _ = std::fs::remove_file(record_path());
    let _ = run_once(false);
    log("Clean test ended: everything put back");
    lines
}

fn switch(i: &Item, back: bool) -> String {
    let result = match &i.kind {
        Kind::Service(original) => services::set_start(&i.name, if back { *original } else { Start::Disabled }),
        Kind::Startup(location, command) => crate::startup::list()
            .into_iter()
            .find(|e| e.name == i.name && e.location == location.as_str() && e.command == *command)
            .ok_or_else(|| "not found any more".to_string())
            .and_then(|e| crate::startup::set_enabled(&e, back)),
    };
    let what = if back { "back on" } else { "off" };
    match result {
        Ok(()) => format!("✔ {} {what}", i.name),
        Err(e) => format!("✖ {}: {e}", i.name),
    }
}

fn run_once(on: bool) -> Result<(), String> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_ONCE).map_err(|e| e.to_string())?;
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value(RUN_ONCE_VALUE, &format!("\"{}\" {}cleantest", exe.display(), crate::system::PAGE_ARG)).map_err(|e| e.to_string())
    } else {
        match key.delete_value(RUN_ONCE_VALUE) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_what_games_and_hardware_need() {
        assert!(keep_reason("BEService", "BattlEye Service", "BattlEye Innovations", true).is_some());
        assert!(keep_reason("vgc", "vgc", "Riot Games, Inc.", true).is_some());
        assert!(keep_reason("NVDisplay.ContainerLocalSystem", "NVIDIA Display Container LS", "NVIDIA Corporation", true).is_some());
        assert!(keep_reason("RtkAudioUniversalService", "Realtek Audio Universal Service", "Realtek Semiconductor", true).is_some());
        assert!(keep_reason("Steam Client Service", "Steam Client Service", "Valve Corporation", true).is_some());
        assert!(keep_reason("Kestrel", "", "", false).is_some());
        assert!(keep_reason("XboxService", "XBOX Input Service", "Windows (R) Win 7 DDK provider", true).is_some());
        assert!(keep_reason("GameInputRedistService", "GameInput Redist Service", "", true).is_some());
        // A launcher may stop starting with Windows; its service stays.
        assert!(keep_reason("Steam", "steam.exe -silent", "Valve Corporation", false).is_none());
        // Ordinary background programs go off.
        assert!(keep_reason("TeamViewer", "TeamViewer", "TeamViewer Germany GmbH", true).is_none());
        assert!(keep_reason("Spotify", "Spotify.exe", "Spotify AB", false).is_none());
    }

    #[test]
    fn record_lines_round_trip() {
        let items = [
            Item { name: "TeamViewer".into(), detail: String::new(), kind: Kind::Service(Start::Delayed) },
            Item {
                name: "Spotify".into(),
                detail: String::new(),
                kind: Kind::Startup("Registry · current user".into(), r#""C:\x\Spotify.exe" /minimized"#.into()),
            },
        ];
        for i in &items {
            assert_eq!(from_line(&to_line(i)).as_ref(), Some(i));
        }
        assert_eq!(from_line("garbage"), None);
    }

    #[test]
    #[ignore = "reads this PC's services and startup apps (read-only)"]
    fn live_plan() {
        let p = plan();
        for i in &p.services {
            println!("OFF service  {} | {}", i.name, i.detail);
        }
        for i in &p.startup {
            println!("OFF startup  {} | {}", i.name, i.detail);
        }
        for (n, why) in &p.kept {
            println!("KEEP {n} | {why}");
        }
    }
}
