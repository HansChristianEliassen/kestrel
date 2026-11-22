//! App updates through winget, Windows' own package manager. Kestrel only reads its list and runs
//! `winget upgrade` for the apps the user picked; winget downloads and verifies the installers.

use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq)]
pub struct AppUpdate {
    pub name: String,
    pub id: String,
    pub current: String,
    pub available: String,
}

/// winget package ids are `Publisher.Name` style; anything else is refused before it reaches the command line.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.starts_with('-') && id.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
}

fn winget(args: &[&str]) -> Result<(i32, String), String> {
    let out = Command::new("winget").args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "winget (App Installer) isn't installed. Get App Installer from the Microsoft Store, then check again.".to_string()
        } else {
            format!("Couldn't start winget: {e}")
        }
    })?;
    Ok((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// Apps from the winget community repository with a newer version. Read-only.
pub fn available() -> Result<Vec<AppUpdate>, String> {
    let (code, text) =
        winget(&["upgrade", "--source", "winget", "--disable-interactivity", "--accept-source-agreements", "--output-locale", "en-US"])?;
    let list = parse(&text);
    // "No installed package found matching input criteria." also exits non-zero.
    if code != 0 && list.is_empty() && !text.contains("No installed package") {
        return Err(last_line(&text).unwrap_or_else(|| format!("winget stopped with code {code:#x}.")));
    }
    Ok(list)
}

/// Popular gaming and everyday apps for the "Get apps" list: (name, winget id). Ids checked against the catalog.
pub const POPULAR: [(&str, &str); 13] = [
    ("Steam", "Valve.Steam"),
    ("Epic Games Launcher", "EpicGames.EpicGamesLauncher"),
    ("EA app", "ElectronicArts.EADesktop"),
    ("Ubisoft Connect", "Ubisoft.Connect"),
    ("Battle.net", "Blizzard.BattleNet"),
    ("GOG GALAXY", "GOG.Galaxy"),
    ("Discord", "Discord.Discord"),
    ("OBS Studio", "OBSProject.OBSStudio"),
    ("Spotify", "Spotify.Spotify"),
    ("Mozilla Firefox", "Mozilla.Firefox"),
    ("Google Chrome", "Google.Chrome"),
    ("7-Zip", "7zip.7zip"),
    ("VLC media player", "VideoLAN.VLC"),
];

/// Searches the winget catalog by name, id or tag. Read-only; `available` holds the version that would install.
pub fn search(query: &str) -> Result<Vec<AppUpdate>, String> {
    let query = query.trim();
    // Passed as one argument (no shell); a leading '-' would be read as a winget option.
    if query.is_empty() || query.len() > 64 || query.starts_with('-') {
        return Err("Type an app name to search for.".into());
    }
    let (code, text) = winget(&[
        "search",
        query,
        "--source",
        "winget",
        "--disable-interactivity",
        "--accept-source-agreements",
        "--output-locale",
        "en-US",
    ])?;
    let list = parse(&text);
    if code != 0 && list.is_empty() && !text.contains("No package found") {
        return Err(last_line(&text).unwrap_or_else(|| format!("winget stopped with code {code:#x}.")));
    }
    Ok(list)
}

/// Versions the user chose to skip, one "id version" per line. A newer version shows again.
fn skipped_file() -> std::path::PathBuf {
    crate::utils::data_dir().join("app-updates-skipped.txt")
}

/// Hides this exact version of the app from the update list.
pub fn skip(app: &AppUpdate) -> Result<(), String> {
    let mut text = std::fs::read_to_string(skipped_file()).unwrap_or_default();
    text.push_str(&format!("{} {}\n", app.id, app.available));
    crate::utils::write_atomic(&skipped_file(), text.as_bytes()).map_err(|e| format!("Couldn't save the skipped version: {e}"))
}

/// Shows every skipped version again.
pub fn unskip_all() {
    let _ = std::fs::remove_file(skipped_file());
}

/// The list without skipped versions, and how many were hidden.
pub fn without_skipped(list: Vec<AppUpdate>) -> (Vec<AppUpdate>, usize) {
    filter_skipped(list, &std::fs::read_to_string(skipped_file()).unwrap_or_default())
}

fn filter_skipped(list: Vec<AppUpdate>, skipped: &str) -> (Vec<AppUpdate>, usize) {
    let total = list.len();
    let kept: Vec<AppUpdate> =
        list.into_iter().filter(|a| !skipped.lines().any(|l| l.split_once(' ') == Some((a.id.as_str(), a.available.as_str())))).collect();
    let hidden = total - kept.len();
    (kept, hidden)
}

/// Updates one app silently. winget asks Windows for administrator rights itself when the installer needs them.
pub fn install(app: &AppUpdate) -> Result<String, String> {
    run(app, "upgrade", format!("updated to {}", app.available))
}

/// Installs one app from the winget catalog silently, the same way as `install` updates.
pub fn install_new(app: &AppUpdate) -> Result<String, String> {
    run(app, "install", if app.available.is_empty() { "installed".into() } else { format!("installed {}", app.available) })
}

fn run(app: &AppUpdate, verb: &str, done: String) -> Result<String, String> {
    if !valid_id(&app.id) {
        return Err(format!("{}: unexpected package id, skipped.", app.name));
    }
    let mut args = vec![
        verb,
        "--id",
        &app.id,
        "--exact",
        "--source",
        "winget",
        "--silent",
        "--disable-interactivity",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--output-locale",
        "en-US",
    ];
    // winget refuses per-user installs from an administrator session, so install for all users there.
    if verb == "install" && crate::system::is_admin() {
        args.extend(["--scope", "machine"]);
    }
    let (code, text) = winget(&args)?;
    if code == 0 {
        Ok(format!("✔ {}: {done}", app.name))
    } else {
        Err(format!("{}: {}", app.name, explain(&text).unwrap_or_else(|| format!("winget stopped with code {code:#x}."))))
    }
}

/// winget's error, with what to do for the cases it can't handle itself.
fn explain(text: &str) -> Option<String> {
    if text.contains("cannot be run from an administrator context") {
        return Some(
            "its installer refuses to run as administrator. Open Kestrel normally (not as administrator) and update it again.".into(),
        );
    }
    if text.contains("within the user scope") || text.contains("No applicable installer found") {
        return Some(
            "it can only be installed for your account, which isn't possible while Kestrel runs as administrator. Open Kestrel normally and install it again.".into(),
        );
    }
    if text.contains("existing package already installed") {
        return Some("it's already installed. If there's a newer version, it shows under Updates.".into());
    }
    if text.contains("install technology is different") {
        return Some("the new version installs a different way than the one you have, so winget can't update it in place. Update it from the app itself, or uninstall it and install the new version from the publisher's site.".into());
    }
    last_line(text)
}

/// The last meaningful line of winget's output (its error or result message), without progress bars.
fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(|l| l.rsplit('\r').next().unwrap_or("").trim())
        .rfind(|l| l.chars().any(|c| c.is_alphanumeric()) && !l.contains('█') && !l.contains('▒'))
        .map(String::from)
}

/// Parses the table `winget upgrade` and `winget search` print. winget has no machine-readable output, so columns
/// are found from the header (English via `--output-locale en-US`) and cut by character position. Rows end at the
/// summary line. Upgrade tables have Version and Available; search tables have only Version, which goes to `available`.
pub fn parse(text: &str) -> Vec<AppUpdate> {
    // Progress spinners are redrawn with '\r', so only the text after the last one is the real line.
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r').rsplit('\r').next().unwrap_or("")).collect();
    let Some(h) = lines.iter().position(|l| l.starts_with("Name") && l.contains(" Id ") && l.contains("Version")) else {
        return vec![];
    };
    let header = lines[h];
    let col = |name: &str| header.find(name).map(|b| header[..b].chars().count());
    let (Some(id), Some(ver)) = (col(" Id ").map(|c| c + 1), col("Version")) else {
        return vec![];
    };
    let avail = col("Available");
    let end = col("Match").or(col("Source")).unwrap_or(usize::MAX);
    let mut out = Vec::new();
    for line in lines.iter().skip(h + 1).skip_while(|l| l.starts_with('-')) {
        let chars: Vec<char> = line.chars().collect();
        if line.trim().is_empty() || chars.len() <= ver || line.contains("upgrades available") || line.contains("upgrade available") {
            break;
        }
        let cut = |a: usize, b: usize| chars[a.min(chars.len())..b.min(chars.len())].iter().collect::<String>().trim().to_string();
        let app = match avail {
            Some(avail) => AppUpdate { name: cut(0, id), id: cut(id, ver), current: cut(ver, avail), available: cut(avail, end) },
            None => AppUpdate { name: cut(0, id), id: cut(id, ver), current: String::new(), available: cut(ver, end) },
        };
        if valid_id(&app.id) && !app.available.is_empty() {
            out.push(app);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from `winget upgrade --source winget --output-locale en-US` on a Windows 11 PC.
    const SAMPLE: &str = "\r   - \r   \\ \rName                                                         Id                                 Version                 Available\r\n-------------------------------------------------------------------------------------------------------------------------------------------------\r\nEA app                                                       ElectronicArts.EADesktop           13.796.0.6309           13.827.2.6342\r\nMicrosoft Visual C++ v14 Redistributable (x64) - 14.51.36247 Microsoft.VCRedist.2015+.x64       14.51.36247.0           14.52.37029.0\r\nSpotify                                                      Spotify.Spotify                    1.3.8.620.g39b276d5d7ac 1.3.10.2586.g81a118a22460\r\n10 upgrades available.\r\n1 package(s) have version numbers that cannot be determined. Use --include-unknown to see all results.\r\n";

    #[test]
    fn parses_winget_table() {
        let list = parse(SAMPLE);
        assert_eq!(list.len(), 3);
        assert_eq!(
            list[0],
            AppUpdate {
                name: "EA app".into(),
                id: "ElectronicArts.EADesktop".into(),
                current: "13.796.0.6309".into(),
                available: "13.827.2.6342".into()
            }
        );
        assert_eq!(list[1].id, "Microsoft.VCRedist.2015+.x64");
        assert_eq!(list[1].name, "Microsoft Visual C++ v14 Redistributable (x64) - 14.51.36247");
        assert_eq!(list[2].available, "1.3.10.2586.g81a118a22460");
    }

    #[test]
    fn parses_source_column_unknown_versions_and_wide_names() {
        let text = "Name          Id            Version Available Source\n-----------------------------------------------------\nCafé Tool…    Some.Tool     Unknown 2.0       winget\nOther         Other.App     < 1.5   1.6       winget\n2 upgrades available.\n";
        let list = parse(text);
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].name.as_str(), list[0].id.as_str(), list[0].current.as_str()), ("Café Tool…", "Some.Tool", "Unknown"));
        assert_eq!(list[0].available, "2.0");
        assert_eq!(list[1].current, "< 1.5");
    }

    #[test]
    fn parses_search_table() {
        // Captured from `winget search steam --source winget --output-locale en-US`.
        let text = "Name                            Id                                    Version              Match\r\n----------------------------------------------------------------------------------------------------------------\r\nSteam                           Valve.Steam                           2.10.91.91           \r\nAchievement Watcher             AnthonyBeaumont.AchievementWatcher    1.6.8                Tag: steam\r\n";
        let list = parse(text);
        assert_eq!(list.len(), 2);
        assert_eq!(
            list[0],
            AppUpdate { name: "Steam".into(), id: "Valve.Steam".into(), current: String::new(), available: "2.10.91.91".into() }
        );
        assert_eq!(list[1].available, "1.6.8");
        assert!(POPULAR.iter().all(|(_, id)| valid_id(id)));
        assert!(search("-h").is_err() && search("  ").is_err());
    }

    #[test]
    fn skipped_versions_only() {
        let app = |id: &str, v: &str| AppUpdate { name: id.into(), id: id.into(), current: "1".into(), available: v.into() };
        let (kept, hidden) = filter_skipped(vec![app("A.a", "2.0"), app("B.b", "3.0"), app("C.c", "1.5")], "A.a 2.0\nB.b 2.9\n");
        assert_eq!(hidden, 1);
        assert_eq!(kept.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["B.b", "C.c"]);
        // B.b has a newer version than the skipped one
    }

    #[test]
    fn nothing_to_update() {
        assert!(parse("No installed package found matching input criteria.\r\n").is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn ids() {
        assert!(valid_id("Microsoft.VCRedist.2015+.x64") && valid_id("OpenJS.NodeJS.LTS"));
        for bad in ["", "-h", "a b", "x;y", "a\"b", "a&b", "x|y"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn error_line_skips_progress() {
        let out = "  ██████████▒▒▒▒  1.2 MB / 3 MB\r\nInstaller failed with exit code: 1603\r\n";
        assert_eq!(last_line(out).as_deref(), Some("Installer failed with exit code: 1603"));
        assert_eq!(explain(out).as_deref(), Some("Installer failed with exit code: 1603"));
        let scope = "The requested operation is not permitted from an administrator context on packages installed within the user scope.";
        assert!(explain(scope).unwrap().contains("Open Kestrel normally"));
        assert!(explain("Found Spotify\r\nThe installer cannot be run from an administrator context.\r\n")
            .unwrap()
            .contains("not as administrator"));
        assert!(explain("A newer version was found, but the install technology is different from the current version installed.")
            .unwrap()
            .contains("in place"));
    }

    #[test]
    #[ignore = "runs winget on this PC"]
    fn live_list() {
        let list = available().unwrap();
        eprintln!("{list:#?}");
    }
}
