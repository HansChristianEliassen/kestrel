use std::sync::mpsc::{channel, Receiver};

use eframe::egui;

use crate::cleaner::CleanItem;
use crate::gamemode;
use crate::optimizer::games::Game;
use crate::optimizer::health::Check;
use crate::optimizer::{Preset, Tweak};
use crate::startup::StartupEntry;
use crate::uninstaller::InstalledApp;

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Dashboard,
    Cleaner,
    Apps,
    Startup,
    Gaming,
    GameMode,
    Settings,
}

impl Page {
    pub const ALL: [Page; 7] = [Page::Dashboard, Page::Cleaner, Page::Apps, Page::Startup, Page::Gaming, Page::GameMode, Page::Settings];

    pub fn label(self) -> &'static str {
        match self {
            Page::Dashboard => "Overview",
            Page::Cleaner => "Cleanup",
            Page::Apps => "Apps",
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
pub type TweakStatus = Vec<(bool, bool)>;

/// (display device, old Hz, new Hz)
pub type RefreshChange = (String, u32, u32);

#[derive(Clone, Copy, PartialEq)]
pub enum GamingTab {
    Settings,
    Games,
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
        }
    }
}

pub fn tweak_status() -> TweakStatus {
    Tweak::ALL.iter().map(|t| (t.is_applied(), t.has_backup())).collect()
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
    pub apps: Vec<String>,
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
}
