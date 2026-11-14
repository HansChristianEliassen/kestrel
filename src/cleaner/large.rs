//! Largest files in the user's own folders. Read-only: Kestrel only lists them and opens their folder.

use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::scanner::REPARSE_POINT;
use crate::system::env_path;

pub const MIN_SIZE: u64 = 250 * 1024 * 1024;
const SHOWN: usize = 100;

/// Files of at least `MIN_SIZE` in Downloads, Desktop, Documents, Videos, Pictures and Music, largest first.
pub fn find() -> Vec<(PathBuf, u64)> {
    let home = env_path("USERPROFILE");
    let mut found = Vec::new();
    let mut budget = 300_000; // entries; enough for normal profiles without hanging on huge photo libraries
    for d in ["Downloads", "Desktop", "Documents", "Videos", "Pictures", "Music"] {
        if home.is_absolute() {
            walk(&home.join(d), MIN_SIZE, &mut budget, &mut found);
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.1));
    found.truncate(SHOWN);
    found
}

fn walk(dir: &Path, min: u64, budget: &mut u32, out: &mut Vec<(PathBuf, u64)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(meta) = e.metadata() else { continue };
        // Never follow links (OneDrive placeholders and junctions can point anywhere).
        if meta.file_attributes() & REPARSE_POINT != 0 {
            continue;
        }
        if meta.is_dir() {
            walk(&e.path(), min, budget, out);
        } else if meta.len() >= min {
            out.push((e.path(), meta.len()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_big_files() {
        let dir = std::env::temp_dir().join("kestrel_large_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("small.bin"), vec![0u8; 10]).unwrap();
        fs::write(dir.join("sub").join("big.bin"), vec![0u8; 100]).unwrap();
        let (mut out, mut budget) = (Vec::new(), 1000);
        walk(&dir, 50, &mut budget, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].0.ends_with("big.bin"));
        let _ = fs::remove_dir_all(&dir);
    }
}
