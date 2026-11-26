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
/// "Enable DLSS-SR override" (1 = on) and "Override DLSS-SR presets" (0x00FFFFFF = the latest model), the
/// settings behind the NVIDIA App's DLSS Override.
pub const DLSS_SR_OVERRIDE: u32 = 0x10E4_1E01;
pub const DLSS_SR_PRESET: u32 = 0x10E4_1DF3;
pub const DLSS_PRESET_LATEST: u32 = 0x00FF_FFFF;
/// "Vertical Sync" (Force on) and "Frame Rate Limiter" (frames per second), for the G-SYNC setup.
const VSYNC_MODE: u32 = 0x00A8_79CF;
const VSYNC_FORCE_ON: u32 = 0x4781_4940;
const VSYNC_FORCE_OFF: u32 = 0x0841_6747;
/// "Preferred refresh rate": 1 = Highest available.
const REFRESH_RATE: u32 = 0x0064_B541;
// "Texture filtering": Quality (20 = High performance), Anisotropic sample and filter optimizations.
const TEX_QUALITY: u32 = 0x00CE_2691;
const TEX_ANISO_SAMPLE_OPT: u32 = 0x00E7_3211;
const TEX_ANISO_FILTER_OPT: u32 = 0x0084_CD70;
const FRAME_LIMIT: u32 = 0x1083_5002;
// NvAPI display functions: the main display, and its variable refresh (G-SYNC) state.
const GET_PRIMARY_DISPLAY: u32 = 0x1E9D_8A31;
const GET_VRR_INFO: u32 = 0xDF8F_DA57;

/// NV_GET_VRR_INFO (version 1): bit 0 of `flags` is bIsVRREnabled, bit 1 bIsVRRPossible, bit 2 bIsVRRRequested.
#[repr(C)]
struct VrrInfo {
    version: u32,
    flags: u32,
    reserved: [u32; 4],
}
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

    /// NV_GET_VRR_INFO flags for the main display.
    fn primary_vrr(&self) -> Result<u32, String> {
        unsafe {
            let primary: unsafe extern "C" fn(*mut u32) -> Status = self.func(GET_PRIMARY_DISPLAY)?;
            let vrr: unsafe extern "C" fn(u32, *mut VrrInfo) -> Status = self.func(GET_VRR_INFO)?;
            let mut id = 0u32;
            check(primary(&mut id), "find the main display")?;
            let mut info = VrrInfo { version: std::mem::size_of::<VrrInfo>() as u32 | (1 << 16), flags: 0, reserved: [0; 4] };
            check(vrr(id, &mut info), "read the display's G-SYNC state")?;
            Ok(info.flags)
        }
    }

    fn base_profile(&self) -> Result<*mut c_void, String> {
        unsafe {
            let get: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> Status = self.func(DRS_GET_BASE_PROFILE)?;
            let mut profile = std::ptr::null_mut();
            check(get(self.session, &mut profile), "open the global profile")?;
            Ok(profile)
        }
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

    /// A setting's value in this profile, 32-bit or binary (None when the profile doesn't set it).
    fn get_val_in(&self, profile: Handle, id: u32) -> Result<Option<Val>, String> {
        unsafe {
            let get: unsafe extern "C" fn(Handle, Handle, u32, *mut Setting) -> Status = self.func(DRS_GET_SETTING)?;
            let mut s: Box<Setting> = Box::new(std::mem::zeroed());
            s.version = SETTING_SIZE as u32 | (1 << 16);
            match get(self.session, profile, id, &mut *s) {
                SETTING_NOT_FOUND => Ok(None),
                0 if s.location != 0 => Ok(None),
                // NVDRS_BINARY_TYPE: NVDRS_BINARY_SETTING { valueLength, valueData }; up to 8 bytes as a number.
                0 if s.kind == 1 => {
                    let len = (u32::from_le_bytes(s.current[..4].try_into().unwrap()) as usize).min(8);
                    let mut b = [0u8; 8];
                    b[..len].copy_from_slice(&s.current[4..4 + len]);
                    Ok(Some(Val::B(u64::from_le_bytes(b))))
                }
                0 => Ok(Some(Val::D(u32::from_le_bytes(s.current[..4].try_into().unwrap())))),
                st => check(st, "read the setting").map(|_| None),
            }
        }
    }

    fn set_val_in(&self, profile: Handle, id: u32, value: Val) -> Result<(), String> {
        let v = match value {
            Val::D(v) => return self.set_in(profile, id, v),
            Val::B(v) => v,
        };
        unsafe {
            let set: unsafe extern "C" fn(Handle, Handle, *mut Setting) -> Status = self.func(DRS_SET_SETTING)?;
            let mut s: Box<Setting> = Box::new(std::mem::zeroed());
            s.version = SETTING_SIZE as u32 | (1 << 16);
            s.id = id;
            s.kind = 1; // NVDRS_BINARY_TYPE
            s.current[..4].copy_from_slice(&8u32.to_le_bytes());
            s.current[4..12].copy_from_slice(&v.to_le_bytes());
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

/// Whether this PC has an NVIDIA driver with the settings API (checked once).
pub fn available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| Drs::open().is_ok())
}

/// A driver setting's value: 32-bit (most) or binary (up to 8 bytes, like the Resizable BAR size limit).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Val {
    D(u32),
    B(u64),
}

impl Val {
    /// For saved originals: decimal for 32-bit values, "x" and hex for binary ones.
    fn to_text(self) -> String {
        match self {
            Val::D(v) => v.to_string(),
            Val::B(v) => format!("x{v:x}"),
        }
    }

    fn parse(s: &str) -> Option<Val> {
        match s.strip_prefix('x') {
            Some(h) => u64::from_str_radix(h, 16).ok().map(Val::B),
            None => s.parse().ok().map(Val::D),
        }
    }
}

/// RTX series of this PC's NVIDIA card (20, 30, 40, 50…), looked up once.
pub fn series() -> Option<u32> {
    static SERIES: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *SERIES.get_or_init(|| crate::nvdriver::detect().and_then(|g| rtx_series(&g.name)))
}

// Settings behind the Max FPS switches. Resizable BAR and Smooth Motion aren't in NVIDIA's public header; their IDs
// are from NVIDIA Profile Inspector's settings list (CustomSettingNames.xml).
const REBAR_ENABLE: u32 = 0x000F_00BA;
const REBAR_ENABLE_V2: u32 = 0x000B_FA21;
const REBAR_OPTIONS: u32 = 0x000F_00BB;
const REBAR_SIZE_LIMIT: u32 = 0x000F_00FF;
const SMOOTH_MOTION: u32 = 0xB0D3_84C0;
const SMOOTH_MOTION_APIS: u32 = 0xB0CC_0875;
const DLSS_SR_MODE: u32 = 0x10AF_B768;
const DLSS_FG_OVERRIDE: u32 = 0x10E4_1E03;
const DLSSG_MULTI_FRAME_COUNT: u32 = 0x104D_6667;

/// Opt-in settings that raise FPS at a cost, each in the driver's global profile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Boost {
    Rebar,
    SmoothMotion,
    DlssPerformance,
    FrameGenMax,
    TexturePerformance,
    VsyncOff,
}

impl Boost {
    fn values(self) -> &'static [(u32, Val)] {
        match self {
            // Enabled (v1 and v2 switches), options 1, and a 1 GB size limit, as Profile Inspector guides set it.
            Boost::Rebar => &[
                (REBAR_ENABLE, Val::D(1)),
                (REBAR_ENABLE_V2, Val::D(2)),
                (REBAR_OPTIONS, Val::D(1)),
                (REBAR_SIZE_LIMIT, Val::B(0x4000_0000)),
            ],
            // On, for DirectX 12 and DirectX 11 games.
            Boost::SmoothMotion => &[(SMOOTH_MOTION, Val::D(1)), (SMOOTH_MOTION_APIS, Val::D(3))],
            // DLSS Super Resolution override on, in Performance mode (0).
            Boost::DlssPerformance => &[(DLSS_SR_OVERRIDE, Val::D(1)), (DLSS_SR_MODE, Val::D(0))],
            // Frame Generation override on, three generated frames per rendered one (4x).
            Boost::FrameGenMax => &[(DLSS_FG_OVERRIDE, Val::D(1)), (DLSSG_MULTI_FRAME_COUNT, Val::D(3))],
            // The Control Panel's "High performance" texture filtering with both optimizations on.
            Boost::TexturePerformance => &[(TEX_QUALITY, Val::D(20)), (TEX_ANISO_SAMPLE_OPT, Val::D(1)), (TEX_ANISO_FILTER_OPT, Val::D(1))],
            Boost::VsyncOff => &[(VSYNC_MODE, Val::D(VSYNC_FORCE_OFF))],
        }
    }

    /// Why it can't be used on this PC, if it can't. Cached: called while drawing the settings list.
    pub fn unavailable(self) -> Option<&'static str> {
        if !available() {
            return Some("Needs an NVIDIA graphics card and driver.");
        }
        let s = series().unwrap_or(0);
        match self {
            Boost::Rebar if s < 30 => Some("Needs an NVIDIA RTX 30 series card or newer."),
            Boost::Rebar if !bios_rebar() => {
                Some("Turn on Resizable BAR (and Above 4G Decoding) in the BIOS first; Found on this PC explains how.")
            }
            Boost::SmoothMotion if s < 40 => Some("Needs an NVIDIA RTX 40 series card or newer."),
            Boost::DlssPerformance if s < 20 => Some("Needs an NVIDIA RTX card (DLSS)."),
            Boost::FrameGenMax if s < 50 => Some("Needs an NVIDIA RTX 50 series card (Multi Frame Generation)."),
            // With G-SYNC the gaming profile sets VSync on, the lowest-latency setup without tearing.
            Boost::VsyncOff if gsync_on() => Some("G-SYNC is on: the NVIDIA gaming profile uses VSync with a frame cap instead."),
            _ => None,
        }
    }

    /// The settings as they are now, for the saved original ("-" where the global profile doesn't set one).
    pub fn snapshot(self) -> Result<String, String> {
        let drs = Drs::open()?;
        let base = drs.base_profile()?;
        let vals: Result<Vec<String>, String> =
            self.values().iter().map(|(id, _)| Ok(drs.get_val_in(base, *id)?.map_or("-".into(), Val::to_text))).collect();
        Ok(vals?.join(","))
    }

    pub fn apply(self) -> Result<(), String> {
        let drs = Drs::open()?;
        let base = drs.base_profile()?;
        for (id, v) in self.values() {
            drs.set_val_in(base, *id, *v)?;
        }
        drs.save()
    }

    /// Puts the saved original back. The DLSS override switch stays on while the gaming profile uses it.
    pub fn restore(self, saved: &str) -> Result<(), String> {
        let drs = Drs::open()?;
        let base = drs.base_profile()?;
        for ((id, _), p) in self.values().iter().zip(saved.split(',')) {
            if *id == DLSS_SR_OVERRIDE && dlss_model_is_free() && profile_applied() {
                continue;
            }
            match p {
                "-" => drs.clear_in(base, *id)?,
                p => drs.set_val_in(base, *id, Val::parse(p).ok_or("Saved value is invalid.")?)?,
            }
        }
        drs.save()
    }

    pub fn is_applied(self) -> bool {
        let Ok(drs) = Drs::open() else { return false };
        let Ok(base) = drs.base_profile() else { return false };
        self.values().iter().all(|(id, v)| drs.get_val_in(base, *id).is_ok_and(|now| now == Some(*v)))
    }
}

/// G-SYNC on for the main display, checked once (for the settings list, drawn every frame).
fn gsync_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| gsync_frame_cap().is_some())
}

/// Resizable BAR on in the BIOS (Windows reports GPU upload heaps), checked once.
fn bios_rebar() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| super::hardware::resizable_bar().is_some_and(|(_, on)| on))
}

/// Whether the card is an RTX 40 series or newer, where the newest DLSS model costs next to no FPS (on RTX 20
/// and 30 cards it costs a few percent, so the gaming profile leaves DLSS alone there). Checked once.
pub fn dlss_model_is_free() -> bool {
    static NEW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *NEW.get_or_init(|| crate::nvdriver::detect().is_some_and(|g| rtx_series(&g.name).is_some_and(|s| s >= 40)))
}

/// 20, 30, 40, 50… from a name like "NVIDIA GeForce RTX 4070 Ti SUPER".
fn rtx_series(name: &str) -> Option<u32> {
    let model = name.split("RTX ").nth(1)?.split(|c: char| !c.is_ascii_digit()).next()?;
    (model.len() == 4).then(|| model[..2].parse().ok()).flatten()
}

/// The global settings Kestrel's NVIDIA gaming profile manages, in the order its saved original lists them.
/// Settings added later go at the end, so originals saved by earlier versions still line up.
const PROFILE: [u32; 8] =
    [SHADER_CACHE_SIZE, POWER_MODE, PRERENDER_LIMIT, DLSS_SR_OVERRIDE, DLSS_SR_PRESET, VSYNC_MODE, FRAME_LIMIT, REFRESH_RATE];

/// The global profile's own values of these settings (None where it doesn't set one).
fn global(ids: &[u32]) -> Result<Vec<Option<u32>>, String> {
    let drs = Drs::open()?;
    let base = drs.base_profile()?;
    ids.iter().map(|id| drs.get_in(base, *id)).collect()
}

/// Sets (Some) or clears (None) global settings in one session, then saves.
fn set_global(values: &[(u32, Option<u32>)]) -> Result<(), String> {
    let drs = Drs::open()?;
    let base = drs.base_profile()?;
    for (id, v) in values {
        match v {
            Some(v) => drs.set_in(base, *id, *v)?,
            None => drs.clear_in(base, *id)?,
        }
    }
    drs.save()
}

/// What the gaming profile sets, given the current shader cache size (a bigger one, or unlimited, is kept).
fn profile_values(cache_now: Option<u32>) -> Vec<(u32, Option<u32>)> {
    let mut v = vec![
        (SHADER_CACHE_SIZE, Some(cache_now.unwrap_or(0).max(SHADER_CACHE_TARGET_MB))),
        // Prefer maximum performance; Low Latency Mode On; fullscreen games at the screen's highest refresh rate.
        (POWER_MODE, Some(1)),
        (PRERENDER_LIMIT, Some(1)),
        (REFRESH_RATE, Some(1)),
    ];
    if dlss_model_is_free() {
        v.extend([(DLSS_SR_OVERRIDE, Some(1)), (DLSS_SR_PRESET, Some(DLSS_PRESET_LATEST))]);
    }
    // G-SYNC's lowest-latency, tear-free setup: VSync on in the driver plus a cap just under the refresh rate.
    if let Some(cap) = gsync_frame_cap() {
        v.extend([(VSYNC_MODE, Some(VSYNC_FORCE_ON)), (FRAME_LIMIT, Some(cap))]);
    }
    v
}

/// The main display's refresh rate in Hz.
fn primary_refresh() -> Option<u32> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};
    let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
    unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut dm) }
        .as_bool()
        .then_some(dm.dmDisplayFrequency)
        .filter(|hz| *hz > 1)
}

/// A frame cap a little under the refresh rate, as NVIDIA Reflex picks it (144 Hz -> 138, 165 -> 157, 240 -> 224),
/// so frame times stay inside G-SYNC's range and VSync never adds its queue.
fn gsync_cap(hz: u32) -> u32 {
    hz - (hz * hz).div_ceil(3600)
}

/// The frame cap for the G-SYNC setup, when G-SYNC (or G-SYNC Compatible) is on for the main display.
pub fn gsync_frame_cap() -> Option<u32> {
    let flags = Drs::open().ok()?.primary_vrr().ok()?;
    (flags & 1 != 0).then(primary_refresh).flatten().map(gsync_cap)
}

/// The profile's settings as they are now, for the saved original: comma-separated, "-" where unset.
pub fn profile_snapshot() -> Result<String, String> {
    Ok(global(&PROFILE)?.iter().map(|v| v.map_or("-".into(), |v| v.to_string())).collect::<Vec<_>>().join(","))
}

pub fn apply_profile() -> Result<(), String> {
    let cache = global(&[SHADER_CACHE_SIZE])?[0];
    set_global(&profile_values(cache))
}

/// Puts a saved original back. A plain number is an original saved by versions before 0.28, which only
/// changed the shader cache.
pub fn restore_profile(saved: &str) -> Result<(), String> {
    if !saved.contains(',') {
        let mb = saved.parse::<u32>().map_err(|_| "Saved value is invalid.".to_string())?;
        return set_global(&[(SHADER_CACHE_SIZE, Some(mb))]);
    }
    let parts: Vec<&str> = saved.split(',').collect();
    let values: Result<Vec<(u32, Option<u32>)>, String> = PROFILE
        .iter()
        // Only what the original lists: settings added in later versions weren't changed, so they're left alone.
        .zip(parts.iter())
        .map(|(id, p)| match *p {
            "-" => Ok((*id, None)),
            p => p.parse::<u32>().map(|v| (*id, Some(v))).map_err(|_| "Saved value is invalid.".to_string()),
        })
        .collect();
    set_global(&values?)
}

pub fn profile_applied() -> bool {
    let Ok(now) = global(&PROFILE) else { return false };
    let want = profile_values(now[0]);
    now[0].is_some_and(|mb| mb >= SHADER_CACHE_TARGET_MB)
        && want.iter().skip(1).all(|(id, v)| PROFILE.iter().position(|p| p == id).is_some_and(|i| now[i] == *v))
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
    #[ignore = "applies the NVIDIA gaming profile to this PC's global driver settings and puts the originals back"]
    fn live_profile_round_trip() {
        let before = profile_snapshot().unwrap();
        println!("before: {before} (applied: {})", profile_applied());
        apply_profile().unwrap();
        let during = profile_snapshot().unwrap();
        println!("during: {during} (applied: {})", profile_applied());
        assert!(profile_applied());
        restore_profile(&before).unwrap();
        assert_eq!(profile_snapshot().unwrap(), before, "originals back exactly");
        println!("restored: {before}");
    }

    #[test]
    fn values_round_trip() {
        for v in [Val::D(0), Val::D(1), Val::D(0x4781_4940), Val::B(0x4000_0000), Val::B(0)] {
            assert_eq!(Val::parse(&v.to_text()), Some(v));
        }
        assert_eq!(Val::parse("bad"), None);
    }

    #[test]
    #[ignore = "reads this PC's NVIDIA global settings for the Max FPS switches (read-only)"]
    fn live_boosts() {
        for b in [Boost::Rebar, Boost::SmoothMotion, Boost::DlssPerformance, Boost::FrameGenMax, Boost::TexturePerformance, Boost::VsyncOff]
        {
            println!("{b:?}: unavailable {:?}, now {:?}, applied {}", b.unavailable(), b.snapshot(), b.is_applied());
        }
    }

    #[test]
    #[ignore = "applies the texture filtering switch to this PC's global driver settings and puts the originals back"]
    fn live_boost_round_trip() {
        let b = Boost::TexturePerformance;
        let before = b.snapshot().unwrap();
        b.apply().unwrap();
        println!("before {before}, during {} (applied {})", b.snapshot().unwrap(), b.is_applied());
        assert!(b.is_applied());
        b.restore(&before).unwrap();
        assert_eq!(b.snapshot().unwrap(), before, "originals back exactly");
    }

    #[test]
    fn gsync_caps_match_reflex() {
        assert_eq!([60, 120, 144, 165, 240, 360].map(gsync_cap), [59, 116, 138, 157, 224, 324]);
    }

    #[test]
    #[ignore = "reads this PC's main display and its G-SYNC state"]
    fn live_gsync() {
        let drs = Drs::open().unwrap();
        println!("vrr flags {:#b}, refresh {:?}, cap {:?}", drs.primary_vrr().unwrap(), primary_refresh(), gsync_frame_cap());
    }

    #[test]
    fn rtx_series_from_names() {
        assert_eq!(rtx_series("NVIDIA GeForce RTX 2070 SUPER"), Some(20));
        assert_eq!(rtx_series("NVIDIA GeForce RTX 4070 Ti SUPER"), Some(40));
        assert_eq!(rtx_series("NVIDIA GeForce RTX 5090 D"), Some(50));
        assert_eq!(rtx_series("NVIDIA GeForce GTX 1080"), None);
        assert_eq!(rtx_series("NVIDIA RTX A4000"), None);
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(read_name(&name("Kestrel: rdr2.exe")), "Kestrel: rdr2.exe");
        assert_eq!(read_name(&name(&"x".repeat(3000))).len(), 2047);
    }

    #[test]
    #[ignore = "reads this PC's NVIDIA driver settings (read-only)"]
    fn live_read() {
        eprintln!("gaming profile settings: {:?}", profile_snapshot());
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
