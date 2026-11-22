//! Kestrel's own opt-in "Start with Windows": a per-user Run value (the same kind Task Manager > Startup apps
//! lists, no administrator rights), started with `--startup` so Kestrel begins minimized. Off unless the user
//! turns it on; turning it off deletes the value.

use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
use winreg::RegKey;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "Kestrel";

fn command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't find Kestrel's own location: {e}"))?;
    Ok(format!("\"{}\" {}", exe.display(), crate::system::START_ARG))
}

/// StartupApproved values start with an even byte when the entry is enabled and an odd one when Task Manager
/// (or Settings > Apps > Startup) switched it off.
fn switched_off(approved: &[u8]) -> bool {
    approved.first().is_some_and(|b| b & 1 == 1)
}

/// Kestrel starts with Windows from this exact copy of the exe, and Windows hasn't been told to skip it.
pub fn is_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(want) = command() else { return false };
    let set = hkcu.open_subkey(RUN).and_then(|k| k.get_value::<String, _>(NAME)).is_ok_and(|v| v.eq_ignore_ascii_case(&want));
    let off = hkcu.open_subkey(APPROVED).and_then(|k| k.get_raw_value(NAME)).is_ok_and(|v| switched_off(&v.bytes));
    set && !off
}

pub fn set(on: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let not_found = |e: std::io::Error| (e.kind() != std::io::ErrorKind::NotFound).then(|| e.to_string());
    if on {
        let (run, _) = hkcu.create_subkey(RUN).map_err(|e| e.to_string())?;
        run.set_value(NAME, &command()?).map_err(|e| format!("Couldn't turn on Start with Windows: {e}"))?;
        // A switch Task Manager left behind would keep Windows from starting it.
        if let Ok(k) = hkcu.open_subkey_with_flags(APPROVED, KEY_SET_VALUE) {
            if let Err(e) = k.delete_value(NAME) {
                return not_found(e).map_or(Ok(()), Err);
            }
        }
        Ok(())
    } else {
        let run = hkcu.open_subkey_with_flags(RUN, KEY_SET_VALUE).map_err(|e| e.to_string())?;
        run.delete_value(NAME).or_else(|e| not_found(e).map_or(Ok(()), Err))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_is_quoted_and_marked_as_startup() {
        let c = super::command().unwrap();
        assert!(c.starts_with('"') && c.ends_with(" --startup"), "{c}");
    }

    #[test]
    fn task_manager_switch_is_read() {
        assert!(!super::switched_off(&[0x02, 0, 0, 0]));
        assert!(!super::switched_off(&[0x06, 0, 0, 0]));
        assert!(super::switched_off(&[0x03, 0, 0, 0]));
        assert!(!super::switched_off(&[]));
    }

    #[test]
    #[ignore = "writes and removes this user's Run value for Kestrel, then restores it"]
    fn live_round_trip() {
        let before = super::is_enabled();
        super::set(true).unwrap();
        assert!(super::is_enabled());
        super::set(false).unwrap();
        assert!(!super::is_enabled());
        super::set(before).unwrap();
    }
}
