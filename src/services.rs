//! Windows > Services: a fixed list of services that a gaming PC doesn't need, researched against Microsoft's own
//! service guidance. Nothing outside this list can be changed, so services that Windows, anti-cheat, launchers,
//! Xbox, audio or Bluetooth need are never touched. Each original start type is saved first (shared backup file),
//! so Revert and Settings > Restore everything put it back.

use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use crate::utils::backup;

const ROOT: &str = r"SYSTEM\CurrentControlSet\Services";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Start {
    Auto,
    Delayed,
    Manual,
    Disabled,
}

impl Start {
    const ALL: [Start; 4] = [Start::Auto, Start::Delayed, Start::Manual, Start::Disabled];

    pub fn label(self) -> &'static str {
        match self {
            Start::Auto => "Automatic",
            Start::Delayed => "Automatic (delayed)",
            Start::Manual => "Manual",
            Start::Disabled => "Disabled",
        }
    }

    /// How it's written to the backup file.
    fn code(self) -> &'static str {
        match self {
            Start::Auto => "2",
            Start::Delayed => "2d",
            Start::Manual => "3",
            Start::Disabled => "4",
        }
    }

    fn from_code(s: &str) -> Option<Start> {
        Start::ALL.into_iter().find(|t| t.code() == s)
    }

    /// From the registry's Start and DelayedAutostart values. Boot and system starts (drivers) aren't listed.
    fn from_reg(start: u32, delayed: u32) -> Option<Start> {
        match (start, delayed) {
            (2, 1) => Some(Start::Delayed),
            (2, _) => Some(Start::Auto),
            (3, _) => Some(Start::Manual),
            (4, _) => Some(Start::Disabled),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    /// Preselected: nothing a gaming PC uses depends on these.
    Recommended,
    /// A trade-off worth measuring first.
    Optional,
    /// Updaters and remote-access tools installed by other apps: set to Manual, never Disabled.
    Apps,
}

#[derive(Clone, Copy)]
enum When {
    Always,
    NoPrinter,
    NotInsider,
}

struct Def {
    /// Service key names; a trailing '*' matches versioned names (GoogleUpdaterService131.0…).
    names: &'static [&'static str],
    title: &'static str,
    lose: &'static str,
    group: Group,
    to: Start,
    when: When,
}

const fn def(names: &'static [&'static str], title: &'static str, lose: &'static str, group: Group, to: Start) -> Def {
    Def { names, title, lose, group, to, when: When::Always }
}

use Group::{Apps, Optional, Recommended};
use Start::{Disabled, Manual};

/// Microsoft lists these as "OK to disable" (or they are app updaters). Kept off on purpose for gaming PCs: Xbox
/// services (Game Pass), Bluetooth (controllers), audio, camera (streaming), SSDP/UPnP (multiplayer), Windows
/// Search, Error Reporting, Themes, Update, and every anti-cheat, launcher and driver service.
const DEFS: &[Def] = &[
    def(
        &["DiagTrack"],
        "Telemetry (Connected User Experiences)",
        "Nothing you use; diagnostic data stops being sent.",
        Recommended,
        Disabled,
    ),
    def(
        &["dmwappushservice"],
        "Device management messages (WAP Push)",
        "Nothing on a home PC; it's for company device management.",
        Recommended,
        Disabled,
    ),
    def(&["MapsBroker"], "Downloaded Maps Manager", "Offline maps in the Maps app.", Recommended, Disabled),
    def(&["lfsvc"], "Geolocation", "Location for weather, maps and Find my device.", Recommended, Disabled),
    def(&["PhoneSvc"], "Phone Service", "Calls through Phone Link.", Recommended, Disabled),
    def(&["RetailDemo"], "Retail Demo", "Nothing; it's the demo mode for PCs in shops.", Recommended, Disabled),
    def(&["Fax"], "Fax", "Faxing.", Recommended, Disabled),
    def(
        &["TrkWks"],
        "Distributed Link Tracking",
        "Nothing on a home PC; it fixes shortcuts to files moved between network drives.",
        Recommended,
        Disabled,
    ),
    def(&["icssvc"], "Mobile Hotspot", "Sharing this PC's connection as a Wi-Fi hotspot.", Recommended, Disabled),
    def(&["WalletService", "SEMgrSvc"], "Payments and NFC", "Tap-to-pay, which desktop PCs don't have.", Recommended, Disabled),
    Def {
        when: When::NotInsider,
        ..def(&["wisvc"], "Windows Insider Service", "Joining the Windows Insider Program.", Recommended, Disabled)
    },
    Def {
        when: When::NoPrinter,
        ..def(
            &["Spooler", "PrintNotify"],
            "Printing (no printer installed)",
            "Printing, including Print to PDF. Microsoft advises this on PCs that don't print.",
            Recommended,
            Disabled,
        )
    },
    def(
        &["SysMain"],
        "SysMain (preloads apps)",
        "Apps may start a little slower, mainly from hard drives. Some players see fewer hitches: measure a game before and after.",
        Optional,
        Disabled,
    ),
    def(&["edgeupdate", "edgeupdatem"], "Microsoft Edge Update", "Nothing; Edge still updates on its own schedule.", Apps, Manual),
    def(
        &["GoogleUpdaterService*", "GoogleUpdaterInternalService*", "gupdate", "gupdatem"],
        "Google Update",
        "Nothing; Google apps still update on their own schedule.",
        Apps,
        Manual,
    ),
    def(&["brave", "bravem"], "Brave Update", "Nothing; Brave still updates on its own schedule.", Apps, Manual),
    def(&["AdobeARMservice"], "Adobe Acrobat Update", "Nothing; Acrobat still updates when you open it.", Apps, Manual),
    def(&["TeamViewer"], "TeamViewer", "Incoming remote connections until you open TeamViewer.", Apps, Manual),
    def(&["AnyDesk"], "AnyDesk", "Incoming remote connections until you open AnyDesk.", Apps, Manual),
];

/// One row on the Services tab: one or more services that go together.
#[derive(Clone)]
pub struct Candidate {
    pub title: &'static str,
    pub lose: &'static str,
    pub group: Group,
    pub to: Start,
    /// (service key name, current start type).
    pub services: Vec<(String, Start)>,
    pub running: bool,
    /// Kestrel saved an original for at least one of them.
    pub saved: bool,
}

impl Candidate {
    /// Every service is at the target (or further off).
    pub fn applied(&self) -> bool {
        self.services.iter().all(|(_, s)| *s == self.to || *s == Start::Disabled)
    }

    /// Kestrel changed it, and Windows or the app put it back.
    pub fn changed_back(&self) -> bool {
        self.saved && !self.applied()
    }

    /// Stable id for selections.
    pub fn id(&self) -> &'static str {
        self.title
    }
}

/// The listed services that exist on this PC and apply to it.
pub fn candidates() -> Vec<Candidate> {
    let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(ROOT) else { return Vec::new() };
    let keys: Vec<String> = root.enum_keys().flatten().collect();
    let saved = backup::all();
    let (printer, insider) = (has_printer(), is_insider());
    DEFS.iter()
        .filter(|d| match d.when {
            When::Always => true,
            When::NoPrinter => !printer,
            When::NotInsider => !insider,
        })
        .filter_map(|d| {
            let services: Vec<(String, Start)> = keys
                .iter()
                .filter(|k| d.names.iter().any(|n| matches_name(n, k)))
                .filter_map(|k| {
                    let key = root.open_subkey(k).ok()?;
                    let start = Start::from_reg(key.get_value("Start").ok()?, key.get_value("DelayedAutostart").unwrap_or(0))?;
                    Some((k.clone(), start))
                })
                .collect();
            if services.is_empty() {
                return None;
            }
            Some(Candidate {
                title: d.title,
                lose: d.lose,
                group: d.group,
                to: d.to,
                running: services.iter().any(|(n, _)| crate::system::service_state(n)),
                saved: services.iter().any(|(n, _)| saved.contains_key(&key(n))),
                services,
            })
        })
        .collect()
}

fn matches_name(pattern: &str, key: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => key.len() >= prefix.len() && key[..prefix.len()].eq_ignore_ascii_case(prefix),
        None => key.eq_ignore_ascii_case(pattern),
    }
}

/// Whether any printer other than Windows' built-in virtual ones is installed.
fn has_printer() -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SYSTEM\CurrentControlSet\Control\Print\Printers")
        .map(|k| real_printers(k.enum_keys().flatten()))
        .unwrap_or(false)
}

fn real_printers(names: impl IntoIterator<Item = String>) -> bool {
    names.into_iter().any(|n| {
        let n = n.to_lowercase();
        !(n.starts_with("microsoft print to pdf") || n.starts_with("microsoft xps document writer") || n.contains("onenote") || n == "fax")
    })
}

fn is_insider() -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\WindowsSelfHost\Applicability").is_ok_and(|k| {
        k.get_value::<String, _>("BranchName").is_ok_and(|b| !b.is_empty())
            || k.get_value::<u32, _>("EnableExperimentation").is_ok_and(|v| v != 0)
    })
}

fn key(name: &str) -> String {
    format!("service:{}", name.to_lowercase())
}

/// Applies a row: saves each original, sets the target start type and stops the service so it takes effect now.
pub fn apply(c: &Candidate) -> Vec<String> {
    c.services
        .iter()
        .filter(|(_, s)| *s != c.to && *s != Start::Disabled)
        .map(|(name, current)| {
            let result = backup::save_original(&key(name), || Ok(current.code().to_string())).and_then(|()| write(name, c.to));
            match result {
                Ok(()) => {
                    control(name, false);
                    crate::utils::log(&format!("Service {name}: {} -> {}", current.label(), c.to.label()));
                    format!("✔ {}: {name} set to {}", c.title, c.to.label())
                }
                Err(e) => format!("✖ {} ({name}): {e}", c.title),
            }
        })
        .collect()
}

/// Puts back the start type saved before Kestrel changed it, and starts the service again if it started with Windows.
pub fn revert(name: &str) -> Result<(), String> {
    let Some(original) = backup::get(&key(name)).and_then(|s| Start::from_code(&s)) else { return Ok(()) };
    write(name, original)?;
    if matches!(original, Start::Auto | Start::Delayed) {
        control(name, true);
    }
    crate::utils::log(&format!("Service {name}: start type restored to {}", original.label()));
    backup::remove(&key(name))
}

/// Restores every service Kestrel changed. One line per service.
pub fn restore_all() -> Vec<String> {
    backup::with_prefix("service:")
        .into_iter()
        .map(|(name, _)| match revert(&name) {
            Ok(()) => format!("✔ Restored service: {name}"),
            Err(e) => format!("✖ Service {name}: {e}"),
        })
        .collect()
}

/// Starts or stops a service. Best effort: the start type change already takes effect at the next restart.
fn control(name: &str, start: bool) {
    use windows::core::PCWSTR;
    use windows::Win32::System::Services::{
        CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, StartServiceW, SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP,
        SERVICE_START, SERVICE_STATUS, SERVICE_STOP,
    };
    let n = crate::system::wide(name);
    unsafe {
        let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else { return };
        if let Ok(svc) = OpenServiceW(scm, PCWSTR(n.as_ptr()), if start { SERVICE_START } else { SERVICE_STOP }) {
            let _ =
                if start { StartServiceW(svc, None) } else { ControlService(svc, SERVICE_CONTROL_STOP, &mut SERVICE_STATUS::default()) };
            let _ = CloseServiceHandle(svc);
        }
        let _ = CloseServiceHandle(scm);
    }
}

fn write(name: &str, to: Start) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Services::{
        ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW, ENUM_SERVICE_TYPE,
        SC_MANAGER_CONNECT, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG, SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
        SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DEMAND_START, SERVICE_DISABLED, SERVICE_ERROR, SERVICE_NO_CHANGE,
    };
    let start = match to {
        Start::Auto | Start::Delayed => SERVICE_AUTO_START,
        Start::Manual => SERVICE_DEMAND_START,
        Start::Disabled => SERVICE_DISABLED,
    };
    let n = crate::system::wide(name);
    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_CONNECT).map_err(|e| e.message())?;
        let result = (|| {
            let svc = OpenServiceW(scm, PCWSTR(n.as_ptr()), SERVICE_CHANGE_CONFIG).map_err(|e| e.message())?;
            let r = ChangeServiceConfigW(
                svc,
                ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
                start,
                SERVICE_ERROR(SERVICE_NO_CHANGE),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .and_then(|()| {
                if start != SERVICE_AUTO_START {
                    return Ok(());
                }
                let info = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: BOOL::from(to == Start::Delayed) };
                ChangeServiceConfig2W(svc, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, Some(&info as *const _ as *const _))
            })
            .map_err(|e| e.message());
            let _ = CloseServiceHandle(svc);
            r
        })();
        let _ = CloseServiceHandle(scm);
        result.map_err(|e| format!("Windows refused the change: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_codes_round_trip() {
        for s in Start::ALL {
            assert_eq!(Start::from_code(s.code()), Some(s));
        }
        assert_eq!(Start::from_reg(2, 1), Some(Start::Delayed));
        assert_eq!(Start::from_reg(2, 0), Some(Start::Auto));
        assert_eq!(Start::from_reg(1, 0), None);
    }

    /// Services that Windows, games, anti-cheat, controllers or sound need must never be on the list.
    #[test]
    fn never_lists_what_games_or_windows_need() {
        const DENY: &[&str] = &[
            "xblauthmanager",
            "xblgamesave",
            "xboxnetapisvc",
            "xboxgipsvc",
            "xboxservice",
            "gamingservices",
            "gamingservicesnet",
            "bthserv",
            "audiosrv",
            "audioendpointbuilder",
            "frameserver",
            "beservice",
            "easyanticheat",
            "easyanticheat_eos",
            "vgc",
            "vgk",
            "wuauserv",
            "usosvc",
            "bits",
            "wersvc",
            "themes",
            "ssdpsrv",
            "upnphost",
            "wsearch",
            "bfe",
            "mpssvc",
            "windefend",
            "dhcp",
            "dnscache",
            "nsi",
            "rpcss",
            "dcomlaunch",
            "steam client service",
            "nvdisplay.containerlocalsystem",
        ];
        for d in DEFS {
            for n in d.names {
                assert!(!DENY.iter().any(|x| matches_name(n, x)), "{n} must not be listed");
            }
            // Other apps' services are only ever set to start when needed, never disabled.
            assert!(d.group != Group::Apps || d.to == Start::Manual);
        }
    }

    #[test]
    fn versioned_names_match() {
        assert!(matches_name("GoogleUpdaterService*", "GoogleUpdaterService131.0.6738.0"));
        assert!(!matches_name("GoogleUpdaterService*", "GoogleUpdate"));
        assert!(matches_name("Spooler", "spooler"));
    }

    #[test]
    fn virtual_printers_dont_count() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!real_printers(names(&["Microsoft Print to PDF", "Microsoft XPS Document Writer", "OneNote (Desktop)", "Fax"])));
        assert!(real_printers(names(&["Microsoft Print to PDF", "HP LaserJet 1020"])));
    }
}
