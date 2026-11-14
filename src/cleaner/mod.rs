pub mod cleanup;
pub mod large;
pub mod scanner;

pub use scanner::{Category, Kind, Scanned};

use crate::system::{is_admin, running_processes};
use crate::utils::log;

pub struct CleanItem {
    pub cat: Category,
    pub found: Scanned,
    pub selected: bool,
    /// The file list is expanded on the Cleanup page.
    pub show_files: bool,
}

/// Scans every category, one short-lived thread per category. Nothing is deleted.
pub fn scan_all() -> Vec<CleanItem> {
    let running = &running_processes();
    let admin = is_admin();
    std::thread::scope(|s| {
        let scans: Vec<_> = scanner::categories()
            .into_iter()
            .map(|cat| {
                s.spawn(move || {
                    let found = scanner::scan(&cat, running, admin);
                    let selected = cat.default_on && found.count > 0;
                    CleanItem { cat, found, selected, show_files: false }
                })
            })
            .collect();
        scans.into_iter().map(|h| h.join().expect("scan thread panicked")).collect()
    })
}

/// Cleans the given (user-approved) items and returns one summary line per category.
pub fn clean_items(items: &[CleanItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| {
            let line = cleanup::clean(&item.cat, &item.found).summary();
            log(&format!("Cleaner: {line}"));
            line
        })
        .collect()
}
