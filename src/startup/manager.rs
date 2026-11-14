//! Startup entries from the Run keys, Startup folders and sign-in/boot scheduled tasks.
//! Run keys and folders are switched with the same StartupApproved flags Task Manager uses;
//! tasks with Task Scheduler's own Enabled switch. The original entry is never modified or deleted.

use std::fs;

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_BINARY};
use winreg::{RegKey, RegValue};

use super::tasks;
use crate::system::{env_path, expand_env, file_company, shortcut_target, split_command};
use crate::utils::{backup, log};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

const SECURITY: &[&str] = &[
    "securityhealth",
    "defender",
    "msmpeng",
    "malwarebytes",
    "eset",
    "kaspersky",
    "bitdefender",
    "norton",
    "avast",
    "mcafee",
    "sophos",
    "avira",
    "sentinel",
    "crowdstrike",
    "trend micro",
    "webroot",
];

#[derive(Clone)]
pub struct StartupEntry {
    pub name: String,
    pub command: String,
    pub publisher: String,
    pub location: &'static str,
    pub machine: bool,
    pub enabled: bool,
    pub protected: bool,
    kind: Kind,
}

#[derive(Clone)]
enum Kind {
    /// StartupApproved subkey ("Run", "Run32", "StartupFolder") and value name.
    Approved {
        key: &'static str,
        value: String,
    },
    Task {
        path: String,
    },
}

fn hive(machine: bool) -> RegKey {
    RegKey::predef(if machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER })
}

fn is_enabled(machine: bool, approved_key: &str, value: &str) -> bool {
    hive(machine)
        .open_subkey(format!(r"{APPROVED}\{approved_key}"))
        .and_then(|k| k.get_raw_value(value))
        // Even first byte = enabled (02/06), odd = disabled (03/07); no flag = enabled.
        .map_or(true, |v| v.bytes.first().is_none_or(|b| b & 1 == 0))
}

fn entry(name: String, command: String, location: &'static str, machine: bool, enabled: bool, kind: Kind) -> StartupEntry {
    let (exe, _) = split_command(&command);
    let mut exe = expand_env(&exe);
    // Startup-folder shortcuts: read the publisher from the program the shortcut opens.
    if exe.to_ascii_lowercase().ends_with(".lnk") {
        exe = shortcut_target(&exe).unwrap_or_default();
    }
    let publisher = if exe.to_ascii_lowercase().ends_with(".exe") { file_company(&exe).unwrap_or_default() } else { String::new() };
    let haystack = format!("{name} {command} {exe} {publisher}").to_lowercase();
    StartupEntry { enabled, protected: SECURITY.iter().any(|s| haystack.contains(s)), name, command, publisher, location, machine, kind }
}

fn approved(machine: bool, key: &'static str, value: String) -> (bool, Kind) {
    (is_enabled(machine, key, &value), Kind::Approved { key, value })
}

pub fn list() -> Vec<StartupEntry> {
    let mut out = Vec::new();
    for (machine, run, approved, location) in [
        (false, RUN, "Run", "Registry · current user"),
        (true, RUN, "Run", "Registry · all users"),
        (true, RUN32, "Run32", "Registry · all users (32-bit)"),
    ] {
        let Ok(k) = hive(machine).open_subkey(run) else { continue };
        for (name, _) in k.enum_values().flatten() {
            let Ok(command) = k.get_value::<String, _>(&name) else { continue };
            let (enabled, kind) = self::approved(machine, approved, name.clone());
            out.push(entry(name, command, location, machine, enabled, kind));
        }
    }
    let startup = r"Microsoft\Windows\Start Menu\Programs\Startup";
    for (machine, dir, location) in [
        (false, env_path("APPDATA").join(startup), "Startup folder · current user"),
        (true, env_path("ProgramData").join(startup), "Startup folder · all users"),
    ] {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            if file.eq_ignore_ascii_case("desktop.ini") || !e.path().is_file() {
                continue;
            }
            let name = file.strip_suffix(".lnk").unwrap_or(&file).to_string();
            let (enabled, kind) = approved(machine, "StartupFolder", file.clone());
            out.push(entry(name, e.path().to_string_lossy().into_owned(), location, machine, enabled, kind));
        }
    }
    for t in tasks::list() {
        out.push(entry(t.name, t.command, "Scheduled task", !t.mine, t.enabled, Kind::Task { path: t.path }));
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

fn approved_key(machine: bool, sub: &str) -> Result<RegKey, String> {
    hive(machine).create_subkey(format!(r"{APPROVED}\{sub}")).map(|(k, _)| k).map_err(|err| {
        if machine {
            "Changing all-users startup entries needs administrator rights (Settings > Restart as administrator).".into()
        } else {
            err.to_string()
        }
    })
}

fn backup_key(machine: bool, sub: &str, value: &str) -> String {
    format!("startup:{}|{sub}|{value}", if machine { "M" } else { "U" })
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    s.len().is_multiple_of(2).then(|| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect())?
}

/// Puts back every startup flag Kestrel changed. One line per entry.
pub fn restore_all() -> Vec<String> {
    let mut lines = Vec::new();
    for (id, original) in backup::with_prefix("startup:") {
        let mut parts = id.splitn(3, '|');
        let (Some(scope), Some(sub), Some(value)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let machine = scope == "M";
        let result = approved_key(machine, sub).and_then(|k| {
            match original.as_str() {
                "-" => match k.delete_value(value) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                    _ => Ok(()),
                },
                hex => k.set_raw_value(value, &RegValue { bytes: from_hex(hex).unwrap_or_default(), vtype: REG_BINARY }),
            }
            .map_err(|e| e.to_string())
        });
        match result.and_then(|()| backup::remove(&format!("startup:{id}"))) {
            Ok(()) => lines.push(format!("✔ Restored startup entry: {value}")),
            Err(e) => lines.push(format!("✖ Startup entry {value}: {e}")),
        }
    }
    for (path, original) in backup::with_prefix("task:") {
        let result = tasks::set_enabled(&path, original == "1").and_then(|()| backup::remove(&format!("task:{path}")));
        lines.push(match result {
            Ok(()) => format!("✔ Restored scheduled task: {path}"),
            Err(e) => format!("✖ Scheduled task {path}: {e}"),
        });
    }
    if !lines.is_empty() {
        log(&format!("Startup: restored {} original entries", lines.len()));
    }
    lines
}

pub fn set_enabled(e: &StartupEntry, enabled: bool) -> Result<(), String> {
    if e.protected && !enabled {
        return Err("Security software is never disabled by Kestrel.".into());
    }
    match &e.kind {
        Kind::Task { path } => {
            backup::save_original(&format!("task:{path}"), || tasks::is_enabled(path).map(|on| if on { "1" } else { "0" }.into()))?;
            tasks::set_enabled(path, enabled)?;
        }
        Kind::Approved { key: sub, value } => {
            let key = approved_key(e.machine, sub)?;
            backup::save_original(&backup_key(e.machine, sub, value), || {
                Ok(key.get_raw_value(value).map_or("-".into(), |v| to_hex(&v.bytes)))
            })?;
            let mut bytes = vec![0u8; 12];
            bytes[0] = if enabled { 2 } else { 3 };
            key.set_raw_value(value, &RegValue { bytes, vtype: REG_BINARY }).map_err(|err| err.to_string())?;
        }
    }
    log(&format!(
        "Startup: {} '{}' ({}). Re-{} it on the Startup page or in Task Manager.",
        if enabled { "enabled" } else { "disabled" },
        e.name,
        e.location,
        if enabled { "disable" } else { "enable" },
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let b = vec![3u8, 0, 0, 0, 0xab, 0xcd];
        assert_eq!(from_hex(&to_hex(&b)), Some(b));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
    }
}
