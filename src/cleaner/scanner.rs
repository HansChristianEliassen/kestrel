//! Cleanup categories and the read-only scanner. Only well-known cache/temp
//! locations are listed; nothing outside them is ever touched.

use std::collections::HashSet;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::system::{env_path, recycle_bin_info, service_running};

pub const REPARSE_POINT: u32 = 0x400;
const DAY: u64 = 86_400;

#[derive(Clone, Copy)]
pub enum Filter {
    Any,
    /// Skip recently used files (installers and running apps keep working files in Temp).
    OlderThanDays(u64),
    NamePrefix(&'static str),
    /// .log/.cab/.old files in a Windows log folder, older than N days.
    LogsOlderThanDays(u64),
}

pub struct Source {
    pub path: PathBuf,
    pub filter: Filter,
    /// (process or "service:name", app name): skip this location while that app runs, so its live data is never touched.
    pub blocker: Option<(&'static str, &'static str)>,
    pub admin: bool,
}

fn src(path: PathBuf, filter: Filter) -> Source {
    Source { path, filter, blocker: None, admin: false }
}

impl Source {
    fn admin(mut self) -> Self {
        self.admin = true;
        self
    }
    fn blocked_by(mut self, exe: &'static str, app: &'static str) -> Self {
        self.blocker = Some((exe, app));
        self
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Files,
    RecycleBin,
    /// Cleared with the supported Delete-DeliveryOptimizationCache cmdlet.
    DeliveryOptimization,
    /// Windows.old, removed through Disk Cleanup's own "Previous Installations" handler (its permissions block plain deletes).
    PreviousWindows,
    /// Superseded Windows components, removed with DISM /StartComponentCleanup. Its size is only known afterwards.
    ComponentStore,
}

pub struct Category {
    pub name: &'static str,
    pub desc: &'static str,
    pub warning: Option<&'static str>,
    pub default_on: bool,
    pub kind: Kind,
    pub sources: Vec<Source>,
}

#[derive(Default)]
pub struct Scanned {
    pub files: Vec<PathBuf>,
    pub bytes: u64,
    pub count: u64,
    pub notes: Vec<String>,
    /// Resolved scanned folders; cleanup refuses anything that resolves outside them.
    pub roots: Vec<PathBuf>,
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn cat(name: &'static str, desc: &'static str, default_on: bool, sources: Vec<Source>) -> Category {
    Category { name, desc, warning: None, default_on, kind: Kind::Files, sources }
}

pub fn categories() -> Vec<Category> {
    use Filter::*;
    let local = env_path("LOCALAPPDATA");
    let roaming = env_path("APPDATA");
    let win = env_path("SystemRoot");
    let pdata = env_path("ProgramData");

    let mut browser = Vec::new();
    for (dir, exe, app) in [
        (local.join(r"Google\Chrome\User Data"), "chrome.exe", "Chrome"),
        (local.join(r"Microsoft\Edge\User Data"), "msedge.exe", "Edge"),
        (local.join(r"BraveSoftware\Brave-Browser\User Data"), "brave.exe", "Brave"),
        (local.join(r"Vivaldi\User Data"), "vivaldi.exe", "Vivaldi"),
    ] {
        for profile in subdirs(&dir) {
            for c in ["Cache", "Code Cache", "GPUCache"] {
                browser.push(src(profile.join(c), Any).blocked_by(exe, app));
            }
        }
    }
    for profile in subdirs(&local.join(r"Mozilla\Firefox\Profiles")) {
        browser.push(src(profile.join("cache2"), Any).blocked_by("firefox.exe", "Firefox"));
    }

    let mut apps = vec![
        src(local.join(r"Microsoft\Windows\INetCache"), Any),
        src(local.join(r"Steam\htmlcache"), Any).blocked_by("steam.exe", "Steam"),
    ];
    for c in ["Cache", "Code Cache", "GPUCache"] {
        apps.push(src(roaming.join("discord").join(c), Any).blocked_by("discord.exe", "Discord"));
    }

    vec![
        cat(
            "User temporary files",
            "Files apps left in your Temp folder. Anything modified in the last 24 hours is kept.",
            true,
            vec![src(std::env::temp_dir(), OlderThanDays(1))],
        ),
        cat(
            "Windows temporary files",
            "System-wide temp files in the Windows Temp folder older than 24 hours.",
            true,
            vec![src(win.join("Temp"), OlderThanDays(1)).admin()],
        ),
        Category {
            kind: Kind::RecycleBin,
            ..cat("Recycle Bin", "Files you already deleted. Emptying the bin removes them permanently.", false, vec![])
        },
        cat(
            "Browser cache",
            "Cached web pages and scripts for Chrome, Edge, Brave, Vivaldi and Firefox. Logins, history and bookmarks are not touched.",
            true,
            browser,
        ),
        cat(
            "Thumbnail cache",
            "Explorer picture previews. Rebuilt automatically; files Explorer is using are skipped.",
            true,
            vec![src(local.join(r"Microsoft\Windows\Explorer"), NamePrefix("thumbcache_"))],
        ),
        cat(
            "Windows Update cache",
            "Already-installed update downloads. Windows re-downloads anything it still needs. Skipped while Windows Update is working.",
            true,
            // Microsoft's procedure stops the service before clearing this folder; Kestrel just waits until it's idle.
            vec![src(win.join(r"SoftwareDistribution\Download"), Any).admin().blocked_by("service:wuauserv", "Windows Update")],
        ),
        cat(
            "Windows error reports",
            "Archived and queued Windows Error Reporting files.",
            true,
            vec![
                src(local.join(r"Microsoft\Windows\WER\ReportArchive"), Any),
                src(local.join(r"Microsoft\Windows\WER\ReportQueue"), Any),
                src(pdata.join(r"Microsoft\Windows\WER\ReportArchive"), Any).admin(),
                src(pdata.join(r"Microsoft\Windows\WER\ReportQueue"), Any).admin(),
            ],
        ),
        cat(
            "Crash dumps",
            "Memory dumps written when apps or Windows crashed. Only useful for debugging those crashes.",
            true,
            vec![src(local.join("CrashDumps"), Any), src(win.join("Minidump"), Any).admin(), src(win.join("MEMORY.DMP"), Any).admin()],
        ),
        cat(
            "Old Windows log files",
            "Windows setup and servicing logs (.log/.cab) older than 14 days.",
            true,
            vec![src(win.join("Logs"), LogsOlderThanDays(14)).admin()],
        ),
        cat(
            "Application caches",
            "Web caches of Windows, Steam and Discord. Each app's cache is skipped while that app is running.",
            true,
            apps,
        ),
        Category {
            warning: Some("Games rebuild shaders after this, so the first launch of each game may stutter or load slower."),
            ..cat(
                "DirectX shader cache",
                "Compiled GPU shaders from DirectX, NVIDIA, AMD and Intel drivers.",
                false,
                vec![
                    src(local.join("D3DSCache"), Any),
                    src(local.join(r"NVIDIA\DXCache"), Any),
                    src(local.join(r"NVIDIA\GLCache"), Any),
                    src(local.join(r"AMD\DxCache"), Any),
                    src(local.join(r"AMD\DxcCache"), Any),
                    src(local.join(r"AMD\VkCache"), Any),
                    src(local.join(r"Intel\ShaderCache"), Any),
                ],
            )
        },
        Category {
            kind: Kind::DeliveryOptimization,
            ..cat(
                "Delivery Optimization files",
                "Leftover update pieces Windows keeps to share with other PCs. Cleared with Windows' own cleanup command.",
                true,
                vec![src(win.join(r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"), Any)
                    .admin()],
            )
        },
        Category {
            kind: Kind::PreviousWindows,
            warning: Some("After this you can't go back to your previous version of Windows (Settings > System > Recovery)."),
            ..cat(
                "Previous Windows installation",
                "Windows.old, the copy of your old Windows kept after a feature update. Removed with Windows' Disk Cleanup.",
                false,
                vec![src(PathBuf::from(format!(r"{}\Windows.old", env_path("SystemDrive").display())), Any).admin()],
            )
        },
        Category {
            kind: Kind::ComponentStore,
            warning: Some("Installed updates can no longer be uninstalled afterwards. Takes several minutes."),
            ..cat(
                "Old Windows update components",
                "Older versions of Windows components that updates have replaced. Cleaned with Windows' own DISM tool; the space freed is measured afterwards.",
                false,
                vec![src(win.join("WinSxS"), Any).admin()],
            )
        },
    ]
}

fn matches(filter: Filter, path: &Path, meta: &fs::Metadata, now: SystemTime) -> bool {
    // Unknown age counts as new, so it is kept.
    let age_days = || meta.modified().ok().and_then(|m| now.duration_since(m).ok()).map_or(0, |d| d.as_secs() / DAY);
    let lower = |s: Option<&std::ffi::OsStr>| s.and_then(|s| s.to_str()).map(str::to_ascii_lowercase);
    match filter {
        Filter::Any => true,
        Filter::OlderThanDays(d) => age_days() >= d,
        Filter::NamePrefix(p) => lower(path.file_name()).is_some_and(|n| n.starts_with(p)),
        Filter::LogsOlderThanDays(d) => {
            lower(path.extension()).is_some_and(|e| matches!(e.as_str(), "log" | "cab" | "old")) && age_days() >= d
        }
    }
}

/// Collects matching files. Never follows symlinks or junctions.
fn walk(path: &Path, filter: Filter, now: SystemTime, out: &mut Scanned) {
    let Ok(meta) = fs::symlink_metadata(path) else { return };
    if meta.file_attributes() & REPARSE_POINT != 0 {
        return;
    }
    if meta.is_dir() {
        if let Ok(rd) = fs::read_dir(path) {
            for e in rd.flatten() {
                walk(&e.path(), filter, now, out);
            }
        }
    } else if matches(filter, path, &meta, now) {
        out.bytes += meta.len();
        out.files.push(path.to_path_buf());
    }
}

pub fn scan(cat: &Category, running: &HashSet<String>, admin: bool) -> Scanned {
    let mut out = Scanned::default();
    if cat.kind == Kind::RecycleBin {
        match recycle_bin_info() {
            Some((bytes, count)) => (out.bytes, out.count) = (bytes, count),
            None => out.notes.push("Could not read the Recycle Bin size.".into()),
        }
        return out;
    }
    // Not walked (WinSxS is mostly hard links to files Windows uses); one entry stands for the DISM cleanup.
    if cat.kind == Kind::ComponentStore {
        if admin {
            out.count = 1;
        } else {
            out.notes.push("Needs administrator rights (Settings > Restart as administrator).".into());
        }
        return out;
    }
    let now = SystemTime::now();
    let mut needs_admin = false;
    for s in &cat.sources {
        // A missing environment variable would make the path relative; never scan those.
        if !s.path.is_absolute() {
            continue;
        }
        if s.admin && !admin {
            needs_admin |= s.path.exists();
            continue;
        }
        if let Some((exe, app)) = s.blocker {
            let service = exe.strip_prefix("service:");
            if running.contains(exe) || service.is_some_and(service_running) {
                let note = if service.is_some() {
                    format!("{app} is busy, so its cache was skipped. Scan again later to include it.")
                } else {
                    format!("{app} is running, so its cache was skipped. Close it and scan again to include it.")
                };
                if !out.notes.contains(&note) {
                    out.notes.push(note);
                }
                continue;
            }
        }
        if let Ok(root) = fs::canonicalize(&s.path) {
            out.roots.push(root);
        }
        walk(&s.path, s.filter, now, &mut out);
    }
    if needs_admin {
        out.notes.push("Some locations need administrator rights (Settings > Restart as administrator).".into());
    }
    out.count = out.files.len() as u64;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn filters() {
        let dir = std::env::temp_dir().join("kestrel_filter_test");
        let _ = fs::create_dir_all(&dir);
        let f = dir.join("thumbcache_32.db");
        fs::write(&f, b"x").unwrap();
        let meta = fs::metadata(&f).unwrap();
        let now = SystemTime::now();
        assert!(matches(Filter::NamePrefix("thumbcache_"), &f, &meta, now));
        assert!(!matches(Filter::NamePrefix("iconcache_"), &f, &meta, now));
        assert!(!matches(Filter::OlderThanDays(1), &f, &meta, now));
        assert!(matches(Filter::OlderThanDays(1), &f, &meta, now + Duration::from_secs(2 * DAY)));
        assert!(!matches(Filter::LogsOlderThanDays(0), &f, &meta, now));
        let _ = fs::remove_dir_all(&dir);
    }
}
