//! Driver updates from Windows Update, through the documented Windows Update Agent API. Windows Update offers
//! only drivers Microsoft has certified and matched to this PC's hardware IDs, so Kestrel never picks drivers itself.

use windows::core::{Interface, BSTR};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::UpdateAgent::*;

use crate::system::com_init;

#[derive(Clone, Debug)]
pub struct DriverUpdate {
    /// Windows Update's UpdateID (a GUID).
    pub id: String,
    pub title: String,
    pub class: String,
    pub maker: String,
    pub date: String,
    pub size: u64,
}

const APP_ID: &str = "Kestrel";
/// Only driver updates that aren't installed or hidden by the user.
const SEARCH: &str = "IsInstalled=0 and Type='Driver' and IsHidden=0";

fn session() -> windows::core::Result<IUpdateSession> {
    com_init();
    let s: IUpdateSession = unsafe { CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER)? };
    unsafe { s.SetClientApplicationID(&BSTR::from(APP_ID))? };
    Ok(s)
}

/// Searches Microsoft's Windows Update servers (not a company WSUS server, which often carries no drivers).
fn search(s: &IUpdateSession, criteria: &str) -> windows::core::Result<IUpdateCollection> {
    unsafe {
        let searcher = s.CreateUpdateSearcher()?;
        searcher.SetClientApplicationID(&BSTR::from(APP_ID))?;
        searcher.SetServerSelection(ssWindowsUpdate)?;
        searcher.Search(&BSTR::from(criteria))?.Updates()
    }
}

fn err(e: windows::core::Error) -> String {
    let code = e.code().0 as u32;
    match code {
        0x8024402C | 0x80072EE7 | 0x80072EFD | 0x80072EE2 => {
            "Couldn't reach Windows Update. Check the internet connection and try again.".into()
        }
        0x8024001E => "Windows Update was busy or shutting down. Try again in a minute.".into(),
        0x80240044 => "Installing drivers needs administrator rights.".into(),
        0x80070422 => "The Windows Update service is turned off.".into(),
        _ => format!("Windows Update error {code:#010X}: {}", e.message()),
    }
}

/// Driver updates Windows Update offers for this PC. Read-only; takes up to a minute.
pub fn available() -> Result<Vec<DriverUpdate>, String> {
    let list = session().and_then(|s| search(&s, SEARCH)).map_err(err)?;
    let mut out = Vec::new();
    unsafe {
        for i in 0..list.Count().map_err(err)? {
            let Ok(u) = list.get_Item(i) else { continue };
            let Ok(id) = u.Identity().and_then(|x| x.UpdateID()) else { continue };
            let d = u.cast::<IWindowsDriverUpdate>().ok();
            let s = |r: windows::core::Result<BSTR>| r.map(|b| b.to_string()).unwrap_or_default();
            let size = u.MaxDownloadSize().map(|d| d.Anonymous2.Lo64).unwrap_or(0);
            out.push(DriverUpdate {
                id: id.to_string(),
                title: s(u.Title()),
                class: d.as_ref().map(|d| s(d.DriverClass())).unwrap_or_default(),
                maker: d.as_ref().map(|d| s(d.DriverManufacturer())).unwrap_or_default(),
                date: d.as_ref().and_then(|d| d.DriverVerDate().ok()).map(ole_date).unwrap_or_default(),
                size,
            });
        }
    }
    Ok(out)
}

/// Downloads and installs the chosen drivers. Returns one line per driver and whether Windows needs a restart.
/// Never restarts by itself.
pub fn install(ids: &[String]) -> Result<(Vec<String>, bool), String> {
    if !crate::system::is_admin() {
        return Err("Installing drivers needs administrator rights (Restart as administrator).".into());
    }
    let s = session().map_err(err)?;
    // Searched again by UpdateID so only what the user saw and picked is installed.
    let found = search(&s, SEARCH).map_err(err)?;
    unsafe {
        let picked: IUpdateCollection = CoCreateInstance(&UpdateCollection, None, CLSCTX_INPROC_SERVER).map_err(err)?;
        let mut titles = Vec::new();
        for i in 0..found.Count().map_err(err)? {
            let Ok(u) = found.get_Item(i) else { continue };
            let id = u.Identity().and_then(|x| x.UpdateID()).map(|b| b.to_string()).unwrap_or_default();
            if ids.contains(&id) {
                if !u.EulaAccepted().is_ok_and(|b| b.as_bool()) {
                    u.AcceptEula().map_err(err)?;
                }
                picked.Add(&u).map_err(err)?;
                titles.push(u.Title().map(|b| b.to_string()).unwrap_or(id));
            }
        }
        if titles.is_empty() {
            return Ok((vec!["Windows Update no longer offers these drivers (already installed or replaced).".into()], false));
        }
        let downloader = s.CreateUpdateDownloader().map_err(err)?;
        downloader.SetClientApplicationID(&BSTR::from(APP_ID)).map_err(err)?;
        downloader.SetUpdates(&picked).map_err(err)?;
        downloader.Download().map_err(err)?;
        let installer = s.CreateUpdateInstaller().map_err(err)?;
        installer.SetClientApplicationID(&BSTR::from(APP_ID)).map_err(err)?;
        installer.SetUpdates(&picked).map_err(err)?;
        let result = installer.Install().map_err(err)?;
        let reboot = result.RebootRequired().is_ok_and(|b| b.as_bool());
        let lines = titles
            .iter()
            .enumerate()
            .map(|(i, t)| match result.GetUpdateResult(i as i32).and_then(|r| r.ResultCode()) {
                Ok(c) if c == orcSucceeded || c == orcSucceededWithErrors => format!("✔ Installed: {t}"),
                Ok(c) if c == orcAborted => format!("✖ {t}: cancelled"),
                Ok(_) => {
                    let hr = result.GetUpdateResult(i as i32).and_then(|r| r.HResult()).unwrap_or(0);
                    format!("✖ {t}: failed ({:#010X})", hr as u32)
                }
                Err(e) => format!("✖ {t}: {}", err(e)),
            })
            .collect();
        Ok((lines, reboot))
    }
}

/// OLE automation date (days since 30 Dec 1899) as YYYY-MM-DD.
fn ole_date(d: f64) -> String {
    // Days since 1970-01-01, then the civil-from-days algorithm (H. Hinnant).
    let z = d.floor() as i64 - 25569 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ole_dates() {
        assert_eq!(ole_date(25569.0), "1970-01-01");
        assert_eq!(ole_date(45658.0), "2025-01-01");
        assert_eq!(ole_date(45716.5), "2025-02-28");
        assert_eq!(ole_date(45351.0), "2024-02-29");
    }

    #[test]
    #[ignore = "asks Windows Update on this PC (read-only, needs internet)"]
    fn live_list() {
        let list = available().unwrap();
        eprintln!("{list:#?}");
    }
}
