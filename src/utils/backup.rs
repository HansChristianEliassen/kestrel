//! Original values of everything Kestrel changes, in %LOCALAPPDATA%\Kestrel\backup.txt.
//! One `key<TAB>value` per line; keys are namespaced ("game_mode", "startup:…", "fso:…", "display:…").

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use super::data_dir;

fn path() -> PathBuf {
    data_dir().join("backup.txt")
}

fn parse(text: &str) -> BTreeMap<String, String> {
    text.lines()
        // Older versions wrote "key=value"; keys never contain '=' or tabs.
        .filter_map(|l| l.split_once('\t').or_else(|| l.split_once('=')))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn load() -> BTreeMap<String, String> {
    parse(&fs::read_to_string(path()).unwrap_or_default())
}

/// Serializes every read-modify-write, since the UI thread and background jobs both change backups.
static LOCK: Mutex<()> = Mutex::new(());

/// Writes a temporary file and renames it over the old one, so a crash mid-write never loses saved originals.
fn save(b: &BTreeMap<String, String>) -> Result<(), String> {
    let text: String = b.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
    let tmp = path().with_extension("tmp");
    fs::write(&tmp, text)
        .and_then(|()| fs::rename(&tmp, path()))
        .map_err(|e| format!("Could not save the backup of the original setting: {e}"))
}

/// Everything saved, for showing many statuses without re-reading the file.
pub fn all() -> BTreeMap<String, String> {
    load()
}

pub fn get(key: &str) -> Option<String> {
    load().remove(key)
}

pub fn has(key: &str) -> bool {
    load().contains_key(key)
}

/// Saves the original value the first time only, so repeated changes never overwrite it.
pub fn save_original(key: &str, original: impl FnOnce() -> Result<String, String>) -> Result<(), String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut b = load();
    if !b.contains_key(key) {
        b.insert(key.to_string(), original()?);
        save(&b)?;
    }
    Ok(())
}

pub fn remove(key: &str) -> Result<(), String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut b = load();
    if b.remove(key).is_some() {
        save(&b)?;
    }
    Ok(())
}

/// (key without prefix, value) for every saved original under `prefix`.
pub fn with_prefix(prefix: &str) -> Vec<(String, String)> {
    load().into_iter().filter_map(|(k, v)| k.strip_prefix(prefix).map(|k| (k.to_string(), v))).collect()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn reads_both_formats() {
        let b = parse("game_mode=0\nfso:C:\\a=b.exe\t~ X\nwindowed_games\tA=1;B=2;\n");
        assert_eq!(b["game_mode"], "0");
        assert_eq!(b["fso:C:\\a=b.exe"], "~ X");
        assert_eq!(b["windowed_games"], "A=1;B=2;");
    }
}
