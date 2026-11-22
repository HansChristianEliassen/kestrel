pub mod autostart;
pub mod manager;
mod tasks;

pub use manager::{exe_name, list, restore_all, set_enabled, StartupEntry};

/// Common apps that start with Windows and keep running in the background, but don't need to for gaming.
/// (pattern matched in the lower-case name or command, ticked by default, why). Turning one off is the same
/// reversible switch as on the Startup apps page; nothing is uninstalled. Device software (mouse, headset,
/// RGB) and drivers are deliberately not listed, so their settings keep working.
const RECOMMEND: &[(&str, bool, &str)] = &[
    ("spotify", true, "Starts hidden and keeps running. It opens fine when you start it yourself."),
    ("discord", true, "Starts with Windows and stays in the background. Start it when you want to chat."),
    ("msteams", true, "Microsoft Teams starts with Windows and uses memory in the background."),
    ("microsoftedgeautolaunch", true, "Preloads Microsoft Edge at sign-in so it opens faster, using memory all the time."),
    ("skype", true, "Starts with Windows and stays in the background."),
    ("zoom", true, "Starts with Windows and stays in the background."),
    ("adobe", true, "Adobe helpers and updaters run in the background; the apps still work and update when opened."),
    ("acrobat", true, "Adobe helpers and updaters run in the background; the apps still work and update when opened."),
    ("googleupdate", false, "Google's updater. Chrome also updates itself while it's open."),
    ("onedrive", false, "Keeps your files in sync with OneDrive. Leave it on if you rely on that."),
    ("steam", false, "Game launchers start with Windows for quick downloads and chat. Leave it on if you like that."),
    ("epicgameslauncher", false, "Game launchers start with Windows for quick downloads and chat. Leave it on if you like that."),
    ("eadesktop", false, "Game launchers start with Windows for quick downloads and chat. Leave it on if you like that."),
    ("ubisoft", false, "Game launchers start with Windows for quick downloads and chat. Leave it on if you like that."),
    ("battle.net", false, "Game launchers start with Windows for quick downloads and chat. Leave it on if you like that."),
];

/// Whether an enabled startup entry is one Kestrel suggests turning off: (ticked by default, why).
pub fn recommendation(name: &str, command: &str) -> Option<(bool, &'static str)> {
    let text = format!("{} {}", name.to_lowercase(), command.to_lowercase());
    RECOMMEND.iter().find(|(pattern, ..)| text.contains(pattern)).map(|(_, on, why)| (*on, *why))
}

/// The enabled, unprotected startup entries worth turning off: (index into `entries`, ticked by default, why).
pub fn recommended(entries: &[StartupEntry]) -> Vec<(usize, bool, &'static str)> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.enabled && !e.protected)
        .filter_map(|(i, e)| recommendation(&e.name, &e.command).map(|(on, why)| (i, on, why)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::recommendation;

    #[test]
    fn recommends_known_background_apps_only() {
        assert_eq!(recommendation("Spotify", r#""C:\Users\x\AppData\Roaming\Spotify\Spotify.exe" --autostart"#).map(|r| r.0), Some(true));
        assert_eq!(recommendation("MicrosoftEdgeAutoLaunch_0123ABCD", "msedge.exe --no-startup-window").map(|r| r.0), Some(true));
        assert_eq!(recommendation("Steam", r#""C:\Program Files (x86)\Steam\steam.exe" -silent"#).map(|r| r.0), Some(false));
        assert_eq!(recommendation("OneDrive", "OneDrive.exe /background").map(|r| r.0), Some(false));
        // Device software and unknown apps are never suggested.
        for (name, cmd) in [
            ("Logitech G HUB", "lghub.exe"),
            ("Razer Synapse", "razer.exe"),
            ("SecurityHealth", "SecurityHealthSystray.exe"),
            ("MyTool", "tool.exe"),
        ] {
            assert!(recommendation(name, cmd).is_none(), "{name}");
        }
    }
}
