//! Windows > System analysis: what runs on this PC (processes, threads, handles, svchost hosts, services, memory,
//! background CPU, startup apps, scheduled tasks), compared with a saved baseline, plus what other optimizers or
//! manual changes have already done. Read-only.

use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use crate::system::process_list;
use crate::utils::data_dir;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// Unix time.
    pub when: u64,
    pub processes: u32,
    pub threads: u32,
    pub handles: u32,
    pub svchost: u32,
    pub services: u32,
    /// Running services hosted in svchost.exe.
    pub hosted: u32,
    pub ram_mb: u64,
    /// Whole-system CPU use over one second, in tenths of a percent.
    pub cpu_permille: u32,
    pub startup: u32,
    pub tasks: u32,
}

/// Measures now. Takes about a second (the CPU sample) plus the service and task scans; run it off the UI thread.
pub fn take() -> Snapshot {
    use windows::Win32::System::ProcessStatus::{K32GetPerformanceInfo, PERFORMANCE_INFORMATION};
    let mut perf = PERFORMANCE_INFORMATION { cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32, ..Default::default() };
    let _ = unsafe { K32GetPerformanceInfo(&mut perf, perf.cb) };
    let (services, hosted) = running_services();
    Snapshot {
        when: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        processes: perf.ProcessCount,
        threads: perf.ThreadCount,
        handles: perf.HandleCount,
        svchost: process_list().iter().filter(|(_, n)| n == "svchost.exe").count() as u32,
        services,
        hosted,
        ram_mb: ((perf.PhysicalTotal - perf.PhysicalAvailable) * perf.PageSize / 1_048_576) as u64,
        cpu_permille: cpu_permille(),
        startup: crate::startup::list().iter().filter(|e| e.enabled).count() as u32,
        tasks: crate::startup::enabled_task_count() as u32,
    }
}

/// (running services, of which hosted in svchost.exe). Drivers aren't counted.
fn running_services() -> (u32, u32) {
    let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SYSTEM\CurrentControlSet\Services") else { return (0, 0) };
    let (mut running, mut hosted) = (0, 0);
    for name in root.enum_keys().flatten() {
        let Ok(k) = root.open_subkey(&name) else { continue };
        let kind: u32 = k.get_value("Type").unwrap_or(0);
        if kind & 0x30 == 0 || !crate::system::service_state(&name) {
            continue;
        }
        running += 1;
        if k.get_value::<String, _>("ImagePath").is_ok_and(|p| p.to_lowercase().contains("svchost.exe")) {
            hosted += 1;
        }
    }
    (running, hosted)
}

fn cpu_permille() -> u32 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetSystemTimes;
    let sample = || {
        let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        let _ = unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) };
        let v = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
        (v(idle), v(kernel) + v(user))
    };
    let (i0, t0) = sample();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let (i1, t1) = sample();
    busy_permille(i1.saturating_sub(i0), t1.saturating_sub(t0))
}

/// Kernel time includes idle time, so busy = total - idle.
fn busy_permille(idle: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    (total.saturating_sub(idle) * 1000 / total) as u32
}

fn baseline_path() -> std::path::PathBuf {
    data_dir().join("analysis-baseline.txt")
}

fn to_line(s: &Snapshot) -> String {
    [
        s.when,
        s.processes.into(),
        s.threads.into(),
        s.handles.into(),
        s.svchost.into(),
        s.services.into(),
        s.hosted.into(),
        s.ram_mb,
        s.cpu_permille.into(),
        s.startup.into(),
        s.tasks.into(),
    ]
    .map(|v| v.to_string())
    .join("\t")
}

fn from_line(line: &str) -> Option<Snapshot> {
    let v: Vec<u64> = line.trim().split('\t').map(|f| f.parse().ok()).collect::<Option<_>>()?;
    let [when, processes, threads, handles, svchost, services, hosted, ram_mb, cpu, startup, tasks] = v.as_slice() else { return None };
    let n = |x: &u64| u32::try_from(*x).ok();
    Some(Snapshot {
        when: *when,
        processes: n(processes)?,
        threads: n(threads)?,
        handles: n(handles)?,
        svchost: n(svchost)?,
        services: n(services)?,
        hosted: n(hosted)?,
        ram_mb: *ram_mb,
        cpu_permille: n(cpu)?,
        startup: n(startup)?,
        tasks: n(tasks)?,
    })
}

pub fn baseline() -> Option<Snapshot> {
    from_line(&std::fs::read_to_string(baseline_path()).ok()?)
}

pub fn save_baseline(s: &Snapshot) -> Result<(), String> {
    crate::utils::write_atomic(&baseline_path(), to_line(s).as_bytes()).map_err(|e| format!("Couldn't save the baseline: {e}"))
}

/// Plain-language reading of the change, separating less work from fewer processes doing the same work.
pub fn verdict(before: &Snapshot, now: &Snapshot) -> Vec<String> {
    let d = |a: u32, b: u32| i64::from(b) - i64::from(a);
    let (hosts, services, threads) = (d(before.svchost, now.svchost), d(before.services, now.services), d(before.threads, now.threads));
    let mut out = Vec::new();
    if hosts <= -5 && services * 2 > hosts {
        out.push(format!(
            "{} fewer svchost processes, but almost the same services run ({} → {}). That's service grouping: the same work in fewer processes, which lowers the count without freeing CPU time.",
            -hosts, before.services, now.services
        ));
    }
    if services < 0 {
        out.push(format!("{} fewer services running: work Windows no longer does.", -services));
    }
    if threads <= -50 {
        out.push(format!("{} fewer threads: less is waiting to run.", -threads));
    }
    if out.is_empty() {
        out.push("No real change yet. Apply changes, restart, and measure again with nothing else open.".into());
    }
    out
}

/// Something found on this PC: (what, current state, whether it reduces protection or compatibility).
pub struct Finding {
    pub title: &'static str,
    pub detail: String,
    pub risk: bool,
}

fn hklm_dword(key: &str, name: &str) -> Option<u32> {
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(key).ok()?.get_value(name).ok()
}

/// Other optimizers and earlier changes, so they're recognized instead of applied twice or contradicted.
pub fn findings() -> Vec<Finding> {
    let mut f = Vec::new();
    let windir = crate::system::env_path("SystemRoot");
    if windir.join("AtlasDesktop").is_dir() {
        f.push(Finding {
            title: "AtlasOS",
            detail: "Its playbook was applied (the AtlasDesktop folder is in the Windows folder).".into(),
            risk: false,
        });
    }
    if RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\XOSToolkit").is_ok() {
        f.push(Finding { title: "xOS", detail: "Its playbook was applied (the XOSToolkit registry key exists).".into(), risk: false });
    }
    let ram_kb = {
        use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
            m.ullTotalPhys / 1024
        } else {
            0
        }
    };
    let threshold = hklm_dword(r"SYSTEM\CurrentControlSet\Control", "SvcHostSplitThresholdInKB");
    let per_service = grouped_services();
    let grouping = match (threshold, per_service) {
        (Some(t), _) if u64::from(t) > ram_kb && ram_kb > 0 => {
            Some(format!("On for all services (threshold {} MB is above this PC's memory).", t / 1024))
        }
        (_, n) if n >= 20 => Some(format!("On for {n} services, set one by one (the way AtlasOS does it).")),
        _ => None,
    };
    f.push(Finding {
        title: "Service grouping",
        detail: grouping.unwrap_or_else(|| {
            "Off: Windows runs most services in their own svchost process (its default on PCs with over 3.5 GB of memory).".into()
        }),
        risk: false,
    });
    let defender = process_list().iter().any(|(_, n)| n == "msmpeng.exe");
    f.push(Finding {
        title: "Microsoft Defender",
        detail: if defender { "Running.".into() } else { "Not running. Make sure another antivirus protects this PC.".into() },
        risk: !defender,
    });
    let hvci = hklm_dword(r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity", "Enabled") == Some(1);
    f.push(Finding {
        title: "Memory integrity (HVCI)",
        detail: if hvci {
            "On.".into()
        } else {
            "Off. Faster in some CPU-limited games, but FACEIT requires virtualization-based security and Riot Vanguard's on-demand mode requires memory integrity.".into()
        },
        risk: !hvci,
    });
    if hklm_dword(r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU", "NoAutoUpdate") == Some(1) {
        f.push(Finding {
            title: "Windows Update",
            detail: "Automatic updates are turned off by policy. Security fixes don't install, and FACEIT and ESEA need current updates."
                .into(),
            risk: true,
        });
    }
    let mm = r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management";
    if hklm_dword(mm, "FeatureSettingsOverride") == Some(3) && hklm_dword(mm, "FeatureSettingsOverrideMask") == Some(3) {
        f.push(Finding {
            title: "CPU vulnerability protections",
            detail: "Turned off (FeatureSettingsOverride). This reopens Spectre-class attacks, and AtlasOS notes it can lower performance on some recent CPUs.".into(),
            risk: true,
        });
    }
    if hklm_dword(r"SYSTEM\CurrentControlSet\Services\DiagTrack", "Start") == Some(4) {
        f.push(Finding { title: "Telemetry service", detail: "Disabled (DiagTrack).".into(), risk: false });
    }
    f
}

/// Services marked SvcHostSplitDisable=1, which share an svchost process whatever the threshold.
pub(crate) fn grouped_services() -> usize {
    let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SYSTEM\CurrentControlSet\Services") else { return 0 };
    root.enum_keys()
        .flatten()
        .filter(|n| root.open_subkey(n).and_then(|k| k.get_value::<u32, _>("SvcHostSplitDisable")).is_ok_and(|v| v == 1))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(svchost: u32, services: u32, threads: u32) -> Snapshot {
        Snapshot {
            when: 1,
            processes: 100,
            threads,
            handles: 50_000,
            svchost,
            services,
            hosted: services,
            ram_mb: 4000,
            cpu_permille: 12,
            startup: 3,
            tasks: 150,
        }
    }

    #[test]
    fn baseline_line_round_trips() {
        let s = snap(70, 150, 2000);
        assert_eq!(from_line(&to_line(&s)), Some(s));
        assert_eq!(from_line("1\t2"), None);
        assert_eq!(from_line("garbage"), None);
    }

    #[test]
    fn grouping_is_told_apart_from_less_work() {
        let grouped = verdict(&snap(70, 150, 2000), &snap(20, 149, 1990));
        assert!(grouped[0].contains("service grouping"), "{grouped:?}");
        let less = verdict(&snap(70, 150, 2000), &snap(60, 120, 1700));
        assert!(!less.iter().any(|l| l.contains("grouping")) && less.iter().any(|l| l.contains("30 fewer services")), "{less:?}");
        assert!(verdict(&snap(30, 100, 1000), &snap(30, 100, 1000))[0].contains("No real change"));
    }

    #[test]
    #[ignore = "reads this PC (read-only)"]
    fn live_snapshot() {
        println!("{:?}", take());
        for f in findings() {
            println!("{} | {} | risk={}", f.title, f.detail, f.risk);
        }
    }

    #[test]
    fn busy_share_excludes_idle() {
        assert_eq!(busy_permille(900, 1000), 100);
        assert_eq!(busy_permille(0, 0), 0);
    }
}
