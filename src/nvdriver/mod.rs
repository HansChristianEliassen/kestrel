//! GPU > Driver Optimizer: finds the NVIDIA driver for this graphics card on NVIDIA's own servers (or takes a
//! driver file you already have), checks NVIDIA's digital signature, unpacks it, leaves out the optional
//! components a preset doesn't need, and installs or exports the result. See `package.rs` for the stripping.
//!
//! Talks only to nvidia.com (driver lookup and download). Sends nothing about the user; Kestrel has no telemetry.

pub mod install;
pub mod package;
pub mod telemetry;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};

use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use crate::system::run_hidden;
use crate::update::http;
use crate::utils::json_str;

/// The NVIDIA graphics card in this PC.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub name: String,
    /// PCI device ID, e.g. "1E84".
    pub device: String,
    /// Installed driver in NVIDIA's numbering (e.g. "621.24").
    pub installed: Option<String>,
    pub laptop: bool,
}

/// NVIDIA's version (621.24) from Windows' driver version (32.0.16.2124): the last five digits.
pub fn nvidia_version(windows: &str) -> Option<String> {
    let parts: Vec<&str> = windows.split('.').collect();
    let [.., a, b] = parts[..] else { return None };
    let digits = format!("{a}{b:0>4}");
    let tail = digits.get(digits.len().checked_sub(5)?..)?;
    tail.chars().all(|c| c.is_ascii_digit()).then(|| format!("{}.{}", &tail[..3], &tail[3..]))
}

/// The NVIDIA display adapter, from the display device class (readable without administrator rights).
pub fn detect() -> Option<Gpu> {
    let class = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}")
        .ok()?;
    class.enum_keys().flatten().find_map(|sub| {
        let k = class.open_subkey(&sub).ok()?;
        let id: String = k.get_value("MatchingDeviceId").ok()?;
        let id = id.to_uppercase();
        let dev = id.strip_prefix(r"PCI\VEN_10DE&DEV_")?.get(..4)?.to_string();
        let name: String = k.get_value("DriverDesc").ok()?;
        let installed = k.get_value::<String, _>("DriverVersion").ok().and_then(|v| nvidia_version(&v));
        let laptop = name.to_lowercase().contains("laptop") || crate::optimizer::hardware::has_battery();
        Some(Gpu { name, device: dev, installed, laptop })
    })
}

/// A driver release on NVIDIA's servers.
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub name: String,
    pub date: String,
    pub size: String,
    pub url: String,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// (ParentID, name, value) rows of NVIDIA's product lookup XML.
fn lookup_rows(xml: &str) -> Vec<(String, String, String)> {
    xml.split("<LookupValue")
        .skip(1)
        .filter_map(|row| {
            let parent = row.split("ParentID=\"").nth(1)?.split('"').next()?.to_string();
            let name = row.split("<Name>").nth(1)?.split("</Name>").next()?.trim().to_string();
            let value = row.split("<Value>").nth(1)?.split("</Value>").next()?.trim().to_string();
            Some((parent, name, value))
        })
        .collect()
}

fn product_name(s: &str) -> String {
    let s = s.to_lowercase().replace("&amp;", "&");
    let s = s.trim().trim_start_matches("nvidia ").replace(" laptop gpu", "");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// NVIDIA's (series id, product id) for a card: its name matched against NVIDIA's product list, in a laptop
/// ("Notebooks") series for laptops and a desktop one otherwise.
fn product_ids(gpu: &Gpu, products: &str, series: &str) -> Option<(String, String)> {
    let notebook: Vec<String> = lookup_rows(series).into_iter().filter(|(_, n, _)| n.contains("Notebooks")).map(|(_, _, v)| v).collect();
    let want = product_name(&gpu.name);
    lookup_rows(products)
        .into_iter()
        .filter(|(_, n, _)| product_name(n) == want)
        .find(|(p, _, _)| notebook.contains(p) == gpu.laptop)
        .map(|(p, _, v)| (p, v))
}

/// Releases from NVIDIA's driver lookup (one `downloadInfo` object each), newest first.
fn parse_releases(json: &str) -> Vec<Release> {
    json.split("\"downloadInfo\"")
        .skip(1)
        .filter_map(|chunk| {
            let url = json_str(chunk, "DownloadURL")?;
            // Only NVIDIA's own download servers over https.
            let host = url.strip_prefix("https://")?.split('/').next()?;
            if !host.ends_with(".nvidia.com") || !url.to_lowercase().ends_with(".exe") {
                return None;
            }
            Some(Release {
                version: json_str(chunk, "Version")?,
                name: percent_decode(&json_str(chunk, "Name").unwrap_or_default()),
                date: json_str(chunk, "ReleaseDateTime").unwrap_or_default(),
                size: json_str(chunk, "DownloadURLFileSize").unwrap_or_default(),
                url,
            })
        })
        .collect()
}

/// Recent Game Ready (or Studio) drivers for this card, newest first.
pub fn releases(gpu: &Gpu, studio: bool) -> Result<Vec<Release>, String> {
    let base = "https://www.nvidia.com/Download/API/lookupValueSearch.aspx";
    let products = http::get_text(&format!("{base}?TypeID=3"), 4 << 20)?;
    let series = http::get_text(&format!("{base}?TypeID=2"), 4 << 20)?;
    let (psid, pfid) = product_ids(gpu, &products, &series)
        .ok_or_else(|| format!("NVIDIA's driver list doesn't name the {}. Import a driver file instead.", gpu.name))?;
    let url = format!(
        "https://gfwsl.geforce.com/services_toolkit/services/com/nvidia/services/AjaxDriverService.php?func=DriverManualLookup&psid={psid}&pfid={pfid}&osID=57&languageCode=1033&beta=0&isWHQL={}&dltype=-1&dch=1&upCRD={}&qnf=0&sort1=0&numberOfResults=15",
        u8::from(!studio),
        u8::from(studio)
    );
    // Studio drivers are listed only with isWHQL=0 (they're WHQL-signed all the same).
    let list = parse_releases(&http::get_text(&url, 8 << 20)?);
    if list.is_empty() {
        return Err("NVIDIA's driver lookup returned no drivers for this card.".into());
    }
    Ok(list)
}

/// Where downloads, unpacked packages and builds go: Windows' temporary folder, removed by `clean_work`.
pub fn work_dir() -> PathBuf {
    std::env::temp_dir().join("Kestrel NVIDIA driver")
}

/// Deletes everything the optimizer downloaded, unpacked or built.
pub fn clean_work() -> Result<(), String> {
    let dir = work_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't remove the temporary files: {e}"))?;
    }
    Ok(())
}

pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

impl Default for Progress {
    fn default() -> Self {
        Progress { done: AtomicU64::new(0), total: AtomicU64::new(0), cancel: AtomicBool::new(false) }
    }
}

/// Downloads a release into the work folder and checks its signature.
pub fn download(r: &Release, p: &Progress) -> Result<PathBuf, String> {
    let file =
        r.url.rsplit('/').next().filter(|f| f.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))).unwrap_or("driver.exe");
    let dir = work_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(file);
    http::download_to(&r.url, &path, &p.done, &p.total, &p.cancel)?;
    if let Err(e) = verify_signature(&path) {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

/// Windows' Authenticode result for a file: (status, signer subject).
pub fn signature_status(path: &Path) -> Result<(String, String), String> {
    let p = path.to_string_lossy().replace('\'', "''");
    let script = format!("$s = Get-AuthenticodeSignature -LiteralPath '{p}'; \"$($s.Status)|$($s.SignerCertificate.Subject)\"");
    let out = run_hidden("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &script])?;
    let (status, subject) = out.trim().split_once('|').unwrap_or((out.trim(), ""));
    Ok((status.to_string(), subject.to_string()))
}

/// Windows' Authenticode check: a valid signature whose signer is NVIDIA Corporation.
pub fn verify_signature(path: &Path) -> Result<(), String> {
    let (status, subject) = signature_status(path)?;
    if status != "Valid" {
        return Err(format!("The file's digital signature isn't valid ({status}). Use a driver downloaded from nvidia.com."));
    }
    if !subject.contains("O=NVIDIA Corporation") {
        return Err("The file isn't signed by NVIDIA.".into());
    }
    Ok(())
}

/// Unpacks a driver package (a 7-Zip self-extractor) with Windows' own tar (libarchive), or 7-Zip if that's
/// installed and tar can't read it.
pub fn extract(exe: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| format!("Couldn't clear the old files: {e}"))?;
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let (exe_s, dest_s) = (exe.to_string_lossy().into_owned(), dest.to_string_lossy().into_owned());
    let tar = crate::system::expand_env(r"%SystemRoot%\System32\tar.exe");
    let mut result = run_hidden(&tar, &["-xf", &exe_s, "-C", &dest_s]).map(|_| ());
    if result.is_err() || !dest.join("setup.cfg").is_file() {
        let seven = crate::system::expand_env(r"%ProgramFiles%\7-Zip\7z.exe");
        if Path::new(&seven).is_file() {
            result = run_hidden(&seven, &["x", "-y", &format!("-o{dest_s}"), &exe_s]).map(|_| ());
        }
    }
    match result {
        Ok(()) if dest.join("setup.cfg").is_file() => Ok(()),
        Ok(()) => Err("This file isn't an NVIDIA driver package.".into()),
        Err(_) => Err("Windows couldn't unpack this file. It may not be an NVIDIA driver package, or the disk may be full.".into()),
    }
}

/// Whether the package's device list names this card (None when the package has no list).
pub fn supports(dir: &Path, device: &str) -> Option<bool> {
    let list = std::fs::read_to_string(dir.join("ListDevices.txt")).ok()?;
    let want = format!("DEV_{}", device.to_uppercase());
    Some(list.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with(&want) && l[want.len()..].starts_with([' ', '&', '\t'])
    }))
}

/// A driver file checked, unpacked and read, ready to strip.
#[derive(Clone)]
pub struct Prepared {
    pub pkg: package::Package,
    /// Whether the package lists this card (None: the package has no list).
    pub supported: Option<bool>,
}

fn set(stage: &std::sync::Mutex<String>, text: &str) {
    if let Ok(mut s) = stage.lock() {
        *s = text.to_string();
    }
}

/// Signature check, unpacking and analysis of a driver file. `stage` says what's happening.
pub fn prepare(exe: &Path, device: &str, stage: &std::sync::Mutex<String>) -> Result<Prepared, String> {
    set(stage, "Checking NVIDIA's signature…");
    verify_signature(exe)?;
    set(stage, "Unpacking the driver (about a minute)…");
    let stem = exe.file_stem().map_or("driver".into(), |s| s.to_string_lossy().into_owned());
    let dir = work_dir().join(format!("{stem} unpacked"));
    extract(exe, &dir)?;
    set(stage, "Reading its components…");
    let pkg = package::analyze(&dir)?;
    let supported = supports(&dir, device);
    crate::utils::log(&format!("NVIDIA driver: prepared {} ({} components)", pkg.version, pkg.components.len()));
    Ok(Prepared { pkg, supported })
}

/// The stripped package's folder for a version, in the work folder.
pub fn build_dir(version: &str) -> PathBuf {
    work_dir().join(format!("{version} for gaming"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(nvidia_version("32.0.16.2124").as_deref(), Some("621.24"));
        assert_eq!(nvidia_version("31.0.15.5222").as_deref(), Some("552.22"));
        assert_eq!(nvidia_version("31.0.15.904").as_deref(), Some("509.04"));
        assert_eq!(nvidia_version("bad"), None);
    }

    #[test]
    fn decoding_and_lookup() {
        assert_eq!(percent_decode("GeForce%20Game%20Ready%20Driver"), "GeForce Game Ready Driver");
        assert_eq!(percent_decode("100%"), "100%");
        let products = r#"<LookupValues><LookupValue ParentID="107">
<Name>GeForce RTX 2070 SUPER</Name>
<Value>903</Value></LookupValue><LookupValue ParentID="111"><Name>GeForce RTX 2070 SUPER</Name><Value>920</Value></LookupValue></LookupValues>"#;
        let series = r#"<LookupValue ParentID="1"><Name>GeForce RTX 20 Series (Notebooks)</Name><Value>111</Value></LookupValue><LookupValue ParentID="1"><Name>GeForce RTX 20 Series</Name><Value>107</Value></LookupValue>"#;
        let mut gpu = Gpu { name: "NVIDIA GeForce RTX 2070 SUPER".into(), device: "1E84".into(), installed: None, laptop: false };
        assert_eq!(product_ids(&gpu, products, series), Some(("107".into(), "903".into())));
        gpu.laptop = true;
        assert_eq!(product_ids(&gpu, products, series), Some(("111".into(), "920".into())));
        let json = r#"{ "IDS" : [{ "downloadInfo": { "Version" : "621.40", "Name" : "GeForce%20Game%20Ready%20Driver", "ReleaseDateTime" : "Tue Nov 24, 2026", "DownloadURL" : "https://us.download.nvidia.com/Windows/621.40/621.40-desktop.exe", "DownloadURLFileSize" : "1.04 GB" }},
            { "downloadInfo": { "Version" : "1.0", "DownloadURL" : "https://evil.example.com/x.exe" }}]}"#;
        let r = parse_releases(json);
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].version.as_str(), r[0].name.as_str(), r[0].size.as_str()), ("621.40", "GeForce Game Ready Driver", "1.04 GB"));
    }

    #[test]
    fn device_list() {
        let dir = std::env::temp_dir().join("kestrel-test-listdevices");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("ListDevices.txt"),
            "nv_disp.inf:\n\tDEV_1E84 \"NVIDIA GeForce RTX 2070 SUPER\"\n\tDEV_1E87&SUBSYS_1 \"x\"\n",
        )
        .unwrap();
        assert_eq!(supports(&dir, "1e84"), Some(true));
        assert_eq!(supports(&dir, "1E87"), Some(true));
        assert_eq!(supports(&dir, "1E8"), Some(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "asks NVIDIA's servers for this PC's drivers"]
    fn live_lookup() {
        let gpu = detect().expect("an NVIDIA card");
        println!("{gpu:?}");
        for r in releases(&gpu, false).unwrap().iter().take(3) {
            println!("{r:?}");
        }
        println!("studio: {:?}", releases(&gpu, true).map(|l| l.first().map(|r| r.version.clone())));
    }
}
