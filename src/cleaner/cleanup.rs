//! Deletes exactly the files a scan found (and the user approved).

use std::ffi::c_void;
use std::fs;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{BOOLEAN, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FileDispositionInfo, FileDispositionInfoEx, GetFinalPathNameByHandleW, SetFileInformationByHandle, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_DISPOSITION_INFO_EX_FLAGS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED,
};

use super::scanner::{Category, Kind, Scanned, REPARSE_POINT};
use crate::system::{empty_recycle_bin, env_path, powershell, run_hidden};
use crate::ui::fmt_bytes;

const DELETE: u32 = 0x0001_0000;
const FILE_READ_ATTRIBUTES: u32 = 0x0080;

/// True when `path` is `root` or lies under it (both as returned by the OS, compared case-insensitively).
fn inside(path: &Path, root: &Path) -> bool {
    let (p, r) = (path.to_string_lossy().to_lowercase(), root.to_string_lossy().to_lowercase());
    let r = r.trim_end_matches('\\');
    p == r || p.strip_prefix(r).is_some_and(|rest| rest.starts_with('\\'))
}

/// Deletes `path` through one open handle, and only if that handle is a plain file inside a scanned root.
/// Checking and deleting the same handle means a folder swapped for a junction after the scan can't
/// redirect the delete elsewhere, which matters when Kestrel runs as administrator.
fn delete_within(path: &Path, roots: &[PathBuf]) -> Option<u64> {
    let file =
        fs::OpenOptions::new().access_mode(DELETE | FILE_READ_ATTRIBUTES).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0).open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.file_attributes() & REPARSE_POINT != 0 {
        return None;
    }
    let h = HANDLE(file.as_raw_handle());
    let mut buf = vec![0u16; 2048];
    let n = unsafe { GetFinalPathNameByHandleW(h, &mut buf, FILE_NAME_NORMALIZED) } as usize;
    if n == 0 || n > buf.len() {
        return None;
    }
    let real = PathBuf::from(String::from_utf16_lossy(&buf[..n]));
    if !roots.iter().any(|r| inside(&real, r)) {
        return None;
    }
    let ex = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_INFO_EX_FLAGS(FILE_DISPOSITION_FLAG_DELETE.0 | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0),
    };
    let size = |v: &dyn std::any::Any| std::mem::size_of_val(v) as u32;
    let deleted = unsafe { SetFileInformationByHandle(h, FileDispositionInfoEx, &ex as *const _ as *const c_void, size(&ex)) }.is_ok() || {
        // File systems without the extended call (FAT32, some network shares).
        let old = FILE_DISPOSITION_INFO { DeleteFile: BOOLEAN(1) };
        unsafe { SetFileInformationByHandle(h, FileDispositionInfo, &old as *const _ as *const c_void, size(&old)) }.is_ok()
    };
    deleted.then_some(meta.len())
}

pub struct Report {
    pub name: &'static str,
    pub deleted: u64,
    pub freed: u64,
    pub skipped: u64,
    pub error: Option<String>,
}

impl Report {
    pub fn summary(&self) -> String {
        match &self.error {
            Some(e) => format!("{}: failed — {e}", self.name),
            None if self.skipped > 0 => format!(
                "{}: removed {} files ({}), {} in use or protected were skipped",
                self.name,
                self.deleted,
                fmt_bytes(self.freed),
                self.skipped
            ),
            // Measured cleanups (DISM) free space without a file count.
            None if self.deleted == 0 && self.freed > 0 => format!("{}: freed {}", self.name, fmt_bytes(self.freed)),
            None => format!("{}: removed {} items ({})", self.name, self.deleted, fmt_bytes(self.freed)),
        }
    }
}

pub fn clean(cat: &Category, found: &Scanned) -> Report {
    let mut r = Report { name: cat.name, deleted: 0, freed: 0, skipped: 0, error: None };
    match cat.kind {
        Kind::Files => {
            for path in &found.files {
                if !path.exists() {
                    continue; // already gone since the scan
                }
                // Files in use, read-only files, links and anything outside the scanned folders are left alone.
                match delete_within(path, &found.roots) {
                    Some(len) => {
                        r.deleted += 1;
                        r.freed += len;
                    }
                    None => r.skipped += 1,
                }
            }
        }
        Kind::RecycleBin if found.count == 0 => {}
        Kind::RecycleBin => match empty_recycle_bin() {
            Ok(()) => (r.deleted, r.freed) = (found.count, found.bytes),
            Err(e) => r.error = Some(e),
        },
        Kind::DeliveryOptimization => match powershell("Delete-DeliveryOptimizationCache -Force") {
            Ok(_) => (r.deleted, r.freed) = (found.count, found.bytes),
            Err(e) => r.error = Some(e),
        },
        Kind::PreviousWindows if found.count == 0 => {}
        Kind::PreviousWindows => match disk_cleanup("Previous Installations") {
            Ok(()) if found.roots.iter().all(|p| !p.exists()) => (r.deleted, r.freed) = (found.count, found.bytes),
            Ok(()) => r.error = Some("Disk Cleanup finished, but Windows.old is still there.".into()),
            Err(e) => r.error = Some(e),
        },
        Kind::ComponentStore if found.count == 0 => {}
        Kind::ComponentStore => {
            let before = free_space();
            match run_hidden("dism.exe", &["/Online", "/Cleanup-Image", "/StartComponentCleanup", "/English"]) {
                Ok(_) => r.freed = free_space().saturating_sub(before),
                Err(e) => r.error = Some(e.lines().rev().find(|l| l.contains("Error") || l.contains(':')).unwrap_or(&e).trim().to_string()),
            }
        }
    }
    r
}

/// Free bytes on the system drive.
fn free_space() -> u64 {
    let root = crate::system::wide(&format!("{}\\", env_path("SystemDrive").display()));
    let mut free = 0u64;
    let _ = unsafe {
        windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(windows::core::PCWSTR(root.as_ptr()), Some(&mut free), None, None)
    };
    free
}

/// Runs one of Disk Cleanup's own handlers silently (cleanmgr /sagerun), the supported way to remove what it manages.
/// The handler is ticked under a Kestrel-only preset number and unticked again afterwards.
fn disk_cleanup(handler: &str) -> Result<(), String> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_SET_VALUE};
    const PRESET: &str = "StateFlags0042";
    let key = winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches\{handler}"), KEY_SET_VALUE)
        .map_err(|e| format!("Disk Cleanup isn't available ({e})."))?;
    key.set_value(PRESET, &2u32).map_err(|e| e.to_string())?;
    let result = std::process::Command::new("cleanmgr.exe").arg("/sagerun:42").status();
    let _ = key.delete_value(PRESET);
    match result {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("Disk Cleanup stopped with code {}.", s.code().unwrap_or(-1))),
        Err(e) => Err(format!("Couldn't start Disk Cleanup: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleaner::scanner::Source;

    #[test]
    fn deletes_only_listed_files() {
        let dir = std::env::temp_dir().join("kestrel_cleanup_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (gone, kept) = (dir.join("a.tmp"), dir.join("b.tmp"));
        fs::write(&gone, b"1234").unwrap();
        fs::write(&kept, b"5678").unwrap();
        let cat = Category { name: "t", desc: "", warning: None, default_on: true, kind: Kind::Files, sources: Vec::<Source>::new() };
        let roots = vec![fs::canonicalize(&dir).unwrap()];
        let found = Scanned { files: vec![gone.clone()], bytes: 4, count: 1, notes: vec![], roots };
        let r = clean(&cat, &found);
        assert_eq!((r.deleted, r.freed, r.skipped), (1, 4, 0));
        assert!(!gone.exists() && kept.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A folder swapped for a junction after the scan must not redirect the delete.
    #[test]
    fn refuses_junction_redirect() {
        let base = std::env::temp_dir().join("kestrel_junction_test");
        let _ = fs::remove_dir_all(&base);
        let (scanned, outside) = (base.join("scanned"), base.join("outside"));
        fs::create_dir_all(&scanned).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let victim = outside.join("important.txt");
        fs::write(&victim, b"keep me").unwrap();
        let link = scanned.join("sub");
        let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&outside).output().unwrap().status.success();
        assert!(ok, "could not create junction");
        let cat = Category { name: "t", desc: "", warning: None, default_on: true, kind: Kind::Files, sources: Vec::<Source>::new() };
        let found = Scanned {
            files: vec![link.join("important.txt")],
            bytes: 7,
            count: 1,
            notes: vec![],
            roots: vec![fs::canonicalize(&scanned).unwrap()],
        };
        let r = clean(&cat, &found);
        assert_eq!((r.deleted, r.skipped), (0, 1));
        assert!(victim.exists());
        let _ = fs::remove_dir(&link);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn inside_root() {
        let root = Path::new(r"\\?\C:\Windows\Temp");
        assert!(inside(Path::new(r"\\?\C:\WINDOWS\temp\a.tmp"), root));
        assert!(!inside(Path::new(r"\\?\C:\Windows\TempX\a.tmp"), root));
        assert!(!inside(Path::new(r"\\?\C:\Windows\System32\a.dll"), root));
    }
}
