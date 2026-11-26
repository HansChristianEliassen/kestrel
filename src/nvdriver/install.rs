//! Installing a built package with NVIDIA's own installer, and measuring NVIDIA's footprint before and after:
//! services, scheduled tasks, running processes and their memory, and disk space. Only measured values are shown.

use std::collections::HashSet;
use std::path::Path;

use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use crate::gamemode::watch::process_memory;
use crate::system::{expand_env, process_list, service_state, shell_execute};
use crate::utils::{data_dir, log};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Footprint {
    /// (service key, display name, running).
    pub services: Vec<(String, String, bool)>,
    /// (task path, enabled).
    pub tasks: Vec<(String, bool)>,
    /// (process file name, memory in bytes).
    pub processes: Vec<(String, u64)>,
    /// Bytes in NVIDIA's Program Files and ProgramData folders.
    pub disk: u64,
}

fn is_nvidia_process(name: &str) -> bool {
    name.starts_with("nv") || name.contains("nvidia") || name == "fvcontainer.exe"
}

fn is_nvidia_task(path: &str) -> bool {
    let name = path.rsplit('\\').next().unwrap_or(path);
    name.starts_with("nv") || path.contains("nvidia")
}

/// Telemetry, as far as names show it: reporting services and NVIDIA's "NvTm" reporting tasks.
pub fn is_telemetry(name: &str) -> bool {
    let n = name.to_lowercase();
    let last = n.rsplit('\\').next().unwrap_or(&n);
    n.contains("telemetry") || last.starts_with("nvtm")
}

const DISK: [&str; 3] =
    [r"%ProgramFiles%\NVIDIA Corporation", r"%ProgramFiles(x86)%\NVIDIA Corporation", r"%ProgramData%\NVIDIA Corporation"];

pub fn footprint() -> Footprint {
    let mut services = Vec::new();
    if let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SYSTEM\CurrentControlSet\Services") {
        for name in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&name) else { continue };
            // Programs that run as services (not kernel drivers), per-user templates left out.
            let kind: u32 = k.get_value("Type").unwrap_or(0);
            if kind & 0x30 == 0 || kind & 0x80 != 0 {
                continue;
            }
            let display: String = k.get_value("DisplayName").unwrap_or_default();
            let image: String = k.get_value("ImagePath").unwrap_or_default();
            if display.to_lowercase().contains("nvidia") || image.to_lowercase().contains("nvidia") {
                let running = service_state(&name);
                services.push((name, if display.is_empty() || display.starts_with('@') { String::new() } else { display }, running));
            }
        }
    }
    crate::system::com_init();
    let tasks = crate::startup::find_tasks(is_nvidia_task);
    let names: HashSet<String> = process_list().into_iter().map(|(_, n)| n).filter(|n| is_nvidia_process(n)).collect();
    let memory = process_memory(&names);
    let mut processes: Vec<(String, u64)> = names.into_iter().map(|n| (n.clone(), memory.get(&n).copied().unwrap_or(0))).collect();
    processes.sort();
    let disk = DISK.iter().map(|d| super::package::dir_size(Path::new(&expand_env(d)))).sum();
    Footprint { services, tasks, processes, disk }
}

impl Footprint {
    pub fn memory(&self) -> u64 {
        self.processes.iter().map(|p| p.1).sum()
    }

    pub fn running(&self) -> usize {
        self.services.iter().filter(|s| s.2).count()
    }

    fn to_text(&self) -> String {
        let mut out = format!("disk\t{}\n", self.disk);
        for (k, d, r) in &self.services {
            out.push_str(&format!("service\t{k}\t{d}\t{}\n", u8::from(*r)));
        }
        for (p, e) in &self.tasks {
            out.push_str(&format!("task\t{p}\t{}\n", u8::from(*e)));
        }
        for (n, m) in &self.processes {
            out.push_str(&format!("process\t{n}\t{m}\n"));
        }
        out
    }

    fn from_text(text: &str) -> Footprint {
        let mut f = Footprint::default();
        for line in text.lines() {
            let p: Vec<&str> = line.split('\t').collect();
            match p[..] {
                ["disk", n] => f.disk = n.parse().unwrap_or(0),
                ["service", k, d, r] => f.services.push((k.into(), d.into(), r == "1")),
                ["task", t, e] => f.tasks.push((t.into(), e == "1")),
                ["process", n, m] => f.processes.push((n.into(), m.parse().unwrap_or(0))),
                _ => {}
            }
        }
        f
    }
}

fn before_path() -> std::path::PathBuf {
    data_dir().join("nvidia-before.txt")
}

/// The footprint measured just before the last install, if any.
pub fn saved_before() -> Option<Footprint> {
    std::fs::read_to_string(before_path()).ok().map(|t| Footprint::from_text(&t))
}

fn pending_path() -> std::path::PathBuf {
    data_dir().join("nvidia-pending.txt")
}

fn result_path() -> std::path::PathBuf {
    data_dir().join("nvidia-result.txt")
}

/// An install, from start until its result has been checked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pending {
    pub version: String,
    /// Components left out (folder names).
    pub removed: Vec<String>,
    /// NVIDIA's installer exit code; None until it reports back (or when it never did).
    pub code: Option<u32>,
    /// Block NVIDIA telemetry afterwards.
    pub block: bool,
    /// Apply the NVIDIA gaming profile afterwards (a clean installation resets driver settings).
    pub profile: bool,
    /// The driver installed before, kept for rollback.
    pub previous: Option<String>,
}

impl Pending {
    fn to_text(&self) -> String {
        format!(
            "{}\n{}\nblock={} profile={}\n{}\n",
            self.version,
            self.removed.join("|"),
            u8::from(self.block),
            u8::from(self.profile),
            self.previous.as_deref().unwrap_or("")
        )
    }

    fn from_text(text: &str) -> Option<Pending> {
        let mut lines = text.lines();
        let version = lines.next()?.trim().to_string();
        let removed = lines.next().unwrap_or("").split('|').filter(|s| !s.is_empty()).map(String::from).collect();
        let flags = lines.next().unwrap_or("");
        let previous = lines.next().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        Some(Pending { version, removed, code: None, block: flags.contains("block=1"), profile: flags.contains("profile=1"), previous })
    }
}

/// Starts NVIDIA's installer on a package folder, after saving the current footprint. Windows asks for
/// administrator rights when Kestrel doesn't have them. Returns once the installer has started.
///
/// NVIDIA's OpenGL driver ends every OpenGL program (Kestrel included) when the graphics card disconnects during
/// the swap, and with a clean installation the old driver is gone for minutes before the new one is in. So the
/// installer runs from a small script in a visible window that says what's happening and not to restart; the
/// script records the exit code and opens Kestrel again on the Driver Optimizer, which finishes the job
/// (`install_state`, `after_install`). Kestrel closes itself right after starting it instead of being killed.
pub fn install(dir: &Path, clean: bool, job: &Pending) -> Result<(), String> {
    let setup = dir.join("setup.exe");
    if !setup.is_file() {
        return Err("The driver package is missing; build it again.".into());
    }
    let _ = crate::utils::write_atomic(&before_path(), footprint().to_text().as_bytes());
    let _ = std::fs::remove_file(result_path());
    std::fs::write(pending_path(), job.to_text()).map_err(|e| e.to_string())?;
    let args = if clean { "-s -noreboot -noeula -clean" } else { "-s -noreboot -noeula" };
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let v = &job.version;
    let script = [
        "@echo off".to_string(),
        format!("title Kestrel - installing NVIDIA driver {v}"),
        "echo.".into(),
        format!("echo   Installing NVIDIA driver {v}. This takes a few minutes."),
        "echo.".into(),
        "echo   The screen may go dark or flicker, and for a while Windows shows no graphics driver.".into(),
        "echo   That's normal during the install. DON'T restart or turn off the PC until this window closes.".into(),
        "echo.".into(),
        "echo   Kestrel opens again by itself when NVIDIA's installer is done.".into(),
        format!("\"{}\" {args}", setup.display()),
        format!("> \"{}\" echo %ERRORLEVEL%", result_path().display()),
        format!("start \"\" \"{}\" {} {}driver", me.display(), crate::system::RELAUNCHED, crate::system::PAGE_ARG),
        String::new(),
    ]
    .join("\r\n");
    let cmd = data_dir().join("nvidia-install.cmd");
    std::fs::write(&cmd, script).map_err(|e| e.to_string())?;
    log(&format!("NVIDIA driver: installing {} ({args})", dir.display()));
    shell_execute("cmd.exe", &format!("/c \"\"{}\"\"", cmd.display()), true, None).map_err(|e| {
        let _ = std::fs::remove_file(pending_path());
        format!("NVIDIA's installer didn't start: {e}")
    })?;
    Ok(())
}

/// Where the last install stands.
pub enum InstallState {
    /// Nothing pending.
    Idle,
    /// NVIDIA's installer is still working.
    Running,
    /// Finished (with its exit code), or interrupted (no code: a restart or a closed window stopped it).
    Done(Pending),
}

/// Whether an install is running, finished or was interrupted. A finished or interrupted one is reported once.
pub fn install_state() -> InstallState {
    let Ok(text) = std::fs::read_to_string(pending_path()) else { return InstallState::Idle };
    let code = std::fs::read_to_string(result_path()).ok().and_then(|t| t.trim().parse().ok());
    if code.is_none() {
        let age = std::fs::metadata(pending_path()).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        // shortcut: any process named setup.exe counts as NVIDIA's installer; match its path if other installers confuse it.
        let running = process_list().iter().any(|(_, n)| n == "setup.exe");
        if running || age.is_some_and(|a| a.as_secs() < 30) {
            return InstallState::Running;
        }
    }
    let _ = std::fs::remove_file(pending_path());
    let _ = std::fs::remove_file(result_path());
    let Some(mut p) = Pending::from_text(&text) else { return InstallState::Idle };
    p.code = code;
    log(&format!("NVIDIA driver: installer for {} ended with code {code:?}", p.version));
    InstallState::Done(p)
}

/// Stripped packages kept for rollback.
fn archive_dir() -> std::path::PathBuf {
    data_dir().join("NVIDIA drivers")
}

/// Driver packages kept for rollback: (version, folder, bytes).
pub fn kept() -> Vec<(String, std::path::PathBuf, u64)> {
    let mut out: Vec<_> = std::fs::read_dir(archive_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("setup.exe").is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path(), super::package::dir_size(&e.path())))
        .collect();
    out.sort();
    out
}

/// The components a kept package leaves out.
pub fn kept_removed(version: &str) -> Vec<String> {
    std::fs::read_to_string(archive_dir().join(format!("{version}.removed")))
        .map_or(Vec::new(), |t| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
}

/// Keeps the package just installed (moving the build out of the temporary folder) and the one installed before;
/// older ones are deleted. A version already kept stays where it is.
fn keep(p: &Pending) -> Result<(), String> {
    let dir = archive_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(&p.version);
    let build = super::build_dir(&p.version);
    if build.join("setup.exe").is_file() {
        if target.exists() {
            std::fs::remove_dir_all(&target).map_err(|e| e.to_string())?;
        }
        // Same drive as the temporary folder in practice; a plain rename then, nothing copied.
        std::fs::rename(&build, &target).map_err(|e| format!("Couldn't keep the package for rollback: {e}"))?;
        std::fs::write(dir.join(format!("{}.removed", p.version)), p.removed.join("\n")).map_err(|e| e.to_string())?;
    }
    for (v, path, _) in kept() {
        if v != p.version && Some(&v) != p.previous.as_ref() {
            let _ = std::fs::remove_dir_all(path);
            let _ = std::fs::remove_file(dir.join(format!("{v}.removed")));
        }
    }
    Ok(())
}

/// Deletes the packages kept for rollback.
pub fn forget_kept() -> Result<(), String> {
    let dir = archive_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't delete the kept drivers: {e}"))?;
    }
    Ok(())
}

/// Finishes an install: blocks telemetry and applies the gaming profile when chosen, puts Kestrel's NVIDIA
/// settings back where the install reset them, keeps the package for rollback, then checks the result.
pub fn after_install(p: &Pending) -> Vec<CheckLine> {
    let mut out: Vec<CheckLine> = Vec::new();
    let mut done = Vec::new();
    if p.block {
        match super::telemetry::set_blocked(true) {
            Ok(()) => done.push("NVIDIA telemetry blocked".to_string()),
            Err(e) => out.push((Some(false), "Telemetry block".into(), e)),
        }
    }
    let profile = crate::optimizer::Tweak::NvShaderCache;
    if p.profile && !profile.is_applied() {
        match profile.apply() {
            Ok(()) => done.push("NVIDIA gaming profile applied".to_string()),
            Err(e) => out.push((Some(false), "NVIDIA gaming profile".into(), e)),
        }
    }
    let again = crate::optimizer::gaming::reapply_nvidia();
    if !again.is_empty() {
        done.push(format!("put back after the install: {}", again.join("; ")));
    }
    if !done.is_empty() {
        out.push((Some(true), "Kestrel's settings".into(), format!("{}.", done.join(", "))));
    }
    if let Err(e) = keep(p) {
        out.push((None, "Rollback".into(), e));
    }
    out.extend(verify(&p.version, &p.removed, saved_before().as_ref()));
    out
}

/// One verification result: (good, bad or just information, title, detail).
pub type CheckLine = (Option<bool>, String, String);

/// What's true after an install of `expected` with these components left out, compared with `before`.
pub fn verify(expected: &str, removed: &[String], before: Option<&Footprint>) -> Vec<CheckLine> {
    let mut out: Vec<CheckLine> = Vec::new();
    let gpu = super::detect();
    match &gpu {
        Some(g) => {
            let installed = g.installed.clone().unwrap_or_else(|| "unknown".into());
            if installed == expected {
                out.push((Some(true), "Driver version".into(), format!("{} runs driver {installed}.", g.name)));
            } else {
                out.push((
                    Some(false),
                    "Driver version".into(),
                    format!("Windows still reports {installed}. Restart Windows to finish, then check again."),
                ));
            }
        }
        None => {
            out.push((Some(false), "Graphics card".into(), "Windows doesn't list an NVIDIA graphics card with an NVIDIA driver.".into()))
        }
    }
    if let Some(sys) = driver_file() {
        let ok = super::signature_status(&sys).is_ok_and(|(s, _)| s == "Valid");
        out.push((
            Some(ok),
            "Driver signature".into(),
            if ok { "nvlddmkm.sys has a valid signature.".into() } else { "Windows couldn't confirm the driver file's signature.".into() },
        ));
    }
    let app = Path::new(&expand_env(r"%ProgramFiles%\NVIDIA Corporation\NVIDIA App")).is_dir();
    if removed.iter().any(|r| r == "NvApp") && app {
        out.push((
            Some(false),
            "NVIDIA App".into(),
            "Still installed from before: remove it in Settings > Apps if you don't want it.".into(),
        ));
    }
    let physx = Path::new(&expand_env(r"%ProgramFiles(x86)%\NVIDIA Corporation\PhysX")).is_dir();
    if !removed.iter().any(|r| r == "PhysX") {
        out.push((Some(physx), "PhysX".into(), if physx { "Installed.".into() } else { "Not found.".into() }));
    }
    let now = footprint();
    let tel_services: Vec<String> = now.services.iter().filter(|s| is_telemetry(&s.0) || is_telemetry(&s.1)).map(|s| s.0.clone()).collect();
    let tel_tasks: Vec<String> = now.tasks.iter().filter(|t| is_telemetry(&t.0)).map(|t| t.0.clone()).collect();
    out.push((
        Some(tel_services.is_empty() && tel_tasks.is_empty()),
        "Telemetry services and tasks".into(),
        if tel_services.is_empty() && tel_tasks.is_empty() {
            "None by name. The display driver's own container still reports some usage data; that's part of NVIDIA's signed driver.".into()
        } else {
            format!("Still there: {}", tel_services.iter().chain(&tel_tasks).cloned().collect::<Vec<_>>().join(", "))
        },
    ));
    let line =
        |label: &str, b: Option<String>, a: String| (None, label.to_string(), b.map_or(a.clone(), |b| format!("{b} before, {a} now")));
    out.push(line(
        "NVIDIA services",
        before.map(|b| format!("{} ({} running)", b.services.len(), b.running())),
        format!("{} ({} running)", now.services.len(), now.running()),
    ));
    out.push(line("NVIDIA scheduled tasks", before.map(|b| b.tasks.len().to_string()), now.tasks.len().to_string()));
    out.push(line("NVIDIA processes", before.map(|b| b.processes.len().to_string()), now.processes.len().to_string()));
    // NVIDIA's services run as SYSTEM: without administrator rights their memory can't be read, which isn't 0.
    let memory = |f: &Footprint| if f.memory() == 0 && !f.processes.is_empty() { "unreadable".to_string() } else { mb(f.memory()) };
    let mut mem = line("Their memory", before.map(memory), memory(&now));
    if memory(&now) == "unreadable" {
        mem.2.push_str(" (Kestrel needs administrator rights to read the memory of NVIDIA's services)");
    }
    out.push(mem);
    out.push(line("Disk space (NVIDIA folders)", before.map(|b| mb(b.disk)), mb(now.disk)));
    out
}

fn mb(b: u64) -> String {
    format!("{} MB", b >> 20)
}

/// The newest installed copy of NVIDIA's kernel driver in the driver store.
fn driver_file() -> Option<std::path::PathBuf> {
    let store = Path::new(&expand_env(r"%SystemRoot%\System32\DriverStore\FileRepository")).to_path_buf();
    std::fs::read_dir(store)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().to_lowercase().starts_with("nv_dispi.inf_"))
        .map(|e| e.path().join("nvlddmkm.sys"))
        .filter(|p| p.is_file())
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_round_trip() {
        let p = Pending {
            version: "621.40".into(),
            removed: vec!["NvApp".into(), "NvTelemetry".into()],
            code: None,
            block: true,
            profile: false,
            previous: Some("621.24".into()),
        };
        assert_eq!(Pending::from_text(&p.to_text()), Some(p));
        // Written by 0.27: version and components only.
        let old = Pending::from_text("621.40\nNvApp|NvVAD\n").unwrap();
        assert_eq!((old.removed.len(), old.block, old.previous), (2, false, None));
    }

    #[test]
    fn names_and_saving() {
        assert!(is_nvidia_process("nvdisplay.container.exe") && is_nvidia_process("nvidia app.exe") && !is_nvidia_process("steam.exe"));
        assert!(is_nvidia_task(r"\nvtmrep_crashreport1_{b2fe1952}") && !is_nvidia_task(r"\microsoft\windows\defrag\scheduleddefrag"));
        assert!(
            is_telemetry(r"\NvTmRep_CrashReport1")
                && is_telemetry("NvTelemetryContainer")
                && !is_telemetry("NVDisplay.ContainerLocalSystem")
        );
        let f = Footprint {
            services: vec![("NVDisplay.ContainerLocalSystem".into(), "NVIDIA Display Container LS".into(), true)],
            tasks: vec![(r"\NvDriverUpdateCheckDaily".into(), false)],
            processes: vec![("nvdisplay.container.exe".into(), 42)],
            disk: 7,
        };
        assert_eq!(Footprint::from_text(&f.to_text()), f);
    }

    #[test]
    #[ignore = "reads this PC's NVIDIA services, tasks and processes"]
    fn live_footprint() {
        let f = footprint();
        println!("{f:#?}\nmemory {} MB, disk {} MB", f.memory() >> 20, f.disk >> 20);
    }
}
