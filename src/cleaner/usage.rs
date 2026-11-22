//! Disk usage: how much each folder (and file) directly inside a folder takes, largest first. Read-only.
//! Links and junctions are never followed, so nothing is counted twice and the walk can't loop.

use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::scanner::REPARSE_POINT;

/// One entry: (path, bytes, is a folder).
pub type Entry = (PathBuf, u64, bool);

/// Total size of the files under the folder `path`; unreadable folders count as empty.
/// Uses the size and attributes the directory listing already returns (no extra call per file).
fn dir_size(path: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(path) else { return 0 };
    rd.flatten()
        .filter_map(|e| e.metadata().ok().map(|m| (e, m)))
        .filter(|(_, m)| m.file_attributes() & REPARSE_POINT == 0)
        .map(|(e, m)| if m.is_dir() { dir_size(&e.path()) } else { m.len() })
        .sum()
}

/// The entries directly inside `dir` with their sizes, largest first. Folders are sized on up to 8 threads.
pub fn list(dir: &Path) -> Vec<Entry> {
    let children: Vec<PathBuf> = fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    let threads = 8.min(children.len().max(1));
    let mut out: Vec<Entry> = std::thread::scope(|s| {
        let chunks: Vec<_> = (0..threads)
            .map(|t| {
                let mine: Vec<&PathBuf> = children.iter().skip(t).step_by(threads).collect();
                s.spawn(move || {
                    mine.into_iter()
                        .filter_map(|p| {
                            let meta = fs::symlink_metadata(p).ok()?;
                            let linked = meta.file_attributes() & REPARSE_POINT != 0;
                            (!linked).then(|| (p.clone(), if meta.is_dir() { dir_size(p) } else { meta.len() }, meta.is_dir()))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        chunks.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });
    out.sort_by_key(|e| std::cmp::Reverse(e.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_folders_largest_first() {
        let base = std::env::temp_dir().join("kestrel_usage_test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(r"big\deeper")).unwrap();
        fs::create_dir_all(base.join("small")).unwrap();
        fs::write(base.join(r"big\a.bin"), vec![0u8; 3000]).unwrap();
        fs::write(base.join(r"big\deeper\b.bin"), vec![0u8; 2000]).unwrap();
        fs::write(base.join(r"small\c.bin"), vec![0u8; 100]).unwrap();
        fs::write(base.join("file.bin"), vec![0u8; 700]).unwrap();
        // A junction back to the parent must not be followed (it would loop and double count).
        let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(base.join(r"big\loop")).arg(&base).output().unwrap();
        assert!(ok.status.success());
        let got: Vec<(String, u64, bool)> =
            list(&base).into_iter().map(|(p, n, d)| (p.file_name().unwrap().to_string_lossy().into_owned(), n, d)).collect();
        assert_eq!(got, [("big".into(), 5000, true), ("file.bin".into(), 700, false), ("small".into(), 100, true)]);
        let _ = fs::remove_dir(base.join(r"big\loop"));
        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod live {
    #[test]
    #[ignore = "walks this PC's user folder"]
    fn time_user_folder() {
        let t = std::time::Instant::now();
        let list = super::list(&crate::system::env_path("USERPROFILE"));
        eprintln!(
            "{} entries in {:?}; top: {:?}",
            list.len(),
            t.elapsed(),
            list.iter().take(3).map(|e| (&e.0, e.1 >> 20)).collect::<Vec<_>>()
        );
    }
}
