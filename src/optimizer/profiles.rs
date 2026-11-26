//! Optimization profiles: Balanced, Competitive and Extreme, across Windows settings, services, scheduled tasks and
//! (Extreme only) app removal, Recall and security trade-offs. Each profile includes the ones below it. Items are
//! checked against this PC first (already done, not available, battery, anti-cheat), security-sensitive and removal
//! items start unticked, and everything applied is recorded so "Undo profile" can put it back.

use crate::optimizer::Tweak;
use crate::services::{self, Group};
use crate::uninstaller::{AppSource, InstalledApp};
use crate::utils::{backup, data_dir, log};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub enum Profile {
    #[default]
    Balanced,
    Competitive,
    Extreme,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::Balanced, Profile::Competitive, Profile::Extreme];

    pub fn label(self) -> &'static str {
        match self {
            Profile::Balanced => "Balanced",
            Profile::Competitive => "Competitive",
            Profile::Extreme => "Extreme",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            Profile::Balanced => "Keeps every Windows feature, security and updates. Turns off telemetry and background work nobody uses.",
            Profile::Competitive => "For a PC used mainly for games. More services and diagnostic tasks off; anti-cheat, controllers and launchers keep working.",
            Profile::Extreme => "Closest to custom gaming builds. Adds options that cost Windows features or security; each needs your tick.",
        }
    }
}

#[derive(Clone)]
pub enum Kind {
    Tweak(Tweak),
    /// A row of the Services list (one or more services).
    Service(services::Candidate),
    /// Scheduled tasks, by full path.
    Tasks(Vec<&'static str>),
    App(InstalledApp),
    Recall,
    /// Shown for information; nothing to apply.
    Info,
}

#[derive(Clone)]
pub struct Item {
    pub kind: Kind,
    pub title: String,
    pub detail: String,
    /// Already in place on this PC.
    pub done: bool,
    /// Why it isn't offered on this PC.
    pub blocked: Option<String>,
    /// Costs security or removes something: unticked until the user ticks it.
    pub sensitive: bool,
    pub profile: Profile,
}

impl Item {
    pub fn id(&self) -> String {
        format!("{:?}:{}", self.profile, self.title)
    }

    pub fn offered(&self) -> bool {
        !self.done && self.blocked.is_none() && !matches!(self.kind, Kind::Info)
    }
}

/// What on this PC changes the plan.
#[derive(Clone, Default)]
pub struct Hardware {
    pub battery: bool,
    /// Installed anti-cheat that requires virtualization-based security, by name.
    pub anticheat: Vec<&'static str>,
}

/// Anti-cheat whose current requirements include VBS or memory integrity: (service name, product).
const VBS_ANTICHEAT: &[(&str, &str)] = &[("FACEIT", "FACEIT"), ("vgc", "Riot Vanguard"), ("vgk", "Riot Vanguard")];

pub fn hardware() -> Hardware {
    let root = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
    let mut anticheat: Vec<&'static str> = VBS_ANTICHEAT
        .iter()
        .filter(|(svc, _)| root.open_subkey(format!(r"SYSTEM\CurrentControlSet\Services\{svc}")).is_ok())
        .map(|(_, name)| *name)
        .collect();
    anticheat.dedup();
    Hardware { battery: super::hardware::has_battery(), anticheat }
}

/// Telemetry and diagnostic tasks, by the lowest profile that turns them off. Balanced is AtlasOS's own list;
/// Competitive adds telemetry and diagnostics from xOS's list. Update, backup, disk-integrity and memory-diagnostic
/// tasks are left alone on purpose. Windows AI tasks (Recall, Click to Do) aren't listed: Windows lets only itself
/// change them, even for administrators (xOS runs as TrustedInstaller to get around that); removing Recall covers it.
const TASKS: &[(Profile, &str)] = &[
    (Profile::Balanced, r"\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector"),
    (Profile::Balanced, r"\Microsoft\Windows\Customer Experience Improvement Program\Consolidator"),
    (Profile::Balanced, r"\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip"),
    (Profile::Balanced, r"\Microsoft\Windows\Flighting\FeatureConfig\UsageDataReporting"),
    (Profile::Balanced, r"\Microsoft\Windows\Autochk\Proxy"),
    (Profile::Competitive, r"\Microsoft\Windows\Customer Experience Improvement Program\KernelCeipTask"),
    (Profile::Competitive, r"\Microsoft\Windows\Customer Experience Improvement Program\BthSQM"),
    (Profile::Competitive, r"\Microsoft\Windows\Customer Experience Improvement Program\Uploader"),
    (Profile::Competitive, r"\Microsoft\Windows\Application Experience\ProgramDataUpdater"),
    (Profile::Competitive, r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser"),
    (Profile::Competitive, r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser Exp"),
    (Profile::Competitive, r"\Microsoft\Windows\Application Experience\MareBackup"),
    (Profile::Competitive, r"\Microsoft\Windows\Feedback\Siuf\DmClient"),
    (Profile::Competitive, r"\Microsoft\Windows\Feedback\Siuf\DmClientOnScenarioDownload"),
    (Profile::Competitive, r"\Microsoft\Windows\Power Efficiency Diagnostics\AnalyzeSystem"),
    (Profile::Competitive, r"\Microsoft\Windows\Maintenance\WinSAT"),
    (Profile::Competitive, r"\Microsoft\Windows\DiskFootprint\Diagnostics"),
    (Profile::Competitive, r"\Microsoft\Windows\Maps\MapsToastTask"),
    (Profile::Competitive, r"\Microsoft\Windows\Sustainability\SustainabilityTelemetry"),
    (Profile::Competitive, r"\Microsoft\Windows\Diagnosis\RecommendedTroubleshootingScanner"),
    (Profile::Competitive, r"\Microsoft\Windows\Windows Error Reporting\QueueReporting"),
];

/// Store apps that keep background processes running: (package name prefix, name).
const EXTREME_APPS: &[(&str, &str)] = &[
    ("microsoftwindows.client.webexperience", "Widgets"),
    ("microsoft.yourphone", "Phone Link"),
    ("microsoft.copilot", "Copilot"),
    ("msteams", "Microsoft Teams"),
];

fn tweaks(p: Profile) -> &'static [Tweak] {
    match p {
        Profile::Balanced => &[Tweak::GameMode, Tweak::GameDvrOff, Tweak::WindowedGames],
        Profile::Competitive => &[
            Tweak::PowerMode,
            Tweak::GpuScheduling,
            Tweak::DeliveryUploadOff,
            Tweak::ServiceGrouping,
            Tweak::NvShaderCache,
            Tweak::UsbSuspendOff,
        ],
        Profile::Extreme => {
            &[Tweak::NoAnimations, Tweak::NoTransparency, Tweak::HighPerfPower, Tweak::MemoryIntegrityOff, Tweak::UpdatesOff]
        }
    }
}

fn service_profile(g: Group) -> Profile {
    match g {
        Group::Recommended => Profile::Balanced,
        Group::Optional | Group::Apps => Profile::Competitive,
        Group::Extreme => Profile::Extreme,
    }
}

/// Why a tweak is left out on this PC, if it is.
fn tweak_blocked(t: Tweak, hw: &Hardware, build: u32) -> Option<String> {
    if let Some(why) = t.unsupported(build) {
        return Some(why.into());
    }
    // The NVIDIA gaming profile keeps the graphics card at full clocks (Prefer maximum performance).
    if hw.battery && matches!(t, Tweak::UsbSuspendOff | Tweak::HighPerfPower | Tweak::NvShaderCache) {
        return Some("Left out on a PC with a battery: it costs battery life.".into());
    }
    if t == Tweak::MemoryIntegrityOff && !hw.anticheat.is_empty() {
        return Some(format!("Kept on: {} is installed and requires it.", hw.anticheat.join(" and ")));
    }
    t.not_needed()
}

/// Everything the profile includes (its own items and those of the profiles below it). Reads the PC; run it off
/// the UI thread. `admin` decides whether Recall's state can be read.
pub fn plan(p: Profile, build: u32, admin: bool) -> (Hardware, Vec<Item>) {
    let hw = hardware();
    let mut items = Vec::new();
    for level in Profile::ALL.into_iter().filter(|l| *l <= p) {
        for &t in tweaks(level) {
            let info = t.info();
            items.push(Item {
                kind: Kind::Tweak(t),
                title: info.title.into(),
                detail: format!("{} Trade-off: {}", info.purpose, info.drawback),
                done: t.is_applied(),
                blocked: tweak_blocked(t, &hw, build),
                sensitive: matches!(t, Tweak::MemoryIntegrityOff | Tweak::UpdatesOff),
                profile: level,
            });
        }
    }
    for c in services::candidates() {
        let level = service_profile(c.group);
        if level <= p {
            items.push(Item {
                title: format!("Service: {}", c.title),
                detail: format!("You lose: {}", c.lose),
                done: c.applied(),
                blocked: None,
                sensitive: c.group == Group::Extreme,
                profile: level,
                kind: Kind::Service(c),
            });
        }
    }
    for level in Profile::ALL.into_iter().filter(|l| *l <= p) {
        let present: Vec<(&'static str, bool)> = TASKS
            .iter()
            .filter(|(l, _)| *l == level)
            .filter_map(|(_, path)| crate::startup::task_enabled(path).ok().map(|on| (*path, on)))
            .collect();
        if present.is_empty() {
            continue;
        }
        let title = "Telemetry and diagnostics scheduled tasks";
        items.push(Item {
            title: format!("{title} ({})", present.len()),
            detail: "Microsoft's own tasks that collect usage and diagnostic data. Turned off, not deleted.".into(),
            done: present.iter().all(|(_, on)| !on),
            blocked: None,
            sensitive: false,
            profile: level,
            kind: Kind::Tasks(present.into_iter().map(|(path, _)| path).collect()),
        });
    }
    if p == Profile::Extreme {
        if let Some(state) = recall_state(admin) {
            items.push(Item {
                kind: Kind::Recall,
                title: "Remove Recall".into(),
                detail: "Turns off the Recall Windows feature (Settings > System > Optional features). Turn it back on there or with Undo profile.".into(),
                done: state == RecallState::Off,
                blocked: None,
                sensitive: true,
                profile: Profile::Extreme,
            });
        }
        let apps = crate::uninstaller::list().0;
        for (prefix, name) in EXTREME_APPS {
            let Some(app) =
                apps.iter().find(|a| matches!(&a.source, AppSource::Store { full_name } if full_name.to_lowercase().starts_with(prefix)))
            else {
                continue;
            };
            items.push(Item {
                kind: Kind::App(app.clone()),
                title: format!("Remove {name}"),
                detail: format!("Removes the app and the background process it keeps running. Reinstall {name} from the Microsoft Store if you want it back; Undo profile can't."),
                done: false,
                blocked: app.blocked().map(String::from),
                sensitive: true,
                profile: Profile::Extreme,
            });
        }
        items.push(Item {
            kind: Kind::Info,
            title: "Microsoft Defender stays on".into(),
            detail: "Windows only lets you turn Defender off for good by installing another antivirus, which then takes over. KernelOS and xOS force it off with TrustedInstaller workarounds that Microsoft doesn't support; Kestrel doesn't use them.".into(),
            done: false,
            blocked: Some("Not offered".into()),
            sensitive: false,
            profile: Profile::Extreme,
        });
    }
    (hw, items)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RecallState {
    On,
    Off,
}

/// None when this Windows has no Recall feature, or without administrator rights (DISM needs them).
fn recall_state(admin: bool) -> Option<RecallState> {
    if !admin {
        return None;
    }
    let out = crate::system::powershell("(Get-WindowsOptionalFeature -Online -FeatureName Recall).State").ok()?;
    match out.trim() {
        "Enabled" | "EnablePending" => Some(RecallState::On),
        "Disabled" | "DisablePending" | "DisabledWithPayloadRemoved" => Some(RecallState::Off),
        _ => None,
    }
}

fn record_path() -> std::path::PathBuf {
    data_dir().join("profile-applied.txt")
}

pub fn has_record() -> bool {
    std::fs::read_to_string(record_path()).is_ok_and(|t| !t.trim().is_empty())
}

fn record(line: &str) {
    let mut text = std::fs::read_to_string(record_path()).unwrap_or_default();
    if !text.lines().any(|l| l == line) {
        text.push_str(line);
        text.push('\n');
        let _ = crate::utils::write_atomic(&record_path(), text.as_bytes());
    }
}

fn task_key(path: &str) -> String {
    format!("task:{path}")
}

/// Applies the ticked items, in a safe order (settings, services, tasks, Recall, then removals). One line per item.
pub fn apply(items: &[Item]) -> Vec<String> {
    let mut lines = Vec::new();
    for item in items.iter().filter(|i| i.offered()) {
        match &item.kind {
            Kind::Tweak(t) => match t.apply() {
                Ok(()) => {
                    record(&format!("tweak\t{t:?}"));
                    lines.push(format!("✔ {}", item.title));
                }
                Err(e) => lines.push(format!("✖ {}: {e}", item.title)),
            },
            Kind::Service(c) => {
                c.services.iter().for_each(|(name, _)| record(&format!("service\t{name}")));
                lines.extend(services::apply(c));
            }
            Kind::Tasks(paths) => {
                let mut failed = Vec::new();
                for path in paths {
                    let saved_before = backup::has(&task_key(path));
                    let result = backup::save_original(&task_key(path), || {
                        crate::startup::task_enabled(path).map(|on| if on { "1" } else { "0" }.to_string())
                    })
                    .and_then(|()| crate::startup::set_task_enabled(path, false));
                    match result {
                        Ok(()) => record(&format!("task\t{path}")),
                        Err(e) => {
                            // Nothing changed, so the original saved a moment ago must not linger for Restore everything.
                            if !saved_before {
                                let _ = backup::remove(&task_key(path));
                            }
                            failed.push(format!("{path}: {e}"));
                        }
                    }
                }
                lines.push(if failed.is_empty() {
                    format!("✔ {}", item.title)
                } else {
                    format!("✖ {}: {}", item.title, failed.join("; "))
                });
            }
            Kind::Recall => {
                match crate::system::powershell("Disable-WindowsOptionalFeature -Online -FeatureName Recall -NoRestart | Out-Null") {
                    Ok(_) => {
                        record("recall");
                        lines.push("✔ Recall removed (finishes after a restart)".into());
                    }
                    Err(e) => lines.push(format!("✖ Recall: {e}")),
                }
            }
            Kind::App(app) => {
                let stop = std::sync::atomic::AtomicBool::new(false);
                lines.push(match crate::uninstaller::uninstall(app, &stop) {
                    Ok(m) => format!("✔ {m}"),
                    Err(e) => format!("✖ {}: {e}", app.name),
                });
            }
            Kind::Info => {}
        }
    }
    log(&format!("Profile applied: {} items", lines.len()));
    lines
}

/// Puts back everything a profile changed, newest first. Removed apps are listed to reinstall from the Store.
pub fn undo() -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(record_path()) else { return Vec::new() };
    let mut lines = Vec::new();
    let mut left = Vec::new();
    for line in text.lines().rev() {
        let result = match line.split_once('\t') {
            Some(("tweak", name)) => match Tweak::ALL.into_iter().find(|t| format!("{t:?}") == name) {
                Some(t) => t.restore().map(|_| format!("✔ {} restored", t.info().title)),
                None => Ok(String::new()),
            },
            Some(("service", name)) => services::revert(name).map(|()| format!("✔ Service {name} restored")),
            Some(("task", path)) => match backup::get(&task_key(path)) {
                Some(original) => crate::startup::put_task_back(path, original == "1")
                    .and_then(|()| backup::remove(&task_key(path)))
                    .map(|()| format!("✔ Scheduled task restored: {path}")),
                None => Ok(String::new()),
            },
            None if line == "recall" => {
                crate::system::powershell("Enable-WindowsOptionalFeature -Online -FeatureName Recall -NoRestart | Out-Null")
                    .map(|_| "✔ Recall turned back on (finishes after a restart)".to_string())
            }
            _ => Ok(String::new()),
        };
        match result {
            Ok(l) if !l.is_empty() => lines.push(l),
            Ok(_) => {}
            Err(e) => {
                lines.push(format!("✖ {line}: {e}"));
                left.push(line);
            }
        }
    }
    // Keep what couldn't be undone, so a second try (for example as administrator) can finish it.
    let rest: String = left.iter().rev().map(|l| format!("{l}\n")).collect();
    let _ = if rest.is_empty() { std::fs::remove_file(record_path()) } else { crate::utils::write_atomic(&record_path(), rest.as_bytes()) };
    log(&format!("Profile undone: {} items", lines.len()));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads this PC (read-only)"]
    fn live_plan() {
        let (hw, items) = plan(Profile::Extreme, crate::system::windows_build(), false);
        println!("battery={} anticheat={:?}", hw.battery, hw.anticheat);
        for i in items {
            println!("{:?} | {} | done={} blocked={:?} sensitive={}", i.profile, i.title, i.done, i.blocked, i.sensitive);
        }
    }

    #[test]
    fn profiles_build_on_each_other() {
        for t in tweaks(Profile::Balanced) {
            assert!(!tweaks(Profile::Competitive).contains(t) && !tweaks(Profile::Extreme).contains(t));
        }
        assert!(Profile::Balanced < Profile::Competitive && Profile::Competitive < Profile::Extreme);
        // Security trade-offs are Extreme only.
        for t in [Tweak::MemoryIntegrityOff, Tweak::UpdatesOff] {
            assert!(tweaks(Profile::Extreme).contains(&t));
            assert!(!tweaks(Profile::Balanced).contains(&t) && !tweaks(Profile::Competitive).contains(&t));
        }
    }

    #[test]
    fn battery_and_anticheat_hold_items_back() {
        let laptop = Hardware { battery: true, anticheat: vec![] };
        assert!(tweak_blocked(Tweak::UsbSuspendOff, &laptop, 26200).is_some());
        assert!(tweak_blocked(Tweak::HighPerfPower, &laptop, 26200).is_some());
        assert!(tweak_blocked(Tweak::NvShaderCache, &laptop, 26200).is_some());
        let faceit = Hardware { battery: false, anticheat: vec!["FACEIT"] };
        assert!(tweak_blocked(Tweak::MemoryIntegrityOff, &faceit, 26200).unwrap().contains("FACEIT"));
    }

    #[test]
    fn task_list_skips_updates_and_integrity_tasks() {
        for (_, path) in TASKS {
            let p = path.to_lowercase();
            assert!(p.starts_with(r"\microsoft\windows\"), "{path}");
            for keep in ["updateorchestrator", "windowsupdate", "data integrity", "memorydiagnostic", "regidlebackup", "windows defender"] {
                assert!(!p.contains(keep), "{path} must stay on");
            }
        }
    }
}
