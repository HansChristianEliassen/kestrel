//! Scheduled tasks that start at sign-in or boot, through Task Scheduler's COM interface.
//! Windows' own tasks (under \Microsoft) are left out so they can't be switched off by accident.

use windows::core::{Interface, BSTR, VARIANT};
use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, ITaskFolder, ITaskService, TaskScheduler, TASK_ENUM_HIDDEN, TASK_TRIGGER_BOOT, TASK_TRIGGER_LOGON,
};

use crate::system::com_init;

pub struct StartupTask {
    pub name: String,
    pub path: String,
    pub command: String,
    pub enabled: bool,
    /// Runs as the signed-in user, so they can usually switch it without administrator rights.
    pub mine: bool,
}

/// shortcut: compares the task's account name with %USERNAME%; a task the user can't change still fails with a clear message.
fn runs_as_current_user(t: &IRegisteredTask) -> bool {
    let user = std::env::var("USERNAME").unwrap_or_default().to_lowercase();
    let mut id = BSTR::new();
    unsafe { t.Definition().and_then(|d| d.Principal()).and_then(|p| p.UserId(&mut id)) }.is_ok()
        && !user.is_empty()
        && id.to_string().to_lowercase().rsplit('\\').next() == Some(user.as_str())
}

fn service() -> Result<ITaskService, String> {
    com_init();
    unsafe {
        let s: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
        let none = VARIANT::default();
        s.Connect(&none, &none, &none, &none).map_err(|e| e.message())?;
        Ok(s)
    }
}

/// The task's program and arguments, if it runs at sign-in or boot.
fn startup_command(t: &IRegisteredTask) -> Option<String> {
    unsafe {
        let def = t.Definition().ok()?;
        let triggers = def.Triggers().ok()?;
        let mut count = 0;
        triggers.Count(&mut count).ok()?;
        let at_start = (1..=count).any(|i| {
            let mut kind = Default::default();
            triggers.get_Item(i).and_then(|tr| tr.Type(&mut kind)).is_ok() && (kind == TASK_TRIGGER_LOGON || kind == TASK_TRIGGER_BOOT)
        });
        if !at_start {
            return None;
        }
        let action: IExecAction = def.Actions().ok()?.get_Item(1).ok()?.cast().ok()?;
        let mut path = BSTR::new();
        action.Path(&mut path).ok()?;
        let mut args = BSTR::new();
        let _ = action.Arguments(&mut args);
        let (path, args) = (path.to_string(), args.to_string());
        Some(if args.is_empty() { format!("\"{path}\"") } else { format!("\"{path}\" {args}") })
    }
}

fn walk(folder: &ITaskFolder, out: &mut Vec<StartupTask>) {
    unsafe {
        if folder.Path().is_ok_and(|p| p.to_string().to_lowercase().starts_with(r"\microsoft")) {
            return;
        }
        if let Ok(tasks) = folder.GetTasks(TASK_ENUM_HIDDEN.0) {
            for i in 1..=tasks.Count().unwrap_or(0) {
                let Ok(t) = tasks.get_Item(&VARIANT::from(i)) else { continue };
                let Some(command) = startup_command(&t) else { continue };
                let (Ok(name), Ok(path), Ok(enabled)) = (t.Name(), t.Path(), t.Enabled()) else { continue };
                let mine = runs_as_current_user(&t);
                out.push(StartupTask { name: name.to_string(), path: path.to_string(), command, enabled: enabled.as_bool(), mine });
            }
        }
        if let Ok(subs) = folder.GetFolders(0) {
            for i in 1..=subs.Count().unwrap_or(0) {
                if let Ok(f) = subs.get_Item(&VARIANT::from(i)) {
                    walk(&f, out);
                }
            }
        }
    }
}

pub fn list() -> Vec<StartupTask> {
    let mut out = Vec::new();
    if let Ok(root) = service().and_then(|s| unsafe { s.GetFolder(&BSTR::from("\\")) }.map_err(|e| e.message())) {
        walk(&root, &mut out);
    }
    out
}

/// Enabled scheduled tasks in every folder, Windows' own included (for System analysis).
pub fn enabled_task_count() -> usize {
    fn count(folder: &ITaskFolder) -> usize {
        unsafe {
            let tasks = folder.GetTasks(TASK_ENUM_HIDDEN.0).map_or(0, |ts| {
                (1..=ts.Count().unwrap_or(0))
                    .filter(|i| ts.get_Item(&VARIANT::from(*i)).and_then(|t| t.Enabled()).is_ok_and(|e| e.as_bool()))
                    .count()
            });
            let subs = folder
                .GetFolders(0)
                .map_or(0, |fs| (1..=fs.Count().unwrap_or(0)).filter_map(|i| fs.get_Item(&VARIANT::from(i)).ok()).map(|f| count(&f)).sum());
            tasks + subs
        }
    }
    service().and_then(|s| unsafe { s.GetFolder(&BSTR::from("\\")) }.map_err(|e| e.message())).map_or(0, |root| count(&root))
}

/// (path, enabled) of every scheduled task whose path matches `pred` (lower-case), Windows' folders included.
pub fn find_tasks(pred: impl Fn(&str) -> bool) -> Vec<(String, bool)> {
    fn walk(folder: &ITaskFolder, pred: &dyn Fn(&str) -> bool, out: &mut Vec<(String, bool)>) {
        unsafe {
            if let Ok(ts) = folder.GetTasks(TASK_ENUM_HIDDEN.0) {
                for i in 1..=ts.Count().unwrap_or(0) {
                    let Ok(t) = ts.get_Item(&VARIANT::from(i)) else { continue };
                    if let (Ok(path), Ok(on)) = (t.Path(), t.Enabled()) {
                        let path = path.to_string();
                        if pred(&path.to_lowercase()) {
                            out.push((path, on.as_bool()));
                        }
                    }
                }
            }
            if let Ok(fs) = folder.GetFolders(0) {
                for i in 1..=fs.Count().unwrap_or(0) {
                    if let Ok(f) = fs.get_Item(&VARIANT::from(i)) {
                        walk(&f, pred, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(root) = service().and_then(|s| unsafe { s.GetFolder(&BSTR::from("\\")) }.map_err(|e| e.message())) {
        walk(&root, &pred, &mut out);
    }
    out
}

/// Puts a task back to a saved state. Nothing to do when it's already there or the task no longer exists, so a
/// saved original that was never actually changed doesn't fail every restore.
pub fn put_back(path: &str, on: bool) -> Result<(), String> {
    match is_enabled(path) {
        Ok(now) if now != on => set_enabled(path, on),
        _ => Ok(()),
    }
}

pub fn is_enabled(path: &str) -> Result<bool, String> {
    unsafe {
        let task = service()?.GetFolder(&BSTR::from("\\")).and_then(|f| f.GetTask(&BSTR::from(path))).map_err(|e| e.message())?;
        Ok(task.Enabled().map_err(|e| e.message())?.as_bool())
    }
}

pub fn set_enabled(path: &str, enabled: bool) -> Result<(), String> {
    unsafe {
        let task = service()?.GetFolder(&BSTR::from("\\")).and_then(|f| f.GetTask(&BSTR::from(path))).map_err(|e| e.message())?;
        task.SetEnabled(if enabled { VARIANT_TRUE } else { VARIANT_FALSE }).map_err(|e| {
            if e.code().0 as u32 == 0x8007_0005 && crate::system::is_admin() {
                "Windows protects this task: only Windows itself can change it, even as administrator.".into()
            } else if e.code().0 as u32 == 0x8007_0005 {
                "This task belongs to all users; changing it needs administrator rights (Settings > Restart as administrator).".into()
            } else {
                e.message()
            }
        })
    }
}
