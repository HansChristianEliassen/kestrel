//! Drive health, read-only: NVMe drives report their SMART/health log page (the same data Settings > Storage >
//! Disks & volumes shows), SATA drives their built-in failure prediction. Opened without access rights, so no
//! administrator is needed and nothing can be written.

use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Ioctl::{IOCTL_STORAGE_PREDICT_FAILURE, IOCTL_STORAGE_QUERY_PROPERTY};
use windows::Win32::System::IO::DeviceIoControl;

use crate::system::wide;

/// The fields of the NVMe health log (NVMe spec, Get Log Page 02h) Kestrel reports.
#[derive(Debug, PartialEq)]
pub struct NvmeHealth {
    /// Any bit set: the drive itself reports a problem (spare low, temperature, reliability, read-only, backup).
    pub critical_warning: u8,
    pub celsius: i32,
    pub spare: u8,
    pub spare_threshold: u8,
    /// Estimated share of the drive's rated life used; can pass 100.
    pub used: u8,
    pub hours: u64,
    pub media_errors: u64,
}

pub fn parse_nvme(log: &[u8]) -> Option<NvmeHealth> {
    let u64_at = |o: usize| u64::from_le_bytes(log[o..o + 8].try_into().unwrap());
    (log.len() >= 512).then(|| NvmeHealth {
        critical_warning: log[0],
        celsius: i32::from(u16::from_le_bytes([log[1], log[2]])) - 273,
        spare: log[3],
        spare_threshold: log[4],
        used: log[5],
        hours: u64_at(128),
        media_errors: u64_at(160),
    })
}

/// One drive: (name, report, needs attention).
pub fn drives() -> Vec<(String, String, bool)> {
    (0..16).filter_map(drive).collect()
}

fn ioctl(h: windows::Win32::Foundation::HANDLE, code: u32, input: &[u8], out: &mut [u8]) -> bool {
    let mut n = 0u32;
    unsafe {
        DeviceIoControl(
            h,
            code,
            (!input.is_empty()).then_some(input.as_ptr().cast()),
            input.len() as u32,
            Some(out.as_mut_ptr().cast()),
            out.len() as u32,
            Some(&mut n),
            None,
        )
    }
    .is_ok()
}

fn drive(index: u32) -> Option<(String, String, bool)> {
    let path = wide(&format!(r"\\.\PhysicalDrive{index}"));
    let h = unsafe {
        CreateFileW(PCWSTR(path.as_ptr()), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None)
    }
    .ok()?;
    // STORAGE_PROPERTY_QUERY for the device descriptor: model name and bus type.
    let mut desc = [0u8; 1024];
    let named = ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &[0u8; 12], &mut desc);
    let at = |o: usize| u32::from_le_bytes(desc[o..o + 4].try_into().unwrap()) as usize;
    let model = if named && (1..desc.len()).contains(&at(16)) {
        let s = &desc[at(16)..];
        String::from_utf8_lossy(&s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]).trim().to_string()
    } else {
        String::new()
    };
    let bus = if named { at(28) } else { 0 };
    let name = if model.is_empty() { format!("Disk {index}") } else { model };
    let result = match bus {
        17 => nvme(h).map(|health| describe(&health)), // BusTypeNvme
        7 => None,                                     // USB enclosures rarely pass health data through
        _ => sata(h),
    };
    unsafe {
        let _ = CloseHandle(h);
    }
    let (text, attention) = result.unwrap_or_else(|| ("health not reported".into(), false));
    Some((name, text, attention))
}

/// Asks the NVMe driver for the health log page (STORAGE_PROTOCOL_SPECIFIC_DATA after the 8-byte query header).
fn nvme(h: windows::Win32::Foundation::HANDLE) -> Option<NvmeHealth> {
    const SPECIFIC: usize = 40; // size of STORAGE_PROTOCOL_SPECIFIC_DATA
    let mut q = vec![0u8; 8 + SPECIFIC + 512];
    let fields: [u32; 8] = [50, 0, 3, 2, 2, 0, SPECIFIC as u32, 512]; // device protocol-specific query, NVMe, log page 2 (health)
    for (i, v) in fields.iter().enumerate() {
        q[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    let mut out = vec![0u8; q.len()];
    if !ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &q, &mut out) {
        return None;
    }
    // Output: STORAGE_PROTOCOL_DATA_DESCRIPTOR (Version, Size, then the specific data); the log follows at its offset.
    let offset = u32::from_le_bytes(out[24..28].try_into().unwrap()) as usize;
    out.get(8 + offset..).and_then(parse_nvme)
}

/// SATA/SAS: the drive's own SMART threshold verdict (IOCTL_STORAGE_PREDICT_FAILURE).
fn sata(h: windows::Win32::Foundation::HANDLE) -> Option<(String, bool)> {
    let mut out = [0u8; 4 + 512];
    ioctl(h, IOCTL_STORAGE_PREDICT_FAILURE, &[], &mut out).then(|| {
        if u32::from_le_bytes(out[..4].try_into().unwrap()) != 0 {
            ("the drive predicts it will fail soon. Back up your files now and plan to replace it".into(), true)
        } else {
            ("healthy (the drive's own check)".into(), false)
        }
    })
}

fn describe(h: &NvmeHealth) -> (String, bool) {
    let mut problems = Vec::new();
    if h.critical_warning != 0 {
        problems.push("the drive reports a critical warning");
    }
    if h.spare < h.spare_threshold {
        problems.push("its spare capacity is below the safe level");
    }
    if h.used >= 90 {
        problems.push("it is near the end of its rated life");
    }
    if h.media_errors > 0 {
        problems.push("it has recorded data errors");
    }
    let facts = format!("{} °C, {}% of rated life used, {} hours powered on", h.celsius, h.used, h.hours);
    if problems.is_empty() {
        (format!("healthy, {facts}"), false)
    } else {
        (format!("{}. Back up your files. {facts}", problems.join(", ")), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvme_health_log() {
        let mut log = [0u8; 512];
        log[1..3].copy_from_slice(&314u16.to_le_bytes()); // 41 °C
        log[3] = 100;
        log[4] = 10;
        log[5] = 3;
        log[128..136].copy_from_slice(&12_345u64.to_le_bytes());
        let h = parse_nvme(&log).unwrap();
        assert_eq!(
            h,
            NvmeHealth { critical_warning: 0, celsius: 41, spare: 100, spare_threshold: 10, used: 3, hours: 12_345, media_errors: 0 }
        );
        assert_eq!(describe(&h), ("healthy, 41 °C, 3% of rated life used, 12345 hours powered on".into(), false));
        log[0] = 1;
        log[160] = 2;
        let (text, attention) = describe(&parse_nvme(&log).unwrap());
        assert!(attention && text.starts_with("the drive reports a critical warning, it has recorded data errors"));
        assert!(parse_nvme(&log[..100]).is_none());
    }

    #[test]
    #[ignore = "reads this PC's drives"]
    fn live() {
        for d in drives() {
            eprintln!("{d:?}");
        }
    }
}
