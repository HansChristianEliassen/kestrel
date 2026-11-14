//! Minimal HTTPS GET through WinHTTP (Windows' own HTTP stack: system proxy, TLS and certificate checks).

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_REDIRECTS,
    WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_LOCATION,
    WINHTTP_QUERY_STATUS_CODE,
};

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn handle(h: *mut c_void, what: &str) -> Result<Handle, String> {
    if h.is_null() {
        Err(format!("{what}: {}", windows::core::Error::from_win32().message()))
    } else {
        Ok(Handle(h))
    }
}

/// An open request with its response headers received. Keeps the session and connection alive.
struct Response {
    request: Handle,
    _connect: Handle,
    _session: Handle,
    status: u32,
}

/// Sends a GET to an https:// URL. With `follow` off, redirects come back as 3xx with a Location header.
fn get(url: &str, follow: bool) -> Result<Response, String> {
    let rest = url.strip_prefix("https://").ok_or("Only https:// addresses are allowed.")?;
    let (host, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    let ua = HSTRING::from(format!("Kestrel/{} (update check)", env!("CARGO_PKG_VERSION")));
    unsafe {
        let session = handle(WinHttpOpen(&ua, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "Network")?;
        // Resolve, connect, send: 15 s each; receive: 30 s between packets, so a stalled download fails instead of hanging.
        let _ = WinHttpSetTimeouts(session.0, 15_000, 15_000, 15_000, 30_000);
        let connect = handle(WinHttpConnect(session.0, &HSTRING::from(host), 443, 0), "Connect")?;
        let request = handle(
            WinHttpOpenRequest(
                connect.0,
                w!("GET"),
                &HSTRING::from(path),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "Request",
        )?;
        if !follow {
            WinHttpSetOption(Some(request.0), WINHTTP_OPTION_DISABLE_FEATURE, Some(&WINHTTP_DISABLE_REDIRECTS.to_le_bytes()))
                .map_err(|e| e.message())?;
        }
        // Following redirects is limited to https: WinHTTP's default policy refuses https-to-http.
        WinHttpSendRequest(request.0, None, None, 0, 0, 0)
            .and_then(|()| WinHttpReceiveResponse(request.0, std::ptr::null_mut()))
            .map_err(|e| format!("Couldn't reach {host}. Check your internet connection. ({})", e.message().trim()))?;
        let status = number(&request, WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
        Ok(Response { request, _connect: connect, _session: session, status })
    }
}

fn number(r: &Handle, query: u32) -> Option<u32> {
    let mut value = 0u32;
    let mut size = 4u32;
    unsafe {
        WinHttpQueryHeaders(
            r.0,
            query | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut value as *mut u32 as *mut c_void),
            &mut size,
            std::ptr::null_mut(),
        )
    }
    .ok()
    .map(|()| value)
}

/// Status code and Location header of a URL, without following the redirect or reading the body.
pub fn redirect(url: &str) -> Result<(u32, Option<String>), String> {
    let r = get(url, false)?;
    let mut buf = [0u16; 2048];
    let mut size = std::mem::size_of_val(&buf) as u32;
    let location = unsafe {
        WinHttpQueryHeaders(
            r.request.0,
            WINHTTP_QUERY_LOCATION,
            PCWSTR::null(),
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut size,
            std::ptr::null_mut(),
        )
    }
    .ok()
    .map(|()| String::from_utf16_lossy(&buf[..size as usize / 2]));
    Ok((r.status, location))
}

/// Downloads a file (following https redirects), refusing anything larger than `max` bytes.
/// `done`/`total` report progress while it runs.
pub fn download(url: &str, max: u64, done: &AtomicU64, total: &AtomicU64) -> Result<Vec<u8>, String> {
    let r = get(url, true)?;
    match r.status {
        200 => {}
        404 => return Err("The release file wasn't found on GitHub.".into()),
        403 | 429 => return Err("GitHub is limiting downloads right now. Try again later.".into()),
        s => return Err(format!("GitHub answered with status {s}.")),
    }
    let expected = number(&r.request, WINHTTP_QUERY_CONTENT_LENGTH).map(u64::from);
    if expected.is_some_and(|n| n > max) {
        return Err("The release file is larger than expected; it wasn't downloaded.".into());
    }
    total.store(expected.unwrap_or(0), Ordering::Relaxed);
    done.store(0, Ordering::Relaxed);
    let mut body = Vec::with_capacity(expected.unwrap_or(0) as usize);
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let mut read = 0u32;
        unsafe { WinHttpReadData(r.request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read) }
            .map_err(|e| format!("The download was interrupted. ({})", e.message().trim()))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read as usize]);
        if body.len() as u64 > max {
            return Err("The release file is larger than expected; the download was stopped.".into());
        }
        done.store(body.len() as u64, Ordering::Relaxed);
    }
    if expected.is_some_and(|n| n != body.len() as u64) {
        return Err("The download was interrupted before it finished.".into());
    }
    Ok(body)
}
