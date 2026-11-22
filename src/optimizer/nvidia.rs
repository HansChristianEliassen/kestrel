//! NVIDIA driver settings (the same ones the NVIDIA Control Panel shows), through NVIDIA's driver settings API
//! (NvAPI DRS) in nvapi64.dll, which ships with every NVIDIA driver. Loaded at run time; PCs without an NVIDIA
//! driver just don't have it. Only documented DRS calls and setting IDs are used.

use std::ffi::c_void;

use windows::core::{s, w};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

// NvAPI function IDs, as resolved through nvapi_QueryInterface (from NVIDIA's public NvAPI headers).
const INITIALIZE: u32 = 0x0150_E828;
const DRS_CREATE_SESSION: u32 = 0x0694_D52E;
const DRS_DESTROY_SESSION: u32 = 0xDAD9_CFF8;
const DRS_LOAD_SETTINGS: u32 = 0x375D_BD6B;
const DRS_SAVE_SETTINGS: u32 = 0xFCBC_7E14;
const DRS_GET_BASE_PROFILE: u32 = 0xDA84_66A0;
const DRS_GET_SETTING: u32 = 0x73BF_8338;
const DRS_SET_SETTING: u32 = 0x577D_D202;

/// "Shader Cache Size" in the NVIDIA Control Panel, in megabytes (0xFFFFFFFF = unlimited).
pub const SHADER_CACHE_SIZE: u32 = 0x00AC_8497;
/// Kestrel's target: 10 GB, so the driver doesn't throw away compiled shaders and games don't stutter rebuilding them.
pub const SHADER_CACHE_TARGET_MB: u32 = 10 * 1024;

/// NVDRS_SETTING (version 1): name, id, type, location, predefined/current flags and two 4100-byte value unions.
#[repr(C)]
struct Setting {
    version: u32,
    name: [u16; 2048],
    id: u32,
    kind: u32,
    location: u32,
    is_current_predefined: u32,
    is_predefined_valid: u32,
    predefined: [u8; 4100],
    current: [u8; 4100],
}
const SETTING_SIZE: usize = 4 + 4096 + 5 * 4 + 4100 + 4100;
const _: () = assert!(std::mem::size_of::<Setting>() == SETTING_SIZE);

type QueryInterface = unsafe extern "C" fn(u32) -> *const c_void;
type Status = i32;

/// An open driver-settings session; closed when dropped.
struct Drs {
    query: QueryInterface,
    session: *mut c_void,
}

impl Drs {
    fn open() -> Result<Drs, String> {
        unsafe {
            let lib = LoadLibraryExW(w!("nvapi64.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
                .map_err(|_| "No NVIDIA driver on this PC.".to_string())?;
            let qi = GetProcAddress(lib, s!("nvapi_QueryInterface")).ok_or("The NVIDIA driver doesn't offer its settings API.")?;
            let query: QueryInterface = std::mem::transmute(qi);
            let drs = Drs { query, session: std::ptr::null_mut() };
            let init: unsafe extern "C" fn() -> Status = drs.func(INITIALIZE)?;
            check(init(), "start")?;
            let create: unsafe extern "C" fn(*mut *mut c_void) -> Status = drs.func(DRS_CREATE_SESSION)?;
            let mut session = std::ptr::null_mut();
            check(create(&mut session), "open the settings")?;
            let drs = Drs { session, ..drs };
            let load: unsafe extern "C" fn(*mut c_void) -> Status = drs.func(DRS_LOAD_SETTINGS)?;
            check(load(drs.session), "load the settings")?;
            Ok(drs)
        }
    }

    /// Looks up an NvAPI function by id and casts it to the expected signature `F` (a function pointer type).
    unsafe fn func<F: Copy>(&self, id: u32) -> Result<F, String> {
        let p = (self.query)(id);
        if p.is_null() {
            return Err(format!("This NVIDIA driver doesn't support a needed function ({id:#010x})."));
        }
        Ok(std::mem::transmute_copy(&p))
    }

    fn base_profile(&self) -> Result<*mut c_void, String> {
        unsafe {
            let get: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> Status = self.func(DRS_GET_BASE_PROFILE)?;
            let mut profile = std::ptr::null_mut();
            check(get(self.session, &mut profile), "open the global profile")?;
            Ok(profile)
        }
    }

    /// The current value of a 32-bit setting in the global profile.
    fn get(&self, id: u32) -> Result<u32, String> {
        unsafe {
            let get: unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut Setting) -> Status = self.func(DRS_GET_SETTING)?;
            let mut s: Box<Setting> = Box::new(std::mem::zeroed());
            s.version = SETTING_SIZE as u32 | (1 << 16);
            check(get(self.session, self.base_profile()?, id, &mut *s), "read the setting")?;
            Ok(u32::from_le_bytes(s.current[..4].try_into().unwrap()))
        }
    }

    /// Sets a 32-bit setting in the global profile and saves it (needs administrator rights).
    fn set(&self, id: u32, value: u32) -> Result<(), String> {
        unsafe {
            let set: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut Setting) -> Status = self.func(DRS_SET_SETTING)?;
            let save: unsafe extern "C" fn(*mut c_void) -> Status = self.func(DRS_SAVE_SETTINGS)?;
            let mut s: Box<Setting> = Box::new(std::mem::zeroed());
            s.version = SETTING_SIZE as u32 | (1 << 16);
            s.id = id;
            s.kind = 0; // NVDRS_DWORD_TYPE
            s.current[..4].copy_from_slice(&value.to_le_bytes());
            check(set(self.session, self.base_profile()?, &mut *s), "change the setting")?;
            check(save(self.session), "save the settings")
        }
    }
}

impl Drop for Drs {
    fn drop(&mut self) {
        if self.session.is_null() {
            return;
        }
        unsafe {
            if let Ok(destroy) = self.func::<unsafe extern "C" fn(*mut c_void) -> Status>(DRS_DESTROY_SESSION) {
                destroy(self.session);
            }
        }
    }
}

fn check(status: Status, what: &str) -> Result<(), String> {
    match status {
        0 => Ok(()),
        -3 => Err(format!("Couldn't {what}: no NVIDIA graphics card is active.")),
        // NVAPI_INVALID_USER_PRIVILEGE
        -137 => Err(format!("Couldn't {what}: needs administrator rights.")),
        s => Err(format!("Couldn't {what} (NVIDIA error {s}).")),
    }
}

/// The global shader cache size in MB, or why it can't be read.
pub fn shader_cache() -> Result<u32, String> {
    Drs::open()?.get(SHADER_CACHE_SIZE)
}

pub fn set_shader_cache(mb: u32) -> Result<(), String> {
    Drs::open()?.set(SHADER_CACHE_SIZE, mb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads this PC's NVIDIA driver settings (read-only)"]
    fn live_read() {
        eprintln!("shader cache MB: {:?}", shader_cache());
    }
}
