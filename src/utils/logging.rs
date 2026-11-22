//! Activity history: a plain text log in %LOCALAPPDATA%\Kestrel.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::system::{env_path, local_time};

pub fn data_dir() -> PathBuf {
    let dir = env_path("LOCALAPPDATA").join("Kestrel");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn history_path() -> PathBuf {
    data_dir().join("history.log")
}

/// The log is trimmed to its newest `KEEP_LINES` lines once it passes `MAX_BYTES`.
const MAX_BYTES: u64 = 1024 * 1024;
const KEEP_LINES: usize = 2000;

pub fn log(msg: &str) {
    // Tests exercise real code paths (updates, cleanup) on temporary files; keep them out of the user's history.
    if cfg!(test) {
        return;
    }
    let path = history_path();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", local_time(), msg);
    }
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        if let Ok(text) = fs::read_to_string(&path) {
            let _ = super::write_atomic(&path, trim(&text).as_bytes());
        }
    }
}

/// The newest `KEEP_LINES` lines of the log.
fn trim(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = lines[lines.len().saturating_sub(KEEP_LINES)..].join("\n");
    out.push('\n');
    out
}

/// Newest entries first.
pub fn read_history() -> String {
    let text = fs::read_to_string(history_path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().rev().take(500).collect();
    if lines.is_empty() {
        "No activity yet.".into()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn trim_keeps_newest_lines() {
        let text: String = (0..2500).map(|i| format!("line {i}\n")).collect();
        let t = super::trim(&text);
        assert_eq!(t.lines().count(), super::KEEP_LINES);
        assert!(t.starts_with("line 500\n") && t.ends_with("line 2499\n"));
        assert_eq!(super::trim("a\nb\n"), "a\nb\n");
    }
}
