//! NVIDIA telemetry that stays after stripping: the display driver's own container (NVDisplay.Container) loads
//! NvTelemetry inside the signed driver, which a package-level strip can't remove without breaking the signature.
//! Kestrel blocks the hosts it reports to (read from its own log: events, feedback and activation) in the Windows
//! hosts file instead. Precise (NVIDIA's game-profile updates and driver downloads keep working), reversible, and
//! it survives driver updates. `check` shows whether it holds.

use std::net::ToSocketAddrs;

use crate::system::{expand_env, process_list, run_hidden};
use crate::utils::log;

/// NvTelemetry's endpoints, from its log (`DisplayDriverRAS\NvTelemetry\NvTelemetry.log`).
pub const HOSTS: [&str; 3] = ["events.telemetry.data.nvidia.com", "feedbacks.telemetry.data.nvidia.com", "activation.gfe.nvidia.com"];
const START: &str = "# Kestrel: NVIDIA telemetry (start)";
const END: &str = "# Kestrel: NVIDIA telemetry (end)";

fn hosts_path() -> std::path::PathBuf {
    std::path::PathBuf::from(expand_env(r"%SystemRoot%\System32\drivers\etc\hosts"))
}

/// The hosts file without Kestrel's block.
fn without_block(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        match line.trim() {
            START => inside = true,
            END => inside = false,
            _ if !inside => out.push_str(line),
            _ => {}
        }
    }
    out
}

fn with_block(text: &str) -> String {
    let mut out = without_block(text);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str("\r\n");
    }
    out.push_str(START);
    out.push_str("\r\n");
    for h in HOSTS {
        out.push_str(&format!("0.0.0.0 {h}\r\n"));
    }
    out.push_str(END);
    out.push_str("\r\n");
    out
}

pub fn is_blocked() -> bool {
    std::fs::read_to_string(hosts_path()).is_ok_and(|t| t.contains(START))
}

/// Adds or removes the block (needs administrator rights), then clears Windows' DNS cache so it applies now.
pub fn set_blocked(on: bool) -> Result<(), String> {
    let path = hosts_path();
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read the hosts file: {e}"))?;
    let new = if on { with_block(&text) } else { without_block(&text) };
    if new != text {
        std::fs::write(&path, new).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                "Changing the hosts file needs administrator rights (Settings > Restart as administrator).".to_string()
            } else {
                format!("Couldn't change the hosts file: {e}")
            }
        })?;
    }
    let _ = run_hidden("ipconfig.exe", &["/flushdns"]);
    log(&format!("NVIDIA telemetry block {}", if on { "added to" } else { "removed from" }));
    Ok(())
}

/// Settings > Restore everything: takes the block out again.
pub fn restore() -> Vec<String> {
    if !is_blocked() {
        return Vec::new();
    }
    vec![match set_blocked(false) {
        Ok(()) => "✔ Restored: NVIDIA telemetry is no longer blocked".into(),
        Err(e) => format!("✖ NVIDIA telemetry block: {e}"),
    }]
}

/// What's true right now.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub blocked: bool,
    /// Telemetry hosts that still resolve to a real address.
    pub reachable: Vec<String>,
    /// Remote addresses NVIDIA's display container is connected to now.
    pub connections: Vec<String>,
    pub container_running: bool,
}

pub fn check() -> Status {
    let reachable = HOSTS
        .iter()
        .filter(|h| (**h, 443).to_socket_addrs().is_ok_and(|mut a| a.any(|a| !a.ip().is_unspecified() && !a.ip().is_loopback())))
        .map(|h| h.to_string())
        .collect();
    let pids: Vec<u32> = process_list().into_iter().filter(|(_, n)| n == "nvdisplay.container.exe").map(|(p, _)| p).collect();
    Status { blocked: is_blocked(), reachable, connections: connections(&pids), container_running: !pids.is_empty() }
}

/// Established IPv4 TCP connections of these processes to non-local addresses.
/// shortcut: IPv4 only; add the IPv6 table if NVIDIA's endpoints ever resolve to IPv6 first.
fn connections(pids: &[u32]) -> Vec<String> {
    use windows::Win32::NetworkManagement::IpHelper::{GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS};
    use windows::Win32::Networking::WinSock::AF_INET;
    if pids.is_empty() {
        return Vec::new();
    }
    let mut size = 0u32;
    unsafe {
        let _ = GetExtendedTcpTable(None, &mut size, false, AF_INET.0 as u32, TCP_TABLE_OWNER_PID_CONNECTIONS, 0);
        let mut buf = vec![0u64; (size as usize).div_ceil(8) + 1];
        if GetExtendedTcpTable(Some(buf.as_mut_ptr().cast()), &mut size, false, AF_INET.0 as u32, TCP_TABLE_OWNER_PID_CONNECTIONS, 0) != 0 {
            return Vec::new();
        }
        let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        rows.iter()
            .filter(|r| pids.contains(&r.dwOwningPid))
            .filter_map(|r| {
                let ip = std::net::Ipv4Addr::from(u32::from_be(r.dwRemoteAddr));
                let port = u16::from_be(r.dwRemotePort as u16);
                (!ip.is_loopback() && !ip.is_unspecified() && !ip.is_private()).then(|| format!("{ip}:{port}"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_round_trip() {
        let hosts = "# Copyright\r\n127.0.0.1 localhost\r\n";
        let on = with_block(hosts);
        assert!(on.starts_with(hosts) && on.contains("0.0.0.0 events.telemetry.data.nvidia.com\r\n") && on.contains(END));
        assert_eq!(with_block(&on), on, "adding twice changes nothing");
        assert_eq!(without_block(&on), hosts);
        assert_eq!(with_block("no newline"), format!("no newline\r\n{}", with_block("")));
    }

    #[test]
    #[ignore = "reads the hosts file, DNS and this PC's connections"]
    fn live_check() {
        println!("{:?}", check());
    }
}
