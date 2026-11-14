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

pub fn log(msg: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(history_path()) {
        let _ = writeln!(f, "[{}] {}", local_time(), msg);
    }
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
