//! Reversible settings. Each tweak only changes documented per-user settings
//! (the same ones the Settings app toggles), powercfg, or SystemParametersInfo.
//! The original value is saved to backup.txt before the first change.

use std::ffi::c_void;

use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, ANIMATIONINFO, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETANIMATION, SPI_GETMOUSE, SPI_SETANIMATION,
    SPI_SETMOUSE, SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::system::{broadcast_setting_change, run_hidden};
use crate::utils::{backup, log};

const HIGH_PERFORMANCE: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
const BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
const SUB_USB: &str = "2a737441-1930-4402-8d77-b2bebba308a3";
const USB_SELECTIVE_SUSPEND: &str = "48e6b7a6-50f5-4782-a5d4-53bb8f07e226";
const DX_PREFS: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
const DX_GLOBAL: &str = "DirectXUserGlobalSettings";
const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const EXPLORER_ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

/// Declaration order = `ALL` order = display order (grouped by category).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tweak {
    GameMode,
    GameDvrOff,
    NoBackgroundApps,
    NoTransparency,
    NoAnimations,
    WindowedGames,
    PowerMode,
    HighPerfPower,
    UsbSuspendOff,
    NoMouseAccel,
    // Machine-wide (administrator, take effect after a restart where noted). Never in a preset.
    GpuScheduling,
    DeliveryUploadOff,
    /// Security trade-off: offered only in the gaming setup, behind its own warning.
    MemoryIntegrityOff,
    /// NVIDIA driver setting (global), through the NvAPI driver-settings interface.
    NvShaderCache,
    /// Fewer svchost processes, same work. Machine-wide.
    ServiceGrouping,
    /// Extreme profile only, behind explicit confirmation.
    UpdatesOff,
    /// A fix for flicker or stutter, not a speed-up: Advanced only, never in a profile.
    MpoOff,
    // Privacy (Privacy page only; never in a preset).
    PrivacyAdId,
    PrivacyTailored,
    PrivacyLanguageList,
    PrivacySuggestions,
    PrivacyStart,
    PrivacyInking,
    PrivacySpeech,
}

pub struct Info {
    pub title: &'static str,
    pub purpose: &'static str,
    pub benefit: &'static str,
    pub drawback: &'static str,
}

impl Tweak {
    pub const ALL: [Tweak; 24] = [
        Tweak::GameMode,
        Tweak::GameDvrOff,
        Tweak::NoBackgroundApps,
        Tweak::NoTransparency,
        Tweak::NoAnimations,
        Tweak::WindowedGames,
        Tweak::PowerMode,
        Tweak::HighPerfPower,
        Tweak::UsbSuspendOff,
        Tweak::NoMouseAccel,
        Tweak::GpuScheduling,
        Tweak::DeliveryUploadOff,
        Tweak::MemoryIntegrityOff,
        Tweak::NvShaderCache,
        Tweak::ServiceGrouping,
        Tweak::UpdatesOff,
        Tweak::MpoOff,
        Tweak::PrivacyAdId,
        Tweak::PrivacyTailored,
        Tweak::PrivacyLanguageList,
        Tweak::PrivacySuggestions,
        Tweak::PrivacyStart,
        Tweak::PrivacyInking,
        Tweak::PrivacySpeech,
    ];

    /// Shown on the Privacy page instead of Gaming.
    pub fn is_privacy(self) -> bool {
        self as usize >= Tweak::PrivacyAdId as usize
    }

    /// Where its registry values live; anything but the user's own settings needs administrator rights.
    fn hive(self) -> Hive {
        match self {
            Tweak::GpuScheduling | Tweak::MemoryIntegrityOff | Tweak::ServiceGrouping | Tweak::UpdatesOff | Tweak::MpoOff => Hive::Machine,
            Tweak::DeliveryUploadOff => Hive::NetworkService,
            _ => Hive::User,
        }
    }

    pub fn needs_admin(self) -> bool {
        self.hive() != Hive::User
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Why this tweak has no effect on the given Windows build, if it doesn't.
    pub fn unsupported(self, build: u32) -> Option<&'static str> {
        const WIN11: u32 = 22000;
        match self {
            Tweak::WindowedGames if build < WIN11 => Some("Windows 11 only."),
            // Kept only so Revert can undo changes made by older versions.
            Tweak::NoBackgroundApps => Some(
                "No longer offered: it only worked on Windows 10, which Microsoft stopped supporting. On Windows 11, set background access per app in Settings > Apps > Installed apps.",
            ),
            _ => None,
        }
    }

    /// Why changing this setting would do nothing on this PC right now (checked live, so call it off the UI thread).
    pub fn not_needed(self) -> Option<String> {
        match self {
            // Whether the driver answers at all: a setting the global profile leaves unset (after a clean install) is fine.
            Tweak::NvShaderCache => (!super::nvidia::available()).then(|| "Needs an NVIDIA graphics card and driver.".into()),
            // Windows only applies power modes on top of the Balanced plan; other plans set performance themselves.
            Tweak::PowerMode => match active_power_scheme() {
                Ok(plan) if plan != BALANCED => Some(format!(
                    "Power modes only apply to the Balanced power plan, and this PC uses {}. That plan already decides CPU performance.",
                    if plan == HIGH_PERFORMANCE { "High performance".to_string() } else { "another plan".to_string() }
                )),
                _ => None,
            },
            _ => None,
        }
    }

    /// When the change takes effect, if not right away.
    pub fn takes_effect(self) -> Option<&'static str> {
        match self {
            Tweak::WindowedGames => Some("takes effect the next time each game starts"),
            Tweak::GpuScheduling | Tweak::MemoryIntegrityOff | Tweak::ServiceGrouping | Tweak::MpoOff => {
                Some("takes effect after a restart")
            }
            Tweak::NvShaderCache => Some("takes effect the next time each game starts"),
            _ => None,
        }
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            Tweak::PowerMode => "power_mode",
            Tweak::GameMode => "game_mode",
            Tweak::GameDvrOff => "background_recording",
            Tweak::HighPerfPower => "power_plan",
            Tweak::NoTransparency => "transparency",
            Tweak::NoAnimations => "animations",
            Tweak::NoBackgroundApps => "background_apps",
            Tweak::NoMouseAccel => "mouse_accel",
            Tweak::WindowedGames => "windowed_games",
            Tweak::UsbSuspendOff => "usb_suspend",
            Tweak::GpuScheduling => "gpu_scheduling",
            Tweak::DeliveryUploadOff => "delivery_upload",
            Tweak::MemoryIntegrityOff => "memory_integrity",
            Tweak::NvShaderCache => "nvidia_shader_cache",
            Tweak::ServiceGrouping => "service_grouping",
            Tweak::UpdatesOff => "updates_off",
            Tweak::MpoOff => "mpo_off",
            Tweak::PrivacyAdId => "privacy_ad_id",
            Tweak::PrivacyTailored => "privacy_tailored",
            Tweak::PrivacyLanguageList => "privacy_language_list",
            Tweak::PrivacySuggestions => "privacy_suggestions",
            Tweak::PrivacyStart => "privacy_start",
            Tweak::PrivacyInking => "privacy_inking",
            Tweak::PrivacySpeech => "privacy_speech",
        }
    }

    pub fn info(self) -> Info {
        let (title, purpose, benefit, drawback) = match self {
            Tweak::GameMode => (
                "Enable Game Mode",
                "Windows gives the game you're playing priority and holds back Windows Update installs and restart notifications while it runs.",
                "Steadier frame times when other apps are busy.",
                "Little difference on powerful PCs.",
            ),
            Tweak::GameDvrOff => (
                "Turn off background game recording",
                "Stops Xbox Game Bar from constantly recording the last few minutes of gameplay (\"Record what happened\").",
                "Frees GPU encoder time and disk writes while you play.",
                "You can't save a clip of something that just happened. Manual recording still works.",
            ),
            Tweak::PowerMode => (
                "Best performance power mode",
                "Sets Windows 11's power mode (Settings > System > Power) to Best performance for the power source you're on now. Windows then lets the CPU boost sooner and park fewer cores.",
                "Most noticeable on laptops and on desktops using the Balanced plan, where the default mode holds clock speeds back.",
                "More power use and heat. Only the current power source changes (plugged in or battery), and it only applies with the Balanced power plan.",
            ),
            Tweak::HighPerfPower => (
                "High performance power plan",
                "Switches to Windows' built-in High performance plan, which keeps the CPU ready at higher clock speeds.",
                "Faster clock ramp-up and fewer power-saving dips in demanding games.",
                "More power draw, heat and fan noise. Drains laptop batteries quickly, so use it when plugged in. Some laptops don't offer this plan.",
            ),
            Tweak::NoTransparency => (
                "Turn off transparency effects",
                "Makes the taskbar, Start menu and app backgrounds solid instead of see-through.",
                "Less desktop compositing work, mostly noticeable on integrated graphics.",
                "Mostly cosmetic: Windows looks a little flatter, and dedicated graphics cards see no measurable gain.",
            ),
            Tweak::NoAnimations => (
                "Turn off window animations",
                "Disables the animation when windows are minimised and maximised.",
                "Alt-tabbing and window switching feel snappier.",
                "Cosmetic only.",
            ),
            Tweak::NoBackgroundApps => (
                "Limit background Store apps",
                "Stops Microsoft Store apps from running in the background (Windows 10).",
                "Less background CPU, memory and network use.",
                "Apps like Mail, Calendar or Phone Link may not show notifications until you open them.",
            ),
            Tweak::NoMouseAccel => (
                "Turn off \"Enhance pointer precision\"",
                "Disables Windows mouse acceleration so the cursor moves exactly as far as your mouse does.",
                "Consistent aim in games that use the Windows cursor.",
                "Personal preference: the desktop cursor may feel slower. Most modern games use raw input and are unaffected.",
            ),
            Tweak::WindowedGames => (
                "Optimizations for windowed games",
                "Windows 11 setting that lets DirectX 10/11 games in windowed or borderless mode present frames the same efficient way as fullscreen games. Kestrel sets the same per-user value as Settings > Graphics.",
                "Lower input latency in borderless and windowed games, plus G-SYNC/FreeSync support for them.",
                "Windows 11 only. Takes effect the next time each game starts. Rarely, an older game shows glitches; you can exclude a single game on its page under Games (Game Mode tab).",
            ),
            Tweak::UsbSuspendOff => (
                "Turn off USB selective suspend",
                "Stops Windows from putting idle USB devices into low-power mode while plugged in, on the active power plan.",
                "Prevents wake-up delays and dropouts on controllers, headsets and other USB devices.",
                "Slightly higher idle power use. Applies to the current power plan only; a mouse you're using never sleeps anyway.",
            ),
            Tweak::GpuScheduling => (
                "Hardware-accelerated GPU scheduling",
                "Lets the graphics card manage its own work queue instead of Windows (Settings > System > Display > Graphics). Needed for frame generation (DLSS 3 and newer).",
                "Slightly lower latency and steadier frames in many games; required for frame generation.",
                "Needs a restart. Only works on graphics cards that support it (NVIDIA GTX 10-series, AMD RX 5000 and newer); older ones ignore it. Results vary by game.",
            ),
            Tweak::DeliveryUploadOff => (
                "Stop sharing updates with other PCs",
                "Windows can upload pieces of updates to other PCs on the internet (Settings > Windows Update > Advanced options > Delivery Optimization). This turns that off; updates still download normally.",
                "Your upload bandwidth stays free for online games and voice chat.",
                "None for most people. Updates may download a bit slower on a network with several Windows PCs.",
            ),
            Tweak::MemoryIntegrityOff => (
                "Turn off memory integrity",
                "Memory integrity (Windows Security > Device security > Core isolation) uses virtualization to block malicious drivers from changing Windows. It costs some CPU performance.",
                "About 4 to 10 percent more FPS in games limited by the processor, according to independent tests.",
                "Lowers protection against malicious drivers. Some anti-cheat systems and company PCs require it on. Needs a restart.",
            ),
            Tweak::NvShaderCache => (
                "NVIDIA gaming profile",
                "One profile in the driver's global settings (NVIDIA Control Panel > Manage 3D settings), for every game: Power management mode Prefer maximum performance, Low Latency Mode On, Shader Cache Size 10 GB (a bigger size you set is kept), and on RTX 40 and 50 cards the latest DLSS model (RTX 20 and 30 cards keep their DLSS setting, since the newest model costs FPS there). With G-SYNC on for your main display it adds G-SYNC's lowest-latency setup: Vertical sync On and a frame rate cap a little under the refresh rate, as NVIDIA Reflex picks it.",
                "No downclocking mid-game, less input lag in DirectX 11 games when the graphics card is the limit, fewer shader-compile hitches, and a sharper DLSS image at no cost on new cards.",
                "The graphics card may stay at higher clocks on the desktop too, using more power and running warmer. Uses up to 10 GB of disk for shaders. With G-SYNC, FPS is capped just under the refresh rate, and games on a second, non-G-SYNC screen get VSync. Games with NVIDIA Reflex use Reflex instead of Low Latency Mode.",
            ),
            Tweak::ServiceGrouping => (
                "Group Windows services into fewer processes",
                "Since 2017 Windows runs most services in their own svchost process on PCs with over 3.5 GB of memory. This raises that limit above this PC's memory, so services share processes again, as on older Windows and in AtlasOS and xOS.",
                "Far fewer processes and a little less memory. The same services still run, so games don't get faster: it changes the count, not the work.",
                "If one service in a shared process crashes, the others in it stop too; separating them is why Microsoft changed this. Needs a restart.",
            ),
            Tweak::UpdatesOff => (
                "Turn off automatic Windows updates",
                "Sets Microsoft's \"Configure Automatic Updates\" policy to off, so Windows stops downloading and installing updates by itself. You can still update by hand in Settings > Windows Update.",
                "No update downloads, installs or restarts in the background.",
                "Security fixes no longer install by themselves, which leaves the PC exposed until you update. FACEIT and ESEA require current updates. Pause updates (Apps > Updates) is the safer choice for a tournament.",
            ),
            Tweak::MpoOff => (
                "Turn off multi-plane overlay (MPO)",
                "MPO lets the graphics card combine screen layers itself. On some setups it causes flicker, black flashes or stutter, especially with G-SYNC, FreeSync or several monitors. This writes the value from NVIDIA's own fix (Dwm OverlayTestMode = 5).",
                "Stops MPO-related flicker and stutter, if that's what you see. Try updating the graphics driver first.",
                "Only for that problem: it doesn't add FPS, and the screen may use slightly more power. Reports differ on whether Windows 11 24H2 and later still honor it; if nothing changes, Revert. Needs a restart.",
            ),
            Tweak::PrivacyAdId => (
                "Turn off the advertising ID",
                "Apps can no longer use your advertising ID to tie what you do across apps to personalized ads (Settings > Privacy & security > General).",
                "Ads in apps stop being personalized from your activity in other apps.",
                "You still see ads, just less targeted ones.",
            ),
            Tweak::PrivacyTailored => (
                "Turn off tailored experiences",
                "Microsoft stops using your diagnostic data to personalize tips, ads and recommendations (Settings > Privacy & security > Diagnostics & feedback).",
                "Less of your usage data is used for marketing.",
                "Tips and recommendations are less personal.",
            ),
            Tweak::PrivacyLanguageList => (
                "Hide your language list from websites",
                "Websites can no longer read the languages you've set in Windows to show local content (Settings > Privacy & security > General).",
                "One less detail sites can use to recognize you.",
                "A few websites may not pick your language automatically.",
            ),
            Tweak::PrivacySuggestions => (
                "Turn off tips and suggestions",
                "Turns off suggested content in Settings, tips while using Windows, \"finish setting up your device\" prompts, and fun facts and tips on the lock screen.",
                "Fewer promotions and prompts, and less content fetched in the background.",
                "You won't get tips about new Windows features. Your lock screen picture stays.",
            ),
            Tweak::PrivacyStart => (
                "Turn off Start recommendations",
                "Start stops recommending tips, shortcuts and new apps, and stops showing Microsoft account notifications (Settings > Personalization > Start).",
                "A cleaner Start menu without promotions.",
                "Your recently opened files stay in Start. May need a sign-out to fully apply.",
            ),
            Tweak::PrivacyInking => (
                "Turn off inking and typing personalization",
                "Windows stops building a personal dictionary from what you type and handwrite, and stops sending it to Microsoft (Settings > Privacy & security > Inking & typing personalization).",
                "What you type isn't used to build a profile.",
                "Handwriting recognition and typing suggestions learn less from you.",
            ),
            Tweak::PrivacySpeech => (
                "Turn off online speech recognition",
                "Your voice is no longer sent to Microsoft's speech service (Settings > Privacy & security > Speech).",
                "Voice recordings stay on your PC.",
                "Voice typing (Win+H) and other features that need online recognition stop working.",
            ),
        };
        Info { title, purpose, benefit, drawback }
    }

    /// Per-user registry values this tweak sets: (key, value, target).
    fn registry(self) -> &'static [(&'static str, &'static str, u32)] {
        match self {
            Tweak::GameMode => &[(r"Software\Microsoft\GameBar", "AutoGameModeEnabled", 1)],
            // "Record what happened" only; Win+Alt+R and screenshots keep working.
            Tweak::GameDvrOff => &[(r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "HistoricalCaptureEnabled", 0)],
            Tweak::NoTransparency => &[(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "EnableTransparency", 0)],
            Tweak::NoBackgroundApps => &[
                (r"Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications", "GlobalUserDisabled", 1),
                (r"Software\Microsoft\Windows\CurrentVersion\Search", "BackgroundAppGlobalToggle", 0),
            ],
            // The per-user values the matching switches in Settings write.
            Tweak::PrivacyAdId => &[(r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", "Enabled", 0)],
            Tweak::PrivacyTailored => {
                &[(r"Software\Microsoft\Windows\CurrentVersion\Privacy", "TailoredExperiencesWithDiagnosticDataEnabled", 0)]
            }
            Tweak::PrivacyLanguageList => &[(r"Control Panel\International\User Profile", "HttpAcceptLanguageOptOut", 1)],
            Tweak::PrivacySuggestions => &[
                (CDM, "SubscribedContent-338393Enabled", 0),
                (CDM, "SubscribedContent-353694Enabled", 0),
                (CDM, "SubscribedContent-353696Enabled", 0),
                (CDM, "SubscribedContent-338389Enabled", 0),
                (CDM, "SubscribedContent-338387Enabled", 0),
                (CDM, "RotatingLockScreenOverlayEnabled", 0),
                (r"Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement", "ScoobeSystemSettingEnabled", 0),
            ],
            Tweak::PrivacyStart => {
                &[(EXPLORER_ADVANCED, "Start_IrisRecommendations", 0), (EXPLORER_ADVANCED, "Start_AccountNotifications", 0)]
            }
            Tweak::PrivacyInking => &[
                (r"Software\Microsoft\InputPersonalization", "RestrictImplicitInkCollection", 1),
                (r"Software\Microsoft\InputPersonalization", "RestrictImplicitTextCollection", 1),
                (r"Software\Microsoft\InputPersonalization\TrainedDataStore", "HarvestContacts", 0),
                (r"Software\Microsoft\Personalization\Settings", "AcceptedPrivacyPolicy", 0),
            ],
            Tweak::PrivacySpeech => &[(r"Software\Microsoft\Speech_OneCore\Settings\OnlineSpeechPrivacy", "HasAccepted", 0)],
            Tweak::GpuScheduling => &[(r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode", 2)],
            Tweak::UpdatesOff => &[(r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU", "NoAutoUpdate", 1)],
            Tweak::MpoOff => &[(r"SOFTWARE\Microsoft\Windows\Dwm", "OverlayTestMode", 5)],
            // The value the Settings switch writes, in the Network Service account that runs Delivery Optimization.
            Tweak::DeliveryUploadOff => {
                &[(r"S-1-5-20\Software\Microsoft\Windows\CurrentVersion\DeliveryOptimization\Settings", "DownloadMode", 0)]
            }
            Tweak::MemoryIntegrityOff => {
                &[(r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity", "Enabled", 0)]
            }
            _ => &[],
        }
    }

    /// Current state, serialized so it can be written back by `write`.
    pub(crate) fn snapshot(self) -> Result<String, String> {
        Ok(match self {
            // Windows keeps one power mode per power source; remember which one this value belongs to.
            Tweak::PowerMode => format!("{}:{}", overlay::source(), overlay::get()?),
            Tweak::HighPerfPower => active_power_scheme()?,
            Tweak::NoAnimations => get_min_animate()?.to_string(),
            Tweak::NoMouseAccel => get_mouse()?.map(|v| v.to_string()).join(","),
            Tweak::WindowedGames => read_sz(DX_PREFS, DX_GLOBAL).unwrap_or_else(|| "-".into()),
            Tweak::UsbSuspendOff => usb_suspend()?.to_string(),
            Tweak::NvShaderCache => super::nvidia::profile_snapshot()?,
            Tweak::ServiceGrouping => {
                read_dword_in(Hive::Machine, SVCHOST_CONTROL, SVCHOST_THRESHOLD).map_or("-".to_string(), |v| v.to_string())
            }
            _ => self
                .registry()
                .iter()
                .map(|(k, v, _)| read_dword_in(self.hive(), k, v).map_or("-".to_string(), |d| d.to_string()))
                .collect::<Vec<_>>()
                .join(","),
        })
    }

    /// Writes the optimized state (`None`) or a saved snapshot.
    pub(crate) fn write(self, saved: Option<&str>) -> Result<(), String> {
        match self {
            Tweak::PowerMode => {
                let (source, guid) = overlay::split_saved(saved.unwrap_or(overlay::BEST_PERFORMANCE));
                if !is_guid(guid) {
                    return Err("Saved power mode is invalid.".into());
                }
                // Setting it now would overwrite the other source's mode; wait until the PC is back on the saved one.
                if source.is_some_and(|s| s != overlay::source()) {
                    return Err(format!(
                        "This PC was {} since the power mode changed. Kestrel puts it back once it's {} again.",
                        if source == Some("ac") { "unplugged" } else { "plugged in" },
                        if source == Some("ac") { "plugged in" } else { "on battery" }
                    ));
                }
                overlay::set(guid)
            }
            Tweak::HighPerfPower => {
                let guid = match saved {
                    Some(g) => g.to_string(),
                    None => {
                        if !run_hidden("powercfg", &["/list"])?.contains(HIGH_PERFORMANCE) {
                            return Err("This PC doesn't offer the High performance power plan.".into());
                        }
                        HIGH_PERFORMANCE.to_string()
                    }
                };
                if !is_guid(&guid) {
                    return Err("Saved power plan is invalid.".into());
                }
                run_hidden("powercfg", &["/setactive", &guid]).map(|_| ())
            }
            Tweak::NoAnimations => set_min_animate(saved.map_or(Ok(0), parse_i32)?),
            Tweak::NoMouseAccel => {
                let v = match saved {
                    Some(s) => {
                        let p: Vec<i32> = s.split(',').map(parse_i32).collect::<Result<_, _>>()?;
                        <[i32; 3]>::try_from(p).map_err(|_| "Saved mouse setting is invalid.".to_string())?
                    }
                    None => [0, 0, 0],
                };
                set_mouse(v)
            }
            Tweak::WindowedGames => match saved {
                Some("-") => write_sz(DX_PREFS, DX_GLOBAL, None),
                Some(s) => write_sz(DX_PREFS, DX_GLOBAL, Some(s)),
                None => {
                    let current = read_sz(DX_PREFS, DX_GLOBAL).unwrap_or_default();
                    write_sz(DX_PREFS, DX_GLOBAL, Some(&set_kv(&current, "SwapEffectUpgradeEnable", Some("1"))))
                }
            },
            // shortcut: writes to whichever plan is active; restore-all runs in reverse so the plan switch is undone last.
            Tweak::UsbSuspendOff => {
                let value = saved.map_or(Ok(0), |s| s.parse::<u32>().map_err(|_| "Saved value is invalid.".to_string()))?;
                run_hidden("powercfg", &["/setacvalueindex", "SCHEME_CURRENT", SUB_USB, USB_SELECTIVE_SUSPEND, &value.to_string()])?;
                run_hidden("powercfg", &["/setactive", "SCHEME_CURRENT"]).map(|_| ())
            }
            Tweak::ServiceGrouping => {
                let value = match saved {
                    Some("-") => None,
                    Some(s) => Some(s.parse::<u32>().map_err(|_| "Saved value is invalid.".to_string())?),
                    None => Some(grouping_threshold(installed_kb())),
                };
                write_dword_in(Hive::Machine, SVCHOST_CONTROL, SVCHOST_THRESHOLD, value)
            }
            Tweak::NvShaderCache => match saved {
                Some(s) => super::nvidia::restore_profile(s),
                None => super::nvidia::apply_profile(),
            },
            _ => {
                write_registry(self.hive(), self.registry(), saved)?;
                if self == Tweak::NoTransparency {
                    broadcast_setting_change("ImmersiveColorSet");
                }
                Ok(())
            }
        }
    }

    pub fn is_applied(self) -> bool {
        match self {
            Tweak::PowerMode => overlay::get().is_ok_and(|g| g == overlay::BEST_PERFORMANCE),
            // Missing value = Windows' default, which is off.
            Tweak::GameDvrOff => self.registry().iter().all(|(k, v, t)| read_dword(k, v).unwrap_or(0) == *t),
            Tweak::HighPerfPower => active_power_scheme().is_ok_and(|g| g == HIGH_PERFORMANCE),
            Tweak::NoAnimations => get_min_animate().is_ok_and(|v| v == 0),
            Tweak::NoMouseAccel => get_mouse().is_ok_and(|m| m[2] == 0),
            Tweak::WindowedGames => read_sz(DX_PREFS, DX_GLOBAL).is_some_and(|s| get_kv(&s, "SwapEffectUpgradeEnable") == Some("1")),
            Tweak::UsbSuspendOff => usb_suspend().is_ok_and(|v| v == 0),
            // The shader cache is only ever raised: a bigger one (or unlimited) already counts.
            Tweak::NvShaderCache => super::nvidia::profile_applied(),
            // AtlasOS groups service by service instead (SvcHostSplitDisable); that counts too.
            Tweak::ServiceGrouping => {
                read_dword_in(Hive::Machine, SVCHOST_CONTROL, SVCHOST_THRESHOLD).is_some_and(|t| u64::from(t) > installed_kb())
                    || super::analysis::grouped_services() >= 20
            }
            // A Delivery Optimization policy wins over the Settings switch; peering off there counts too.
            Tweak::DeliveryUploadOff => {
                matches!(read_dword_in(Hive::Machine, DO_POLICY, "DODownloadMode"), Some(0 | 99 | 100))
                    || self.registry().iter().all(|(k, v, t)| read_dword_in(self.hive(), k, v) == Some(*t))
            }
            _ => self.registry().iter().all(|(k, v, t)| read_dword_in(self.hive(), k, v) == Some(*t)),
        }
    }

    pub fn has_backup(self) -> bool {
        backup::has(self.key()) || (self == Tweak::GameDvrOff && backup::has(LEGACY_DVR_KEY))
    }

    pub fn apply(self) -> Result<(), String> {
        backup::save_original(self.key(), || self.snapshot())?;
        self.write(None)?;
        log(&format!("Optimizer: applied '{}'", self.info().title));
        Ok(())
    }

    /// Restores the saved original. Returns false when there was nothing to restore.
    pub fn restore(self) -> Result<bool, String> {
        // Versions before 0.7 turned off all capture; put those values back too.
        if self == Tweak::GameDvrOff {
            if let Some(saved) = backup::get(LEGACY_DVR_KEY) {
                write_registry(Hive::User, LEGACY_DVR, Some(&saved))?;
                backup::remove(LEGACY_DVR_KEY)?;
                if !backup::has(self.key()) {
                    log("Optimizer: restored original Game DVR settings saved by an older version");
                    return Ok(true);
                }
            }
        }
        let Some(saved) = backup::get(self.key()) else { return Ok(false) };
        self.write(Some(&saved))?;
        backup::remove(self.key())?;
        log(&format!("Optimizer: restored original setting for '{}'", self.info().title));
        Ok(true)
    }
}

const LEGACY_DVR_KEY: &str = "game_dvr";
const LEGACY_DVR: &[(&str, &str, u32)] =
    &[(r"System\GameConfigStore", "GameDVR_Enabled", 0), (r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "AppCaptureEnabled", 0)];

/// Writes the targets (`None`) or a saved "a,b" snapshot, where "-" means the value didn't exist.
fn write_registry(hive: Hive, entries: &[(&str, &str, u32)], saved: Option<&str>) -> Result<(), String> {
    let saved: Vec<&str> = saved.map(|s| s.split(',').collect()).unwrap_or_default();
    for (i, (k, v, target)) in entries.iter().enumerate() {
        let value = match saved.get(i) {
            None if saved.is_empty() => Some(*target),
            None | Some(&"-") => None,
            Some(s) => Some(s.parse().map_err(|_| "Saved value is invalid.".to_string())?),
        };
        write_dword_in(hive, k, v, value)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hive {
    User,
    Machine,
    /// The Network Service account's settings (HKEY_USERS\S-1-5-20; keys include the SID).
    NetworkService,
}

const SVCHOST_CONTROL: &str = r"SYSTEM\CurrentControlSet\Control";
const SVCHOST_THRESHOLD: &str = "SvcHostSplitThresholdInKB";

/// Installed memory in KB.
fn installed_kb() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
        m.ullTotalPhys / 1024
    } else {
        0
    }
}

/// The next whole GiB above installed memory, in KB: services are grouped whenever memory is below the threshold.
fn grouping_threshold(installed_kb: u64) -> u32 {
    let gib = 1_048_576u64;
    u32::try_from((installed_kb / gib + 1) * gib).unwrap_or(u32::MAX)
}

const DO_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization";

fn root(hive: Hive) -> RegKey {
    use winreg::enums::{HKEY_LOCAL_MACHINE, HKEY_USERS};
    RegKey::predef(match hive {
        Hive::User => HKEY_CURRENT_USER,
        Hive::Machine => HKEY_LOCAL_MACHINE,
        Hive::NetworkService => HKEY_USERS,
    })
}

fn read_dword_in(hive: Hive, key: &str, name: &str) -> Option<u32> {
    root(hive).open_subkey(key).ok()?.get_value(name).ok()
}

fn write_dword_in(hive: Hive, key: &str, name: &str, value: Option<u32>) -> Result<(), String> {
    let (k, _) = root(hive).create_subkey(key).map_err(|e| {
        if hive != Hive::User && e.kind() == std::io::ErrorKind::PermissionDenied {
            "Needs administrator rights (Settings > Restart as administrator).".to_string()
        } else {
            e.to_string()
        }
    })?;
    match value {
        Some(v) => k.set_value(name, &v),
        None => match k.delete_value(name) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        },
    }
    .map_err(|e| e.to_string())
}

fn parse_i32(s: &str) -> Result<i32, String> {
    s.trim().parse().map_err(|_| "Saved value is invalid.".to_string())
}

/// "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" exactly.
fn is_guid(s: &str) -> bool {
    s.len() == 36 && s.char_indices().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() })
}

fn read_dword(key: &str, name: &str) -> Option<u32> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(key).ok()?.get_value(name).ok()
}

pub(crate) fn read_sz(key: &str, name: &str) -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(key).ok()?.get_value(name).ok()
}

pub(crate) fn write_sz(key: &str, name: &str, value: Option<&str>) -> Result<(), String> {
    let (k, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(key).map_err(|e| e.to_string())?;
    match value {
        Some(v) => k.set_value(name, &v),
        None => match k.delete_value(name) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        },
    }
    .map_err(|e| e.to_string())
}

/// Value of `key` in a DirectX preference string like "GpuPreference=2;SwapEffectUpgradeEnable=1;".
pub(crate) fn get_kv<'a>(s: &'a str, key: &str) -> Option<&'a str> {
    s.split(';').find_map(|p| p.split_once('=').filter(|(k, _)| k.trim() == key).map(|(_, v)| v.trim()))
}

/// Sets (or with `None` removes) one entry, keeping every other entry as it was.
pub(crate) fn set_kv(s: &str, key: &str, value: Option<&str>) -> String {
    let mut out: String =
        s.split(';').filter(|p| !p.trim().is_empty() && p.split('=').next().map(str::trim) != Some(key)).map(|p| format!("{p};")).collect();
    if let Some(v) = value {
        out.push_str(&format!("{key}={v};"));
    }
    out
}

/// USB selective suspend AC value on the active plan (1 = enabled, 0 = disabled).
fn usb_suspend() -> Result<u32, String> {
    let out = run_hidden("powercfg", &["/q", "SCHEME_CURRENT", SUB_USB, USB_SELECTIVE_SUSPEND])?;
    // The labels are localized; the first hex value is the AC setting.
    out.split_whitespace()
        .find_map(|t| t.strip_prefix("0x").and_then(|h| u32::from_str_radix(h, 16).ok()))
        .ok_or_else(|| "This power plan has no USB selective suspend setting.".into())
}

/// Windows 11 power mode ("overlay" on top of the Balanced plan). Settings > Power uses these powrprof.dll
/// functions; they aren't in the SDK headers, so they're looked up at run time and missing ones fail cleanly.
mod overlay {
    use windows::core::{s, w, GUID};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};

    pub const BEST_PERFORMANCE: &str = "ded574b5-45a0-4f42-8737-46345c09c238";

    /// "ac" when plugged in (or unknown, as on desktops), "dc" on battery.
    pub fn source() -> &'static str {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut s = SYSTEM_POWER_STATUS::default();
        let on_battery = unsafe { GetSystemPowerStatus(&mut s) }.is_ok() && s.ACLineStatus == 0;
        if on_battery {
            "dc"
        } else {
            "ac"
        }
    }

    /// "ac:<guid>" → (Some("ac"), guid). Values saved before 0.12.1 are a bare GUID (source unknown).
    pub fn split_saved(saved: &str) -> (Option<&str>, &str) {
        match saved.split_once(':') {
            Some((s @ ("ac" | "dc"), guid)) => (Some(s), guid),
            _ => (None, saved),
        }
    }

    fn proc(name: windows::core::PCSTR) -> Result<unsafe extern "system" fn() -> isize, String> {
        unsafe {
            let dll = LoadLibraryExW(w!("powrprof.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).map_err(|e| e.message())?;
            GetProcAddress(dll, name).ok_or_else(|| "This version of Windows has no power mode setting.".into())
        }
    }

    /// The power mode chosen for the current power source, as a lower-case GUID (all zeros = Balanced).
    /// "Actual" rather than "effective": the effective mode reads as Balanced whenever a non-Balanced plan overrides it.
    pub fn get() -> Result<String, String> {
        type Get = unsafe extern "system" fn(*mut GUID) -> u32;
        let f: Get = unsafe { std::mem::transmute(proc(s!("PowerGetActualOverlayScheme"))?) };
        let mut g = GUID::zeroed();
        match unsafe { f(&mut g) } {
            0 => Ok(format!("{g:?}").to_lowercase()),
            e => Err(format!("Couldn't read the power mode (error {e}).")),
        }
    }

    pub fn set(guid: &str) -> Result<(), String> {
        type Set = unsafe extern "system" fn(GUID) -> u32;
        if !super::is_guid(guid) {
            return Err("Power mode is invalid.".into());
        }
        // GUID::from panics on bad input; is_guid has checked the exact layout first.
        let g = GUID::from(guid);
        let f: Set = unsafe { std::mem::transmute(proc(s!("PowerSetActiveOverlayScheme"))?) };
        match unsafe { f(g) } {
            0 => Ok(()),
            e => Err(format!("Windows refused the power mode change (error {e}).")),
        }
    }
}

fn active_power_scheme() -> Result<String, String> {
    let out = run_hidden("powercfg", &["/getactivescheme"])?;
    out.split_whitespace().find(|t| is_guid(t)).map(str::to_lowercase).ok_or_else(|| "Could not read the active power plan.".into())
}

fn spi<T>(action: SYSTEM_PARAMETERS_INFO_ACTION, ui_param: u32, value: &mut T, persist: bool) -> Result<(), String> {
    let flags = if persist { SPIF_UPDATEINIFILE | SPIF_SENDCHANGE } else { SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0) };
    unsafe { SystemParametersInfoW(action, ui_param, Some(value as *mut T as *mut c_void), flags) }.map_err(|e| e.message().to_string())
}

fn get_min_animate() -> Result<i32, String> {
    let mut a = ANIMATIONINFO { cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32, iMinAnimate: 0 };
    spi(SPI_GETANIMATION, a.cbSize, &mut a, false)?;
    Ok(a.iMinAnimate)
}

fn set_min_animate(v: i32) -> Result<(), String> {
    let mut a = ANIMATIONINFO { cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32, iMinAnimate: v };
    spi(SPI_SETANIMATION, a.cbSize, &mut a, true)
}

/// [threshold1, threshold2, acceleration]; acceleration 0 = "Enhance pointer precision" off.
fn get_mouse() -> Result<[i32; 3], String> {
    let mut m = [0i32; 3];
    spi(SPI_GETMOUSE, 0, &mut m, false)?;
    Ok(m)
}

fn set_mouse(mut m: [i32; 3]) -> Result<(), String> {
    spi(SPI_SETMOUSE, 0, &mut m, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_write_and_restore() {
        // A throwaway per-user key exercises the same path the machine-wide tweaks use, without admin.
        const KEY: &str = r"Software\KestrelTest\Tweak";
        let entries: &[(&str, &str, u32)] = &[(KEY, "A", 1), (KEY, "B", 0)];
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(r"Software\KestrelTest");
        write_dword_in(Hive::User, KEY, "A", Some(7)).unwrap();
        let snapshot: Vec<String> =
            entries.iter().map(|(k, v, _)| read_dword_in(Hive::User, k, v).map_or("-".into(), |d| d.to_string())).collect();
        assert_eq!(snapshot.join(","), "7,-");
        write_registry(Hive::User, entries, None).unwrap();
        assert_eq!((read_dword_in(Hive::User, KEY, "A"), read_dword_in(Hive::User, KEY, "B")), (Some(1), Some(0)));
        write_registry(Hive::User, entries, Some(&snapshot.join(","))).unwrap();
        assert_eq!((read_dword_in(Hive::User, KEY, "A"), read_dword_in(Hive::User, KEY, "B")), (Some(7), None));
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(r"Software\KestrelTest");
        assert!(Tweak::MemoryIntegrityOff.needs_admin() && !Tweak::GameMode.needs_admin());
        assert!(!super::super::Preset::Performance.includes(Tweak::MemoryIntegrityOff) && !Tweak::MemoryIntegrityOff.is_privacy());
    }

    #[test]
    fn power_mode_remembers_source() {
        let g = overlay::BEST_PERFORMANCE;
        assert_eq!(overlay::split_saved(&format!("ac:{g}")), (Some("ac"), g));
        assert_eq!(overlay::split_saved(&format!("dc:{g}")), (Some("dc"), g));
        // Saved by 0.10–0.12.0: no source, restored as before.
        assert_eq!(overlay::split_saved(g), (None, g));
        // A value for the other power source is refused rather than written to the wrong slot.
        let other = if overlay::source() == "ac" { "dc" } else { "ac" };
        assert!(Tweak::PowerMode.write(Some(&format!("{other}:{g}"))).unwrap_err().contains("puts it back"));
    }

    #[test]
    fn guid_layout() {
        assert!(is_guid(overlay::BEST_PERFORMANCE));
        assert!(is_guid("00000000-0000-0000-0000-000000000000"));
        assert!(!is_guid("ded574b545a04f42873746345c09c238abcd"));
        assert!(!is_guid("ded574b5-45a0-4f42-8737-46345c09c23g"));
    }

    #[test]
    fn grouping_threshold_is_above_installed_memory() {
        let sixteen_gb = 16_423_652; // what this PC reports, a little under 16 GiB
        assert_eq!(super::grouping_threshold(sixteen_gb), 16 * 1_048_576);
        assert!(u64::from(super::grouping_threshold(16 * 1_048_576)) > 16 * 1_048_576);
        assert!(u64::from(super::grouping_threshold(4_000_000)) > 4_000_000);
    }

    #[test]
    fn kv_strings() {
        assert_eq!(set_kv("", "SwapEffectUpgradeEnable", Some("1")), "SwapEffectUpgradeEnable=1;");
        assert_eq!(
            set_kv("VRROptimizeEnable=0;SwapEffectUpgradeEnable=0;", "SwapEffectUpgradeEnable", Some("1")),
            "VRROptimizeEnable=0;SwapEffectUpgradeEnable=1;"
        );
        assert_eq!(set_kv("GpuPreference=2;", "GpuPreference", None), "");
        assert_eq!(get_kv("A=1;GpuPreference=2;", "GpuPreference"), Some("2"));
        assert_eq!(get_kv("A=1;", "GpuPreference"), None);
    }
}
