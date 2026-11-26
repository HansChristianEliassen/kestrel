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
const DRS_DELETE_SETTING: u32 = 0xE4A2_6362;
const DRS_FIND_APPLICATION: u32 = 0xEEE5_66B2;
const DRS_FIND_PROFILE: u32 = 0x7E4A_9A0B;
const DRS_CREATE_PROFILE: u32 = 0xCC17_6068;
const DRS_CREATE_APPLICATION: u32 = 0x4347_A9DE;
const DRS_GET_PROFILE_INFO: u32 = 0x61CD_6FD6;
const DRS_DELETE_PROFILE: u32 = 0x1709_3206;

// NvAPI_Status values that mean "not set here" rather than a failure.
const SETTING_NOT_FOUND: Status = -160;
const PROFILE_NOT_FOUND: Status = -163;
const PROFILE_NAME_IN_USE: Status = -164;
const EXECUTABLE_NOT_FOUND: Status = -166;

/// "Power management mode"; 1 = Prefer maximum performance (default 5, Normal).
pub const POWER_MODE: u32 = 0x1057_EB71;
/// "Maximum pre-rendered frames", shown as Low Latency Mode; 1 = On (default 0, the game decides).
pub const PRERENDER_LIMIT: u32 = 0x007B_A09E;
/// Profiles Kestrel creates for games NVIDIA has none for start with this, so Undo can remove them again.
const OWN_PROFILE: &str = "Kestrel: ";

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

/// NvAPI_UnicodeString.
type Name = [u16; 2048];

/// NVDRS_APPLICATION (version 4).
#[repr(C)]
struct Application {
    version: u32,
    is_predefined: u32,
    app_name: Name,
    friendly_name: Name,
    launcher: Name,
    file_in_folder: Name,
    flags: u32,
    command_line: Name,
}
const APPLICATION_SIZE: usize = 8 + 4 * 4096 + 4 + 4096;
const _: () = assert!(std::mem::size_of::<Application>() == APPLICATION_SIZE);

/// NVDRS_PROFILE (version 1).
#[repr(C)]
struct Profile {
    version: u32,
    name: Name,
    gpu_support: u32,
    is_predefined: u32,
    num_apps: u32,
    num_settings: u32,
}
const PROFILE_SIZE: usize = 4 + 4096 + 16;
const _: () = assert!(std::mem::size_of::<Profile>() == PROFILE_SIZE);

fn name(s: &str) -> Name {
    let mut n = [0u16; 2048];
    for (d, c) in n.iter_mut().zip(s.encode_utf16().take(2047)) {
        *d = c;
    }
    n
}

fn read_name(n: &Name) -> String {
    String::from_utf16_lossy(&n[..n.iter().position(|&c| c == 0).unwrap_or(n.len())])
}

type Handle = *mut c_void;

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
        self.get_in(self.base_profile()?, id)?.ok_or_else(|| "Couldn't read the setting: the driver doesn't have it.".into())
    }

    /// The value a profile itself sets (None when it inherits the global or default value).
    fn get_in(&self, profile: Handle, id: u32) -> Result<Option<u32>, String> {
        unsafe {
            let get: unsafe extern "C" fn(Handle, Handle, u32, *mut Setting) -> Status = self.func(DRS_GET_SETTING)?;
            let mut s: Box<Setting> = Box::new(std::mem::zeroed());
            s.version = SETTING_SIZE as u32 | (1 << 16);
            match get(self.session, profile, id, &mut *s) {
                SETTING_NOT_FOUND => Ok(None),
                // NVDRS_CURRENT_PROFILE_LOCATION: set in this profile, not inherited.
                0 if s.location == 0 => Ok(Some(u32::from_le_bytes(s.current[..4].try_into().unwrap()))),
                0 => Ok(None),
                st => check(st, "read the setting").map(|_| None),
            }
        }
    }

    /// Sets a 32-bit setting in the global profile and saves it (needs administrator rights).
    fn set(&self, id: u32, value: u32) -> Result<(), String> {
        self.set_in(self.base_profile()?, id, value)?;
        self.save()
    }

    fn set_in(&self, profile: Handle, id: u32, value: u32) -> Result<(), String> {
        unsafe {
            let set: unsafe extern "C" fn(Handle, Handle, *mut Setting) -> Status = self.func(DRS_SET_SETTING)?;
            let mut s: Box<Setting> = Box::new(std::mem::zeroed());
            s.version = SETTING_SIZE as u32 | (1 << 16);
            s.id = id;
            s.kind = 0; // NVDRS_DWORD_TYPE
            s.current[..4].copy_from_slice(&value.to_le_bytes());
            check(set(self.session, profile, &mut *s), "change the setting")
        }
    }

    fn save(&self) -> Result<(), String> {
        unsafe {
            let save: unsafe extern "C" fn(Handle) -> Status = self.func(DRS_SAVE_SETTINGS)?;
            check(save(self.session), "save the settings")
        }
    }

    /// The profile the driver uses for this exe (full path), if it has one.
    fn find_app(&self, exe: &str) -> Result<Option<Handle>, String> {
        unsafe {
            let find: unsafe extern "C" fn(Handle, *const u16, *mut Handle, *mut Application) -> Status =
                self.func(DRS_FIND_APPLICATION)?;
            let mut app: Box<Application> = Box::new(std::mem::zeroed());
            app.version = APPLICATION_SIZE as u32 | (4 << 16);
            let mut profile = std::ptr::null_mut();
            match find(self.session, name(exe).as_ptr(), &mut profile, &mut *app) {
                0 => Ok(Some(profile)),
                EXECUTABLE_NOT_FOUND | PROFILE_NOT_FOUND => Ok(None),
                st => check(st, "find the game's profile").map(|_| None),
            }
        }
    }

    /// The game's profile, creating "Kestrel: <exe>" with the exe in it when the driver has none.
    fn app_profile(&self, exe: &str) -> Result<Handle, String> {
        if let Some(p) = self.find_app(exe)? {
            return Ok(p);
        }
        let file = std::path::Path::new(exe).file_name().map_or(exe.to_string(), |f| f.to_string_lossy().to_lowercase());
        unsafe {
            let create: unsafe extern "C" fn(Handle, *mut Profile, *mut Handle) -> Status = self.func(DRS_CREATE_PROFILE)?;
            let find: unsafe extern "C" fn(Handle, *const u16, *mut Handle) -> Status = self.func(DRS_FIND_PROFILE)?;
            let add: unsafe extern "C" fn(Handle, Handle, *mut Application) -> Status = self.func(DRS_CREATE_APPLICATION)?;
            let mut info: Box<Profile> = Box::new(std::mem::zeroed());
            info.version = PROFILE_SIZE as u32 | (1 << 16);
            info.name = name(&format!("{OWN_PROFILE}{file}"));
            let mut profile = std::ptr::null_mut();
            match create(self.session, &mut *info, &mut profile) {
                // Left from an earlier run without the exe in it.
                PROFILE_NAME_IN_USE => check(find(self.session, info.name.as_ptr(), &mut profile), "open the game's profile")?,
                st => check(st, "create a profile for the game")?,
            }
            let mut app: Box<Application> = Box::new(std::mem::zeroed());
            app.version = APPLICATION_SIZE as u32 | (4 << 16);
            app.app_name = name(&file);
            check(add(self.session, profile, &mut *app), "add the game to its profile")?;
            Ok(profile)
        }
    }

    /// Removes the profile's own value; for a profile Kestrel created, the whole profile once it's empty.
    fn clear_in(&self, profile: Handle, id: u32) -> Result<(), String> {
        unsafe {
            let delete: unsafe extern "C" fn(Handle, Handle, u32) -> Status = self.func(DRS_DELETE_SETTING)?;
            match delete(self.session, profile, id) {
                SETTING_NOT_FOUND => {}
                st => check(st, "reset the setting")?,
            }
            let info_fn: unsafe extern "C" fn(Handle, Handle, *mut Profile) -> Status = self.func(DRS_GET_PROFILE_INFO)?;
            let mut info: Box<Profile> = Box::new(std::mem::zeroed());
            info.version = PROFILE_SIZE as u32 | (1 << 16);
            if info_fn(self.session, profile, &mut *info) == 0 && info.num_settings == 0 && read_name(&info.name).starts_with(OWN_PROFILE) {
                let drop_profile: unsafe extern "C" fn(Handle, Handle) -> Status = self.func(DRS_DELETE_PROFILE)?;
                check(drop_profile(self.session, profile), "remove Kestrel's profile for the game")?;
            }
            Ok(())
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

/// Whether this PC has an NVIDIA driver with the settings API (checked once).
pub fn available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| Drs::open().is_ok())
}

/// The game's own value for a driver setting (None: not set for it, so the global setting applies).
pub fn app_setting(exe: &str, id: u32) -> Result<Option<u32>, String> {
    let drs = reader()?;
    match drs.find_app(exe)? {
        Some(p) => drs.get_in(p, id),
        None => Ok(None),
    }
}

thread_local! {
    static READER: std::cell::RefCell<Option<(std::time::Instant, std::rc::Rc<Drs>)>> = const { std::cell::RefCell::new(None) };
}

/// A session shared by reads made close together (opening one takes about 130 ms), so refreshing several games
/// doesn't open one per game and setting. Dropped after two seconds and after every change.
fn reader() -> Result<std::rc::Rc<Drs>, String> {
    READER.with(|r| {
        let mut r = r.borrow_mut();
        if let Some((at, drs)) = &*r {
            if at.elapsed() < std::time::Duration::from_secs(2) {
                return Ok(drs.clone());
            }
        }
        let drs = std::rc::Rc::new(Drs::open()?);
        *r = Some((std::time::Instant::now(), drs.clone()));
        Ok(drs)
    })
}

/// Sets the game's own value (None: back to the global or NVIDIA's value for it) and saves.
/// Needs administrator rights.
pub fn set_app_setting(exe: &str, id: u32, value: Option<u32>) -> Result<(), String> {
    READER.with(|r| r.borrow_mut().take());
    let drs = Drs::open()?;
    match value {
        Some(v) => drs.set_in(drs.app_profile(exe)?, id, v)?,
        None => match drs.find_app(exe)? {
            Some(p) => drs.clear_in(p, id)?,
            None => return Ok(()),
        },
    }
    drs.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        assert_eq!(read_name(&name("Kestrel: rdr2.exe")), "Kestrel: rdr2.exe");
        assert_eq!(read_name(&name(&"x".repeat(3000))).len(), 2047);
    }

    #[test]
    #[ignore = "reads this PC's NVIDIA driver settings (read-only)"]
    fn live_read() {
        eprintln!("shader cache MB: {:?}", shader_cache());
        // KESTREL_NV_EXE=<full path of a game's exe>: its profile and own values, read-only.
        if let Ok(exe) = std::env::var("KESTREL_NV_EXE") {
            eprintln!("has a profile: {:?}", Drs::open().unwrap().find_app(&exe).map(|p| p.is_some()));
            eprintln!("power mode: {:?}, pre-rendered frames: {:?}", app_setting(&exe, POWER_MODE), app_setting(&exe, PRERENDER_LIMIT));
        }
    }

    #[test]
    #[ignore = "changes NVIDIA driver settings for a made-up exe and undoes it; run as administrator"]
    fn live_app_round_trip() {
        let exe = r"C:\KestrelTest\kestrel-test.exe";
        let t = std::time::Instant::now();
        assert_eq!(app_setting(exe, POWER_MODE).unwrap(), None);
        eprintln!("one read: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let _ = app_setting(exe, PRERENDER_LIMIT);
        eprintln!("next read (shared session): {:?}", t.elapsed());
        set_app_setting(exe, POWER_MODE, Some(1)).unwrap();
        set_app_setting(exe, PRERENDER_LIMIT, Some(1)).unwrap();
        assert_eq!(app_setting(exe, POWER_MODE).unwrap(), Some(1));
        assert_eq!(app_setting(exe, PRERENDER_LIMIT).unwrap(), Some(1));
        set_app_setting(exe, POWER_MODE, None).unwrap();
        assert_eq!(app_setting(exe, POWER_MODE).unwrap(), None);
        assert_eq!(app_setting(exe, PRERENDER_LIMIT).unwrap(), Some(1));
        set_app_setting(exe, PRERENDER_LIMIT, None).unwrap();
        // Kestrel's profile is gone again.
        assert!(Drs::open().unwrap().find_app(exe).unwrap().is_none());
    }
}
