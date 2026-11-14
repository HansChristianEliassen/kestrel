//! Installed desktop apps (Uninstall registry keys) and Microsoft Store apps (Windows' PackageManager).
//! Uninstalling always runs the app's own registered uninstaller, or PackageManager's removal for Store apps.

use std::collections::HashSet;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::ApplicationModel::PackageSignatureKind;
use windows::Management::Deployment::{PackageManager, PackageTypes};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

use crate::cleaner::scanner::REPARSE_POINT;
use crate::system::{com_init, env_path, is_admin, shell_execute, split_command};
use crate::utils::log;

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Clone)]
pub enum AppSource {
    Desktop { machine: bool, key: String, command: Option<String>, location: Option<String> },
    Store { full_name: String },
}

#[derive(Clone)]
pub struct InstalledApp {
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub size: Option<u64>,
    pub source: AppSource,
    pub optional: bool,
    pub protected: Option<&'static str>,
    /// Other software (editors, build scripts, command-line tools) may depend on it.
    pub developer: bool,
}

impl InstalledApp {
    /// Stable identity across list reloads.
    pub fn id(&self) -> String {
        match &self.source {
            AppSource::Desktop { key, machine, .. } => format!("{machine}|{key}"),
            AppSource::Store { full_name } => full_name.clone(),
        }
    }

    /// Why Kestrel won't uninstall this app, if it won't.
    pub fn blocked(&self) -> Option<&'static str> {
        let no_uninstaller = matches!(&self.source, AppSource::Desktop { command: None, .. });
        self.protected.or(no_uninstaller.then_some("No uninstaller registered. Use Windows Settings > Apps."))
    }
}

const DEV_TOOLS: &[&str] = &[
    "git",
    "python",
    "node.js",
    "java",
    "openjdk",
    "microsoft visual studio",
    "visual studio build tools",
    "cmake",
    "rustup",
    "docker desktop",
    "go programming language",
    "windows subsystem for linux",
    "powershell 7",
];

fn is_dev_tool(name: &str) -> bool {
    let n = name.to_lowercase();
    DEV_TOOLS.iter().any(|d| n == *d || n.starts_with(&format!("{d} ")))
}

const DRIVER: &str = "Hardware driver — manage it with Device Manager or the vendor's own tool.";
const ANTICHEAT: &str = "Anti-cheat — online games depend on it and reinstall it on launch.";
const RUNTIME: &str = "Shared runtime — other programs and games depend on it.";
const WINDOWS: &str = "Windows component — removing it can break Windows features.";
const SECURITY: &str = "Security software — remove it through its vendor if you really mean to.";

/// Lower-case substring > reason uninstall is blocked.
const PROTECTED: &[(&str, &str)] = &[
    ("driver", DRIVER),
    ("chipset", DRIVER),
    ("amd software", DRIVER),
    ("anti-cheat", ANTICHEAT),
    ("denuvo", ANTICHEAT),
    ("easyanticheat", ANTICHEAT),
    ("battleye", ANTICHEAT),
    ("riot vanguard", ANTICHEAT),
    ("faceit", ANTICHEAT),
    ("ea anticheat", ANTICHEAT),
    ("javelin", ANTICHEAT),
    ("punkbuster", ANTICHEAT),
    ("visual c++", RUNTIME),
    ("redistributable", RUNTIME),
    ("microsoft .net", RUNTIME),
    ("desktop runtime", RUNTIME),
    ("webview2", RUNTIME),
    ("directx", RUNTIME),
    ("vulkan", RUNTIME),
    ("physx", RUNTIME),
    ("microsoft.vclibs", RUNTIME),
    ("microsoft.ui.xaml", RUNTIME),
    ("microsoft.net.", RUNTIME),
    ("videoextension", RUNTIME),
    ("imageextension", RUNTIME),
    ("mediaextensions", RUNTIME),
    ("winappruntime", RUNTIME),
    ("windowsappruntime", RUNTIME),
    ("ink.handwriting", WINDOWS),
    ("winget.source", WINDOWS),
    ("applicationcompatibilityenhancements", WINDOWS),
    ("microsoft edge", WINDOWS),
    ("update health", WINDOWS),
    ("gameinput", WINDOWS),
    ("microsoft.windowsstore", WINDOWS),
    ("microsoft.storepurchaseapp", WINDOWS),
    ("microsoft.desktopappinstaller", WINDOWS),
    ("microsoft.gamingservices", WINDOWS),
    ("microsoft.xboxidentityprovider", WINDOWS),
    ("microsoft.sechealthui", SECURITY),
    ("defender", SECURITY),
];

/// Store packages Microsoft preinstalls that are safe to remove (reinstallable from the Store).
const OPTIONAL: &[&str] = &[
    "microsoft.bingnews",
    "microsoft.bingweather",
    "microsoft.bingsearch",
    "microsoft.gethelp",
    "microsoft.getstarted",
    "microsoft.microsoftsolitairecollection",
    "microsoft.zunemusic",
    "microsoft.zunevideo",
    "microsoft.people",
    "microsoft.windowsfeedbackhub",
    "clipchamp.clipchamp",
    "microsoft.microsoftofficehub",
    "microsoft.todos",
    "microsoft.windowsmaps",
    "microsoft.mixedreality.portal",
    "microsoft.skypeapp",
    "microsoft.549981c3f5f10",
    "microsoft.powerautomatedesktop",
    "microsoft.windowssoundrecorder",
    "microsoft.outlookforwindows",
    "microsoftcorporationii.quickassist",
    "microsoft.windows.devhome",
    "msteams",
    "microsoft.copilot",
    "microsoft.microsoft3dviewer",
    "microsoft.office.onenote",
    "microsoft.wallet",
    "spotifyab.spotifymusic",
    "disney.",
    "king.com.",
    "microsoftcorporationii.microsoftfamily",
];

fn protection(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    PROTECTED.iter().find(|(k, _)| t.contains(k)).map(|(_, why)| *why)
}

fn hive(machine: bool) -> RegKey {
    RegKey::predef(if machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER })
}

fn desktop_apps(out: &mut Vec<InstalledApp>) {
    let mut seen = HashSet::new();
    for (machine, root_path) in [(true, UNINSTALL), (true, UNINSTALL32), (false, UNINSTALL)] {
        let Ok(root) = hive(machine).open_subkey(root_path) else { continue };
        for sub in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&sub) else { continue };
            let Ok(name) = k.get_value::<String, _>("DisplayName") else { continue };
            let name = name.trim().to_string();
            // Hidden components, updates and patches are not standalone apps.
            if name.is_empty()
                || k.get_value::<u32, _>("SystemComponent").is_ok_and(|v| v == 1)
                || k.get_value::<String, _>("ParentKeyName").is_ok()
                || k.get_value::<String, _>("ReleaseType").is_ok_and(|t| t.contains("Update") || t == "Hotfix")
            {
                continue;
            }
            let version = k.get_value::<String, _>("DisplayVersion").unwrap_or_default();
            if !seen.insert((name.to_lowercase(), version.clone())) {
                continue;
            }
            let publisher = k.get_value::<String, _>("Publisher").unwrap_or_default();
            let size = k.get_value::<u32, _>("EstimatedSize").ok().filter(|&kb| kb > 0).map(|kb| kb as u64 * 1024);
            let command = k.get_value::<String, _>("UninstallString").ok().filter(|c| !c.trim().is_empty());
            let location = k
                .get_value::<String, _>("InstallLocation")
                .ok()
                .map(|l| l.trim().trim_matches('"').trim_end_matches('\\').to_string())
                .filter(|l| !l.is_empty());
            out.push(InstalledApp {
                protected: protection(&format!("{name} {publisher}")),
                optional: name == "Microsoft OneDrive",
                developer: is_dev_tool(&name),
                name,
                publisher,
                version,
                size,
                source: AppSource::Desktop { machine, key: format!(r"{root_path}\{sub}"), command, location },
            });
        }
    }
}

/// Total size of a folder, without following links.
fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .filter_map(|e| e.metadata().ok().map(|m| (e.path(), m)))
        .filter(|(_, m)| m.file_attributes() & REPARSE_POINT == 0)
        .map(|(p, m)| if m.is_dir() { dir_size(&p) } else { m.len() })
        .sum()
}

/// Store apps for the current user, through Windows' own package API (no admin needed).
fn store_apps(out: &mut Vec<InstalledApp>) -> Result<(), String> {
    com_init();
    let pm = PackageManager::new().map_err(|e| e.message())?;
    let packages = pm.FindPackagesByUserSecurityIdWithPackageTypes(&HSTRING::new(), PackageTypes::Main).map_err(|e| e.message())?;
    for p in packages {
        let skip = p.IsFramework().unwrap_or(true)
            || p.IsResourcePackage().unwrap_or(true)
            || p.SignatureKind().map_or(true, |k| k == PackageSignatureKind::System);
        let Ok(id) = p.Id() else { continue };
        let (Ok(id_name), Ok(full_name)) = (id.Name(), id.FullName()) else { continue };
        if skip {
            continue;
        }
        let id_name = id_name.to_string();
        let name = p.DisplayName().map(|n| n.to_string()).ok().filter(|n| !n.is_empty() && !n.starts_with("ms-resource:"));
        let version = id.Version().map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision)).unwrap_or_default();
        let size = p.InstalledPath().ok().map(|d| dir_size(Path::new(&d.to_string()))).filter(|&s| s > 0);
        let id_lower = id_name.to_lowercase();
        let name = name.unwrap_or_else(|| id_name.clone());
        out.push(InstalledApp {
            developer: is_dev_tool(&name),
            name,
            publisher: p.PublisherDisplayName().map(|s| s.to_string()).unwrap_or_default(),
            version,
            size,
            source: AppSource::Store { full_name: full_name.to_string() },
            optional: OPTIONAL.iter().any(|o| id_lower.starts_with(o)),
            protected: protection(&id_name),
        });
    }
    Ok(())
}

fn remove_store_app(full_name: &str) -> Result<(), String> {
    com_init();
    let pm = PackageManager::new().map_err(|e| e.message())?;
    let result = pm.RemovePackageAsync(&HSTRING::from(full_name)).and_then(|op| op.get()).map_err(|e| e.message())?;
    match result.ExtendedErrorCode() {
        Ok(code) if code.is_err() => Err(result.ErrorText().map(|t| t.to_string()).unwrap_or_else(|_| code.message())),
        _ => Ok(()),
    }
}

/// All apps sorted by name, plus a note if Store apps could not be listed.
pub fn list() -> (Vec<InstalledApp>, Option<String>) {
    let mut apps = Vec::new();
    desktop_apps(&mut apps);
    let note = store_apps(&mut apps).err().map(|e| format!("Microsoft Store apps could not be listed: {e}"));
    apps.sort_by_key(|a| a.name.to_lowercase());
    (apps, note)
}

/// Windows Installer entries often register "/I" (modify); "/X" is the documented uninstall switch.
fn msi_uninstall(cmd: &str) -> String {
    if cmd.to_ascii_lowercase().contains("msiexec") {
        cmd.replace("/I{", "/X{").replace("/i{", "/X{")
    } else {
        cmd.to_string()
    }
}

/// Runs the official uninstaller and reports what happened.
/// Many uninstallers (NSIS, Inno Setup) relaunch themselves and exit at once, so after the process ends
/// this keeps checking for the app's registry entry until it disappears, `stop` is set, or 30 minutes pass.
pub fn uninstall(app: &InstalledApp, stop: &AtomicBool) -> Result<String, String> {
    let result = match &app.source {
        AppSource::Store { full_name } => remove_store_app(full_name).map(|()| format!("{} was removed.", app.name)),
        AppSource::Desktop { machine, key, command, .. } => {
            let cmd = command.as_deref().ok_or("This app has no registered uninstaller. Use Windows Settings > Apps.")?;
            // Any program running as this user can change a per-user uninstall command, so never run one with admin rights.
            if !machine && is_admin() {
                return Err(format!(
                    "{} is installed for your account only, and Kestrel is running as administrator. \
                     Close Kestrel, open it normally (not as administrator) and uninstall it from there.",
                    app.name
                ));
            }
            let (exe, args) = split_command(&msi_uninstall(cmd));
            let code = shell_execute(&exe, &args, false, Some(stop))?;
            let gone = || hive(*machine).open_subkey(key).is_err();
            let deadline = Instant::now() + Duration::from_secs(30 * 60);
            while !gone() && !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_secs(2));
            }
            if gone() {
                Ok(format!("{} was uninstalled.", app.name))
            } else {
                Ok(format!(
                    "The uninstaller closed (exit code {code}) but {} is still installed. \
                     If you cancelled it, nothing was removed; otherwise finish it and press Refresh.",
                    app.name
                ))
            }
        }
    };
    log(&format!(
        "Uninstall '{}' ({}): {}",
        app.name,
        app.publisher,
        match &result {
            Ok(m) => m.clone(),
            Err(e) => format!("failed — {e}"),
        }
    ));
    result
}

/// "Foo 2.1.0 (x64)" -> "Foo": folders are usually named without the version.
fn base_name(name: &str) -> String {
    let mut words = name.split_whitespace();
    // The first word always stays ("7-Zip").
    let first = words.next().unwrap_or_default();
    let rest = words.take_while(|w| !w.starts_with(|c: char| c.is_ascii_digit() || c == '('));
    std::iter::once(first).chain(rest).collect::<Vec<_>>().join(" ")
}

/// Folders an uninstalled desktop app may have left behind: its install folder, and folders named after it in
/// AppData and ProgramData. Only reported, never deleted; empty while the app is still installed.
pub fn leftovers(app: &InstalledApp) -> Vec<(PathBuf, u64)> {
    let AppSource::Desktop { machine, key, location, .. } = &app.source else { return vec![] };
    if hive(*machine).open_subkey(key).is_ok() {
        return vec![];
    }
    let roots = [env_path("APPDATA"), env_path("LOCALAPPDATA"), env_path("ProgramData")];
    // Shared folders a bad InstallLocation could point at; never suggest these.
    let mut shared: Vec<PathBuf> =
        ["ProgramFiles", "ProgramFiles(x86)", "SystemRoot", "USERPROFILE", "ProgramData", "APPDATA", "LOCALAPPDATA"]
            .iter()
            .map(|v| env_path(v))
            .collect();
    shared.extend(roots.iter().map(|r| r.join("Programs")));
    let mut candidates: Vec<PathBuf> = location.iter().map(PathBuf::from).collect();
    let mut names = vec![app.name.clone(), base_name(&app.name)];
    names.retain(|n| n.chars().filter(char::is_ascii_alphanumeric).count() >= 3);
    names.dedup();
    for root in &roots {
        for n in &names {
            candidates.push(root.join(n));
            if !app.publisher.trim().is_empty() {
                candidates.push(root.join(app.publisher.trim()).join(n));
            }
        }
    }
    let mut out: Vec<(PathBuf, u64)> = Vec::new();
    for c in candidates {
        let is_shared = c.parent().is_none() || shared.iter().any(|s| s.as_os_str().eq_ignore_ascii_case(c.as_os_str()));
        let linked = fs::symlink_metadata(&c).is_ok_and(|m| m.file_attributes() & REPARSE_POINT != 0);
        if is_shared || linked || !c.is_dir() || out.iter().any(|(p, _)| p.as_os_str().eq_ignore_ascii_case(c.as_os_str())) {
            continue;
        }
        let size = dir_size(&c);
        out.push((c, size));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_versions() {
        assert_eq!(base_name("Foo Bar 2.1.0 (x64)"), "Foo Bar");
        assert_eq!(base_name("7-Zip 24.08"), "7-Zip");
        assert_eq!(base_name("Discord"), "Discord");
    }

    #[test]
    fn msi_switch() {
        assert_eq!(msi_uninstall("MsiExec.exe /I{AB-12}"), "MsiExec.exe /X{AB-12}");
        assert_eq!(msi_uninstall(r#""C:\a\unins000.exe" /I{x}"#), r#""C:\a\unins000.exe" /I{x}"#);
    }

    #[test]
    fn protects_critical() {
        assert!(protection("NVIDIA Graphics Driver 560.94 NVIDIA Corporation").is_some());
        assert!(protection("EasyAntiCheat Epic Games").is_some());
        assert!(protection("Microsoft Visual C++ 2015-2022 Redistributable (x64)").is_some());
        assert!(protection("Discord Discord Inc.").is_none());
    }

    #[test]
    fn dev_tools() {
        assert!(is_dev_tool("Git"));
        assert!(is_dev_tool("Python 3.12.4 (64-bit)"));
        assert!(!is_dev_tool("GitHub Desktop"));
        assert!(!is_dev_tool("Digital Paint"));
    }
}
