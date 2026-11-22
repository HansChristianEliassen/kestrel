use std::sync::mpsc::{channel, Receiver};

use eframe::egui;

use crate::cleaner::CleanItem;
use crate::gamemode;
use crate::optimizer::games::Game;
use crate::optimizer::health::Check;
use crate::optimizer::{Preset, Tweak};
use crate::startup::StartupEntry;
use crate::uninstaller::InstalledApp;
use crate::upgrade::apps::AppUpdate;
use crate::upgrade::drivers::DriverUpdate;

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Dashboard,
    Cleaner,
    Apps,
    Upgrades,
    Privacy,
    Startup,
    Gaming,
    GameMode,
    Settings,
}

impl Page {
    pub const ALL: [Page; 9] = [
        Page::Dashboard,
        Page::Cleaner,
        Page::Apps,
        Page::Upgrades,
        Page::Privacy,
        Page::Startup,
        Page::Gaming,
        Page::GameMode,
        Page::Settings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Dashboard => "Overview",
            Page::Cleaner => "Cleanup",
            Page::Apps => "Apps",
            Page::Upgrades => "Updates",
            Page::Privacy => "Privacy",
            Page::Startup => "Startup apps",
            Page::Gaming => "Gaming",
            Page::GameMode => "Game Mode",
            Page::Settings => "Settings",
        }
    }

    pub fn icon(self) -> &'static str {
        use crate::ui::components::icon;
        match self {
            Page::Dashboard => icon::HOME,
            Page::Cleaner => icon::BROOM,
            Page::Apps => icon::APPS,
            Page::Upgrades => icon::UPDATE,
            Page::Privacy => icon::SHIELD,
            Page::Startup => icon::POWER,
            Page::Gaming => icon::SPEED,
            Page::GameMode => icon::GAME,
            Page::Settings => icon::SETTINGS,
        }
    }
}

/// One background operation: a short-lived thread whose result is picked up by the UI.
pub struct Job<T>(Option<Receiver<T>>);

impl<T> Default for Job<T> {
    fn default() -> Self {
        Job(None)
    }
}

impl<T: Send + 'static> Job<T> {
    pub fn start(&mut self, ctx: &egui::Context, f: impl FnOnce() -> T + Send + 'static) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
        self.0 = Some(rx);
    }

    pub fn running(&self) -> bool {
        self.0.is_some()
    }

    pub fn poll(&mut self) -> Option<T> {
        let r = self.0.as_ref()?.try_recv().ok()?;
        self.0 = None;
        Some(r)
    }
}

pub enum Action {
    Clean,
    Uninstall(Vec<InstalledApp>),
    ApplyTweaks(Vec<Tweak>),
    RestoreAll,
    /// Swap in the verified update and restart.
    InstallUpdate,
    UpdateApps(Vec<AppUpdate>),
    /// Fresh installs from Apps > Get apps.
    InstallApps(Vec<AppUpdate>),
    /// The automatic fixes suggested after measuring a game.
    ApplyFixes(Vec<crate::optimizer::advice::Fix>),
    /// Reverts the fixes applied from the last measurement.
    UndoFixes(Vec<crate::optimizer::advice::Fix>),
    /// Restore point first, then download and install these drivers from Windows Update.
    InstallDrivers(Vec<DriverUpdate>),
}

pub struct Confirm {
    pub title: String,
    pub body: String,
    pub warning: Option<String>,
    pub ok: &'static str,
    pub action: Action,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum CleanupTab {
    #[default]
    Junk,
    Large,
    Usage,
}

#[derive(Default)]
pub struct CleanerState {
    pub items: Vec<CleanItem>,
    pub scan: Job<Vec<CleanItem>>,
    pub clean: Job<Vec<String>>,
    pub tab: CleanupTab,
    /// None until the large-files search has run once.
    pub large: Option<Vec<(std::path::PathBuf, u64)>>,
    pub large_job: Job<Vec<(std::path::PathBuf, u64)>>,
    /// Disk usage: the folder being shown and its entries, largest first.
    pub usage_dir: Option<std::path::PathBuf>,
    pub usage: Vec<crate::cleaner::usage::Entry>,
    pub usage_job: Job<Vec<crate::cleaner::usage::Entry>>,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum AppsTab {
    #[default]
    Installed,
    Get,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum SortBy {
    #[default]
    Name,
    Size,
}

/// Per app: (app name, folders it may have left behind with their sizes)
pub type Leftovers = Vec<(String, Vec<(std::path::PathBuf, u64)>)>;

#[derive(Default)]
pub struct AppsState {
    pub apps: Vec<InstalledApp>,
    pub note: Option<String>,
    pub load: Job<(Vec<InstalledApp>, Option<String>)>,
    /// One uninstaller after another: (result lines, leftovers).
    pub uninstall: Job<(Vec<String>, Leftovers)>,
    pub leftovers: Leftovers,
    /// Names in the running queue, and the index of the one whose uninstaller is running.
    pub queue: Vec<String>,
    pub queue_pos: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Set by "Stop waiting" while an uninstaller runs (moves on to the next app).
    pub stop_waiting: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Set by "Cancel the rest": no further uninstallers start.
    pub cancel_queue: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Ids (`InstalledApp::id`) ticked for bulk uninstall.
    pub selected: std::collections::HashSet<String>,
    pub search: String,
    pub sort: SortBy,
    pub optional_only: bool,
    pub loaded: bool,
    pub tab: AppsTab,
    /// Get apps: the search box, its results (None before a search) and the ids ticked for installing.
    pub get_query: String,
    pub get_results: Option<Vec<AppUpdate>>,
    pub get_job: Job<Result<Vec<AppUpdate>, String>>,
    pub get_selected: std::collections::HashSet<String>,
}

#[derive(Default)]
pub struct StartupState {
    pub entries: Vec<StartupEntry>,
    pub load: Job<Vec<StartupEntry>>,
    /// (entry index, result) of a switch in progress; Task Scheduler can take a moment.
    pub toggle: Job<(usize, Result<(), String>)>,
    pub loaded: bool,
}

/// (applied, has saved original) per tweak, in `Tweak::ALL` order.
/// Plus why the setting wouldn't do anything on this PC, if it wouldn't.
pub type TweakStatus = Vec<(bool, bool, Option<String>)>;

/// (display device, old Hz, new Hz)
pub type RefreshChange = (String, u32, u32);

#[derive(Clone, Copy, PartialEq)]
pub enum GamingTab {
    Settings,
    Games,
    Measure,
}

/// (game name, exe file name, the measurement with its advice).
pub type MeasureResult = (String, String, Result<(crate::optimizer::measure::Stats, crate::optimizer::advice::Advice), String>);

pub struct Measured {
    pub name: String,
    pub stats: crate::optimizer::measure::Stats,
    /// The measurement before this one, for the comparison.
    pub before: Option<crate::optimizer::measure::Stats>,
    pub advice: crate::optimizer::advice::Advice,
    /// Fixes applied from this result (kept for Undo); the next measurement shows the difference.
    pub applied: Vec<crate::optimizer::advice::Fix>,
}

pub struct GamingState {
    pub preset: Preset,
    pub selected: [bool; Tweak::ALL.len()],
    pub status: TweakStatus,
    pub status_job: Job<TweakStatus>,
    pub checks: Vec<Check>,
    pub checks_job: Job<Vec<Check>>,
    pub apply: Job<Vec<String>>,
    pub tab: GamingTab,
    pub games: Vec<Game>,
    pub games_job: Job<Vec<Game>>,
    pub game_search: String,
    /// Refresh rates just raised and not yet confirmed: (when, changes).
    pub refresh_trial: Option<(std::time::Instant, Vec<RefreshChange>)>,
    /// A running measurement: (game name, exe file name, result).
    /// A running measurement: (game name, exe file name, result with advice).
    pub measure: Job<MeasureResult>,
    /// Game being measured and when it started (for the countdown).
    pub measuring: Option<(String, std::time::Instant)>,
    /// The latest measurement this session, shown on the Measure tab.
    pub last: Option<Measured>,
    /// Game picked on the Measure tab (index into `games`), and the detected running game.
    pub pick: usize,
    pub running: Option<usize>,
    pub running_checked: Option<std::time::Instant>,
    /// Measure was clicked before PresentMon was installed: (game name, exe), started once the install finishes.
    pub pending_measure: Option<(String, String)>,
}

impl Default for GamingState {
    fn default() -> Self {
        GamingState {
            preset: Preset::Balanced,
            selected: Tweak::ALL.map(|t| Preset::Balanced.includes(t)),
            status: Vec::new(),
            status_job: Job::default(),
            checks: Vec::new(),
            checks_job: Job::default(),
            apply: Job::default(),
            tab: GamingTab::Settings,
            games: Vec::new(),
            games_job: Job::default(),
            game_search: String::new(),
            refresh_trial: None,
            measure: Job::default(),
            measuring: None,
            last: None,
            pick: 0,
            running: None,
            running_checked: None,
            pending_measure: None,
        }
    }
}

pub fn tweak_status() -> TweakStatus {
    Tweak::ALL.iter().map(|t| (t.is_applied(), t.has_backup(), t.not_needed())).collect()
}

#[derive(Default, PartialEq)]
pub enum OptStage {
    #[default]
    Closed,
    Scanning,
    Review,
    Confirm,
    Applying,
    Done(Vec<String>),
}

/// Why Game Mode is on.
pub struct GameModeOn {
    pub profile: gamemode::Profile,
    /// File name of the game that turned it on automatically; None when turned on by hand.
    pub game: Option<String>,
    /// That game has been seen running (a session started with Play waits for the game to start).
    pub seen: bool,
}

pub struct GameModeState {
    pub config: gamemode::Config,
    /// Result lines, and whether Game Mode changes are still applied afterwards.
    pub job: Job<(Vec<String>, bool)>,
    pub on: Option<GameModeOn>,
    pub hook: Option<windows::Win32::UI::Accessibility::HWINEVENTHOOK>,
    /// Turned off by hand while a game was running: don't turn it back on until that game closes.
    pub suppressed: bool,
    pub last_check: std::time::Instant,
    /// Open apps for the background-app picker, with memory use, largest first.
    pub apps: Vec<(String, u64)>,
}

impl GameModeState {
    /// Open apps by memory use, leaving out the games in the Game Mode list (never opened).
    pub fn background_apps(&self) -> Vec<(String, u64)> {
        let games: Vec<String> = self.config.games.iter().map(|g| g.file_name()).collect();
        gamemode::watch::app_memory(&games)
    }
}

impl Default for GameModeState {
    fn default() -> Self {
        GameModeState {
            config: gamemode::Config::load(),
            job: Job::default(),
            on: None,
            hook: None,
            suppressed: false,
            last_check: std::time::Instant::now(),
            apps: Vec::new(),
        }
    }
}

pub struct UpdateState {
    pub prefs: crate::update::Prefs,
    pub check: Job<Result<Option<crate::update::Release>, String>>,
    /// The check was started from Settings, so "no update" and errors are reported.
    pub manual: bool,
    pub available: Option<crate::update::Release>,
    /// "Remind me later" was chosen this session.
    pub hidden: bool,
    pub download: Job<Result<std::path::PathBuf, String>>,
    /// (bytes done, bytes total) of the running download.
    pub progress: std::sync::Arc<(std::sync::atomic::AtomicU64, std::sync::atomic::AtomicU64)>,
    /// Verified update waiting for a restart.
    pub staged: Option<std::path::PathBuf>,
    pub last_error: Option<String>,
}

impl Default for UpdateState {
    fn default() -> Self {
        UpdateState {
            prefs: crate::update::Prefs::load(),
            check: Job::default(),
            manual: false,
            available: None,
            hidden: false,
            download: Job::default(),
            progress: Default::default(),
            staged: None,
            last_error: None,
        }
    }
}

/// What the Optimize PC scan found.
pub struct OptScan {
    pub tweaks: Vec<Tweak>,
    pub already_active: usize,
    pub items: Vec<CleanItem>,
    pub startup_enabled: usize,
    /// Displays below their maximum refresh rate: (device, current Hz, max Hz).
    pub refresh: Vec<(String, u32, u32)>,
    /// Titles of other health checks that need attention (report only).
    pub attention: Vec<&'static str>,
    /// Memory integrity is on (offered, unticked, behind a warning).
    pub memory_integrity: bool,
    /// Startup apps worth turning off: (entry, ticked by default, why).
    pub startup_off: Vec<(StartupEntry, bool, &'static str)>,
    /// Detected games not yet in the Game Mode list: (name, exe).
    pub gm_games: Vec<(String, String)>,
    /// Preinstalled extras that can be removed (Store apps, reinstallable).
    pub extras: Vec<InstalledApp>,
}

#[derive(Default)]
pub struct OptState {
    pub stage: OptStage,
    pub scan: Job<OptScan>,
    pub apply: Job<Vec<String>>,
    pub tweaks: Vec<(Tweak, bool)>,
    pub already_active: usize,
    pub items: Vec<CleanItem>,
    pub startup_enabled: usize,
    pub restore_point: bool,
    pub refresh: Vec<(String, u32, u32)>,
    pub refresh_on: bool,
    pub attention: Vec<&'static str>,
    /// None when memory integrity is already off; Some(ticked) when it's offered.
    pub memory_integrity: Option<bool>,
    pub startup_off: Vec<(StartupEntry, bool, &'static str)>,
    pub gm_games: Vec<(String, String)>,
    /// "Game Mode for your games" is ticked.
    pub gm_on: bool,
    pub extras: Vec<InstalledApp>,
    pub extras_on: bool,
}

/// The Updates page: app updates through winget, driver updates through Windows Update.
#[derive(Default)]
pub struct UpgradeState {
    /// None until the first check.
    pub apps: Option<Vec<AppUpdate>>,
    pub apps_job: Job<Result<Vec<AppUpdate>, String>>,
    /// Ids ticked for "Update selected".
    pub apps_selected: std::collections::HashSet<String>,
    /// One `winget upgrade` after another: result lines.
    pub install: Job<Vec<String>>,
    /// Names in the running queue, and the index of the one updating.
    pub queue: Vec<String>,
    pub queue_pos: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub cancel_queue: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub drivers: Option<Vec<DriverUpdate>>,
    pub drivers_job: Job<Result<Vec<DriverUpdate>, String>>,
    pub drivers_selected: std::collections::HashSet<String>,
    /// (result lines, restart needed)
    pub driver_install: Job<Result<(Vec<String>, bool), String>>,
    pub restart_needed: bool,
    /// Graphics adapters: (name, driver version, age in months). Read when the page opens.
    pub gpus: Vec<(String, String, Option<i32>)>,
    /// Date Windows Update resumes (YYYY-MM-DD), while paused. Read when the page opens.
    pub wu_paused: Option<String>,
    /// Weeks chosen for "Pause for" (1, 2 or 5).
    pub pause_weeks: u64,
    /// The running queue installs new apps (Get apps) rather than updating.
    pub new_installs: bool,
    /// Updates hidden by "Skip this version" in the last check.
    pub skipped: usize,
}
