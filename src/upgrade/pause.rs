//! Pausing Windows Update, the way Settings > Windows Update > Pause updates does it: the same values in the same
//! key, so Settings shows the pause and its Resume button undoes it. Windows itself caps a pause at 5 weeks.

use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE};
use winreg::RegKey;

use super::drivers::date_from_unix_days;
use crate::utils::log;

const KEY: &str = r"SOFTWARE\Microsoft\WindowsUpdate\UX\Settings";
const EXPIRY: &str = "PauseUpdatesExpiryTime";
const VALUES: [&str; 6] = [
    EXPIRY,
    "PauseUpdatesStartTime",
    "PauseFeatureUpdatesStartTime",
    "PauseFeatureUpdatesEndTime",
    "PauseQualityUpdatesStartTime",
    "PauseQualityUpdatesEndTime",
];
pub const MAX_DAYS: u64 = 35;

/// UTC time as Windows Update stores it, e.g. "2026-11-27T10:05:00Z". Sorts like the time it names.
fn iso(unix_secs: u64) -> String {
    let (days, rest) = ((unix_secs / 86_400) as i64, unix_secs % 86_400);
    format!("{}T{:02}:{:02}:{:02}Z", date_from_unix_days(days), rest / 3600, rest % 3600 / 60, rest % 60)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The date updates resume (YYYY-MM-DD, UTC), if they're paused now.
pub fn paused_until() -> Option<String> {
    let k = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(KEY, KEY_READ).ok()?;
    let until: String = k.get_value(EXPIRY).ok()?;
    (until.len() >= 10 && until > iso(now())).then(|| until[..10].to_string())
}

/// Pauses all updates for `days` (1–35). Needs administrator rights.
pub fn pause(days: u64) -> Result<String, String> {
    let days = days.clamp(1, MAX_DAYS);
    let (start, end) = (iso(now()), iso(now() + days * 86_400));
    let k = open_for_writing()?;
    for name in VALUES {
        let v = if name.ends_with("StartTime") { &start } else { &end };
        k.set_value(name, v).map_err(|e| format!("Couldn't pause updates: {e}"))?;
    }
    let msg = format!("Windows Update paused until {} (UTC).", &end[..10]);
    log(&msg);
    Ok(msg)
}

/// Ends a pause (also one set in Settings). Windows then checks for updates on its normal schedule.
pub fn resume() -> Result<String, String> {
    let k = open_for_writing()?;
    for name in VALUES {
        let _ = k.delete_value(name);
    }
    log("Windows Update resumed");
    Ok("Windows Update resumed. Windows checks for updates on its normal schedule again.".into())
}

fn open_for_writing() -> Result<RegKey, String> {
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(KEY, KEY_SET_VALUE).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            "Pausing Windows Update needs administrator rights (Restart as administrator).".to_string()
        } else {
            format!("Couldn't open the Windows Update settings: {e}")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_times() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_795_000_000), "2026-11-18T11:06:40Z");
        // Lexicographic order is time order, which paused_until relies on.
        assert!(iso(1_795_000_000) > iso(1_794_999_999) && iso(1_800_000_000) > iso(1_795_000_000));
    }
}
