//! Opt-in updates from GitHub Releases.
//!
//! Check: github.com/<repo>/releases/latest redirects to the newest stable release's tag; drafts and prereleases
//! are never "latest". No API token, no JSON, and nothing sent besides the request itself.
//!
//! Install: the release's `kestrel-update.sig` is downloaded first, and its ECDSA P-256 signature is checked
//! against the public key built into this exe. It names the version and the exe's SHA-256, so the exe is only
//! written to disk if it matches what the release key signed. Trust comes from that key, not from GitHub alone.
//! Windows lets a running exe be renamed but not overwritten, so the old exe becomes `kestrel.exe.old`, the
//! verified one takes its name, and the new version starts once this one has closed. Nothing is elevated.

pub mod crypto;
pub mod http;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::utils::{data_dir, log};

pub const REPO: &str = env!("CARGO_PKG_REPOSITORY");
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const ASSET: &str = "kestrel.exe";
const SIGNATURE: &str = "kestrel-update.sig";
const MAX_EXE: u64 = 64 * 1024 * 1024;
/// BCRYPT_ECCPUBLIC_BLOB of the release signing key (private half: `%USERPROFILE%\.kestrel\release-signing.key`).
const PUBLIC_KEY: &str = "45435331200000007be43dea6670e22b1231265db2edd1033c5212c078460e93c886fdb06285411f131253c344259c14527cf9071881c38d749be46d0e30efc85bdf69c43ed35a4a";
/// Automatic checks run at most this often.
pub const CHECK_INTERVAL: u64 = 24 * 60 * 60;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// "v1.2.3" or "1.2.3" as numbers. Anything else, prerelease suffixes included, is rejected.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u32>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

pub fn is_newer(candidate: &str, than: &str) -> bool {
    matches!((parse_version(candidate), parse_version(than)), (Some(a), Some(b)) if a > b)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub tag: String,
}

impl Release {
    pub fn notes_url(&self) -> String {
        format!("{REPO}/releases/tag/{}", self.tag)
    }
}

/// The tag a /releases/latest redirect points to.
fn tag_from_location(location: &str) -> Option<&str> {
    location.split_once("/releases/tag/").map(|(_, tag)| tag.trim_end_matches('/')).filter(|t| !t.is_empty() && !t.contains('/'))
}

/// The newest stable release, if it's newer than this build.
pub fn latest() -> Result<Option<Release>, String> {
    let (status, location) = http::redirect(&format!("{REPO}/releases/latest"))?;
    let location = match status {
        301 | 302 | 303 | 307 | 308 => location.ok_or("GitHub's answer had no release link.")?,
        404 => return Err(format!("No published releases were found at {REPO} (the repository may be private).")),
        403 | 429 => return Err("GitHub is limiting requests right now. Kestrel will try again later.".into()),
        s => return Err(format!("GitHub answered with status {s}.")),
    };
    let tag = tag_from_location(&location).ok_or("GitHub's release link wasn't in the expected format.")?;
    let version = tag.trim_start_matches('v');
    if parse_version(version).is_none() {
        return Err(format!("The latest release has an unexpected version name ({tag})."));
    }
    Ok(is_newer(version, CURRENT).then(|| Release { version: version.to_string(), tag: tag.to_string() }))
}

/// Checks a signature file against key_hex (the built-in key in real use); returns the signed SHA-256 (hex) of the exe.
fn verify_signature(text: &str, version: &str, key_hex: &str) -> Result<String, String> {
    let field = |name: &str| text.lines().find_map(|l| l.strip_prefix(name)?.strip_prefix('=')).map(str::trim);
    let (Some(signed_version), Some(sha), Some(sig)) = (field("version"), field("sha256"), field("signature")) else {
        return Err("The update's signature file is incomplete.".into());
    };
    let sig = crypto::unhex(sig).ok_or("The update's signature is malformed.")?;
    let key = crypto::unhex(key_hex).ok_or("Built-in key is malformed.")?;
    if !crypto::verify(&key, crypto::message(signed_version, sha).as_bytes(), &sig) {
        return Err("The update's signature is not valid, so it was not installed.".into());
    }
    if signed_version != version {
        return Err(format!("The signed version ({signed_version}) doesn't match the release ({version}), so it was not installed."));
    }
    if !is_newer(signed_version, CURRENT) {
        return Err("The update isn't newer than this version, so it was not installed.".into());
    }
    Ok(sha.to_ascii_lowercase())
}

/// A 64-bit Windows program: "MZ" header pointing to a "PE" header for x86-64, and a plausible size.
fn looks_like_exe(data: &[u8]) -> bool {
    let pe = data.get(0x3c..0x40).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    data.len() >= 512 * 1024
        && data.starts_with(b"MZ")
        && pe.and_then(|o| data.get(o..o + 6)).is_some_and(|h| h[..4] == *b"PE\0\0" && h[4..6] == [0x64, 0x86])
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    exe.with_file_name(name)
}

/// Downloads and verifies the release. Returns the verified exe, saved next to the running one as `<exe>.new`.
pub fn download(release: &Release, done: &AtomicU64, total: &AtomicU64) -> Result<PathBuf, String> {
    let base = format!("{REPO}/releases/download/{}", release.tag);
    let sig = http::download(&format!("{base}/{SIGNATURE}"), 16 * 1024, done, total).map_err(|e| {
        if e.contains("wasn't found") {
            "This release has no signature file, so it can't be installed automatically. Download it from the release page instead.".into()
        } else {
            e
        }
    })?;
    let sha = verify_signature(&String::from_utf8_lossy(&sig), &release.version, PUBLIC_KEY)?;
    let exe = http::download(&format!("{base}/{ASSET}"), MAX_EXE, done, total)?;
    if crypto::hex(&crypto::sha256(&exe)?) != sha {
        return Err("The downloaded file doesn't match the signed release, so it was not installed.".into());
    }
    // Defence in depth: whatever was signed, only a real Windows program may replace kestrel.exe.
    if !looks_like_exe(&exe) {
        return Err("The update isn't a Windows program, so it was not installed.".into());
    }
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    let staged = sibling(&target, ".new");
    fs::write(&staged, &exe)
        .map_err(|e| format!("Couldn't save the update next to Kestrel ({e}). Download it from the release page instead."))?;
    log(&format!("Update: downloaded and verified Kestrel {}", release.version));
    Ok(staged)
}

/// Puts the verified exe in place of `target`, keeping the old one as `<exe>.old` until the new version has
/// started. On failure the original is put back.
pub fn install(staged: &Path, target: &Path) -> Result<(), String> {
    let old = sibling(target, ".old");
    let _ = fs::remove_file(&old);
    fs::rename(target, &old).map_err(|e| {
        format!("Kestrel's folder can't be changed without administrator rights ({e}). Download the update from the release page and replace kestrel.exe yourself.")
    })?;
    if let Err(e) = fs::rename(staged, target) {
        let _ = fs::rename(&old, target);
        return Err(format!("The update couldn't be put in place ({e}). Nothing was changed."));
    }
    log("Update: installed; the new version starts after Kestrel closes");
    Ok(())
}

/// After an update: removes the previous exe and any unused download. Missing files are fine.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(sibling(&exe, ".old"));
        let _ = fs::remove_file(sibling(&exe, ".new"));
    }
}

/// Update preferences in `%LOCALAPPDATA%\Kestrel\updates.txt`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prefs {
    /// The first-launch question has been answered.
    pub asked: bool,
    pub enabled: bool,
    /// Unix time of the last successful check.
    pub last_check: u64,
    /// "Remind me later" hides the notification until then.
    pub snooze_until: u64,
}

impl Prefs {
    fn path() -> PathBuf {
        data_dir().join("updates.txt")
    }

    fn parse(text: &str) -> Prefs {
        let mut p = Prefs::default();
        for (k, v) in text.lines().filter_map(|l| l.split_once('\t')) {
            match k {
                "asked" => p.asked = v == "1",
                "enabled" => p.enabled = v == "1",
                "last_check" => p.last_check = v.parse().unwrap_or(0),
                "snooze_until" => p.snooze_until = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        p
    }

    pub fn load() -> Prefs {
        Prefs::parse(&fs::read_to_string(Prefs::path()).unwrap_or_default())
    }

    pub fn save(&self) -> Result<(), String> {
        let text = format!(
            "asked\t{}\nenabled\t{}\nlast_check\t{}\nsnooze_until\t{}\n",
            u8::from(self.asked),
            u8::from(self.enabled),
            self.last_check,
            self.snooze_until
        );
        crate::utils::write_atomic(&Prefs::path(), text.as_bytes()).map_err(|e| format!("Couldn't save update settings: {e}"))
    }

    /// An automatic check is allowed and the last successful one is older than a day.
    pub fn check_due(&self, now: u64) -> bool {
        self.enabled && now.saturating_sub(self.last_check) >= CHECK_INTERVAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.10.0"), Some((0, 10, 0)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3-beta.1"), None);
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.9.0", "0.10.0"));
        assert!(!is_newer("0.10.0", "0.10.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn release_links() {
        assert_eq!(tag_from_location("https://github.com/o/r/releases/tag/v1.2.3"), Some("v1.2.3"));
        assert_eq!(tag_from_location("https://github.com/o/r/releases"), None);
        assert_eq!(tag_from_location("https://github.com/o/r/releases/tag/"), None);
    }

    #[test]
    fn prefs_round_trip_and_interval() {
        let p = Prefs { asked: true, enabled: true, last_check: 1000, snooze_until: 5 };
        let text = format!("asked\t1\nenabled\t1\nlast_check\t{}\nsnooze_until\t5\n", p.last_check);
        assert_eq!(Prefs::parse(&text), p);
        assert!(!p.check_due(1000 + CHECK_INTERVAL - 1));
        assert!(p.check_due(1000 + CHECK_INTERVAL));
        assert!(!Prefs { enabled: false, ..p }.check_due(u64::MAX));
        assert_eq!(Prefs::parse(""), Prefs::default());
    }

    /// A throwaway test key (its private half was deleted after signing SAMPLE). Never sign test data with the
    /// release key: a valid release-key signature in the repository would be a ready-made forged update.
    const TEST_KEY: &str = "454353312000000085a5520545e1ad96df908cfc99a09e7675ea13f0bed582319c28a40ebd6052b3bb293d66f5849bf50866a56f1a60384c28e5fd3677a86fed4c11c75aaa9f9ee0";
    /// `kestrel-sign sign` with TEST_KEY over the bytes "kestrel signature test".
    const SAMPLE: &str = "kestrel-update\nversion=99.0.0\nsha256=66b5ca02cc3ff60dfb5452b8162bfdd61839caae6e8ae06c464f1feb90ab896b\nsignature=b0f25d497cdcb4fcc2e2fe5381ae72bfe8f4b3374e21eede4a8fd4d158d1e63c017c24ea911914f10a8f44d9edf055aadf344e28c9ba2bfba97c6280b8bcef15\n";

    #[test]
    fn accepts_signature_only_as_signed() {
        let sha = verify_signature(SAMPLE, "99.0.0", TEST_KEY).unwrap();
        assert_eq!(sha, crypto::hex(&crypto::sha256(b"kestrel signature test").unwrap()));
        // Same signature presented for another version: refused.
        assert!(verify_signature(SAMPLE, "99.0.1", TEST_KEY).is_err());
        // Any change to the signed text breaks it.
        let other = SAMPLE.replace("version=99.0.0", "version=99.0.1");
        assert!(verify_signature(&other, "99.0.1", TEST_KEY).unwrap_err().contains("not valid"));
        assert!(verify_signature(&SAMPLE.replace("66b5", "66b6"), "99.0.0", TEST_KEY).unwrap_err().contains("not valid"));
        // A signature from any other key, the real release key included, is refused.
        assert!(verify_signature(SAMPLE, "99.0.0", PUBLIC_KEY).unwrap_err().contains("not valid"));
    }

    #[test]
    fn rejects_bad_signatures() {
        let sha = "ab".repeat(32);
        // Valid layout but not signed by the release key.
        let forged = format!("{}signature={}\n", crypto::message("99.0.0", &sha), "11".repeat(64));
        assert!(verify_signature(&forged, "99.0.0", PUBLIC_KEY).unwrap_err().contains("not valid"));
        assert!(verify_signature("version=99.0.0\n", "99.0.0", PUBLIC_KEY).unwrap_err().contains("incomplete"));
        let bad_hex = format!("{}signature=zz\n", crypto::message("99.0.0", &sha));
        assert!(verify_signature(&bad_hex, "99.0.0", PUBLIC_KEY).unwrap_err().contains("malformed"));
        assert_eq!(crypto::unhex(PUBLIC_KEY).map(|k| k.len()), Some(72), "release key is a P-256 public blob");
    }

    #[test]
    fn only_real_programs_replace_kestrel() {
        assert!(!looks_like_exe(b"kestrel signature test"));
        assert!(!looks_like_exe(&vec![0u8; 1 << 20]));
        let me = fs::read(std::env::current_exe().unwrap()).unwrap();
        assert!(looks_like_exe(&me), "the running test binary is a real x64 program");
    }

    /// Real network: `cargo test live_update -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_update_network() {
        use std::sync::atomic::{AtomicU64, Ordering};
        // A public repository with releases shows the redirect format the check relies on.
        let (status, location) = http::redirect("https://github.com/BurntSushi/ripgrep/releases/latest").unwrap();
        println!("public repo: {status} {location:?}");
        assert!((300..400).contains(&status));
        assert!(tag_from_location(&location.unwrap()).is_some());
        // Downloading follows GitHub's redirect to its file host and reports progress.
        let (done, total) = (AtomicU64::new(0), AtomicU64::new(0));
        let body = http::download("https://github.com/BurntSushi/ripgrep/raw/master/README.md", 1 << 20, &done, &total).unwrap();
        println!("downloaded {} bytes (progress {}/{})", body.len(), done.load(Ordering::Relaxed), total.load(Ordering::Relaxed));
        assert!(body.len() > 1000);
        // Size limit and missing files fail cleanly.
        assert!(http::download("https://github.com/BurntSushi/ripgrep/raw/master/README.md", 10, &done, &total).is_err());
        assert!(http::download("https://github.com/BurntSushi/ripgrep/releases/download/0.0.0/none.exe", 1 << 20, &done, &total)
            .unwrap_err()
            .contains("wasn't found"));
        assert!(http::redirect("http://github.com/").unwrap_err().contains("https"));
        println!("this repo: {:?}", latest());
    }

    #[test]
    fn install_swaps_and_rolls_back() {
        let dir = std::env::temp_dir().join("kestrel_update_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (target, staged) = (dir.join("kestrel.exe"), dir.join("kestrel.exe.new"));
        fs::write(&target, b"old").unwrap();
        fs::write(&staged, b"new").unwrap();
        install(&staged, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(fs::read(sibling(&target, ".old")).unwrap(), b"old");
        // A missing download leaves the current exe where it was.
        assert!(install(&dir.join("missing.new"), &target).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"new");
        let _ = fs::remove_dir_all(&dir);
    }
}
