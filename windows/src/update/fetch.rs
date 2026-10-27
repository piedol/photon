//! HTTP GET through WinHTTP, Windows' own client: its TLS, certificate checks and proxy
//! settings, so Photon carries no HTTP or TLS code of its own. Calls block, so they run on a
//! background thread. Redirects (GitHub's downloads redirect to its asset host) are followed
//! by WinHTTP, which never follows one from https to http.

use std::ffi::c_void;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use windows::Win32::Foundation::{GetLastError, HMODULE};
use windows::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_CONTENT_LENGTH,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_FLAG_NUMBER64, WINHTTP_QUERY_STATUS_CODE, WinHttpAddRequestHeaders,
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::Win32::System::Diagnostics::Debug::{
    FORMAT_MESSAGE_FROM_HMODULE, FORMAT_MESSAGE_FROM_SYSTEM, FORMAT_MESSAGE_IGNORE_INSERTS, FormatMessageW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::core::{PCWSTR, w};

use super::{Error, Progress};
use crate::util::W;

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a handle WinHTTP returned, closed once.
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// An open response: the session, connection and request, which must outlive the reads.
struct Response {
    request: Handle,
    _connection: Handle,
    _session: Handle,
}

/// `https://host[:port]/path?query` as (host, port, path). None for anything else.
fn split(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("https://")?;
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (authority, 443),
    };
    if host.is_empty() || host.contains(['@', ' ']) {
        return None;
    }
    Some((host.to_string(), port, path))
}

/// A WinHTTP failure as Windows describes it (in Windows' language).
fn last_error() -> Error {
    // SAFETY: reads the calling thread's last error.
    let code = unsafe { GetLastError() }.0;
    Error::Network(describe(code))
}

fn describe(code: u32) -> String {
    let mut buf = [0u16; 512];
    // SAFETY: WinHTTP's messages live in winhttp.dll; the buffer is ours and its length given.
    let n = unsafe {
        let module = GetModuleHandleW(w!("winhttp.dll")).unwrap_or(HMODULE::default());
        FormatMessageW(
            FORMAT_MESSAGE_FROM_HMODULE | FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
            Some(module.0 as *const c_void),
            code,
            0,
            windows::core::PWSTR(buf.as_mut_ptr()),
            buf.len() as u32,
            None,
        )
    };
    let text = String::from_utf16_lossy(&buf[..n as usize]).trim().to_string();
    if text.is_empty() { format!("Windows error {code}.") } else { text }
}

fn open(url: &str, agent: &str, accept: &str, api: bool) -> Result<Response, Error> {
    let (host, port, path) = split(url).ok_or_else(|| Error::Release(format!("{url} isn't an https URL.")))?;
    let agent = W::new(agent);
    // SAFETY: every string outlives the call that reads it; handles are checked before use.
    unsafe {
        let session = Handle(WinHttpOpen(agent.p(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
        if session.0.is_null() {
            return Err(last_error());
        }
        // Resolve, connect, send, receive: 30 seconds each.
        let _ = WinHttpSetTimeouts(session.0, 30_000, 30_000, 30_000, 30_000);
        let host = W::new(&host);
        let connection = Handle(WinHttpConnect(session.0, host.p(), port, 0));
        if connection.0.is_null() {
            return Err(last_error());
        }
        let path = W::new(&path);
        let request = Handle(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            path.p(),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err(last_error());
        }
        let mut headers = format!("Accept: {accept}\r\n");
        if api {
            headers.push_str("X-GitHub-Api-Version: 2022-11-28\r\n");
        }
        let headers: Vec<u16> = headers.encode_utf16().collect();
        WinHttpAddRequestHeaders(request.0, &headers, WINHTTP_ADDREQ_FLAG_ADD).map_err(|_| last_error())?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|_| last_error())?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|_| last_error())?;
        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut size,
            std::ptr::null_mut(),
        )
        .map_err(|_| last_error())?;
        if status != 200 {
            return Err(Error::Http(status.min(u16::MAX as u32) as u16));
        }
        Ok(Response { request, _connection: connection, _session: session })
    }
}

impl Response {
    /// Content-Length, or 0 when the server doesn't say.
    fn length(&self) -> u64 {
        let mut n = 0u64;
        let mut size = std::mem::size_of::<u64>() as u32;
        // SAFETY: a 64-bit number into a buffer of that size.
        let ok = unsafe {
            WinHttpQueryHeaders(
                self.request.0,
                WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER64,
                PCWSTR::null(),
                Some(&mut n as *mut u64 as *mut c_void),
                &mut size,
                std::ptr::null_mut(),
            )
        };
        if ok.is_ok() { n } else { 0 }
    }

    /// Read the body in chunks, handing each to `sink`.
    fn read(&self, mut sink: impl FnMut(&[u8]) -> Result<(), Error>) -> Result<(), Error> {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut got = 0u32;
            // SAFETY: WinHTTP writes at most buf.len() bytes into buf.
            unsafe { WinHttpReadData(self.request.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut got) }
                .map_err(|_| last_error())?;
            if got == 0 {
                return Ok(());
            }
            sink(&buf[..got as usize])?;
        }
    }
}

/// A local file named by a test feed: a plain path or a file:// URL.
fn local_file(url: &str) -> Option<PathBuf> {
    let Some(rest) = url.strip_prefix("file://") else {
        return (!url.contains("://")).then(|| PathBuf::from(url));
    };
    // file:///C:/dir/a%20b.zip → C:\dir\a b.zip
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let mut bytes = Vec::new();
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && let Some(b) = rest.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            bytes.push(b);
            i += 3;
            continue;
        }
        bytes.push(if raw[i] == b'/' { b'\\' } else { raw[i] });
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

/// GET `url` and return the body. `allow_file`: a test feed may name a local file.
pub fn get(url: &str, agent: &str, accept: &str, allow_file: bool) -> Result<Vec<u8>, Error> {
    if allow_file && let Some(path) = local_file(url) {
        return std::fs::read(&path).map_err(|e| Error::Network(format!("{}: {e}", path.display())));
    }
    let response = open(url, agent, accept, true)?;
    let mut body = Vec::new();
    response.read(|chunk| {
        body.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(body)
}

/// Download `url` to `dest`, counting bytes into `progress`.
pub fn download(url: &str, agent: &str, dest: &Path, progress: &Progress, allow_file: bool) -> Result<(), Error> {
    let saved = |e: std::io::Error| Error::Network(format!("The download couldn't be saved: {e}."));
    if allow_file && url.starts_with("file://") {
        let path = local_file(url).ok_or_else(|| Error::Release(format!("{url} isn't a file URL.")))?;
        let n = std::fs::copy(&path, dest).map_err(saved)?;
        progress.total.store(n, Ordering::SeqCst);
        progress.received.store(n, Ordering::SeqCst);
        return Ok(());
    }
    let response = open(url, agent, "application/octet-stream", false)?;
    progress.total.store(response.length(), Ordering::SeqCst);
    let mut file = std::fs::File::create(dest).map_err(saved)?;
    response.read(|chunk| {
        file.write_all(chunk).map_err(saved)?;
        progress.received.fetch_add(chunk.len() as u64, Ordering::SeqCst);
        Ok(())
    })?;
    file.sync_all().map_err(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_urls_split() {
        assert_eq!(
            split("https://api.github.com/repos/piedol/photon/releases/latest"),
            Some(("api.github.com".into(), 443, "/repos/piedol/photon/releases/latest".into()))
        );
        assert_eq!(split("https://example.com:8443/a?b=c"), Some(("example.com".into(), 8443, "/a?b=c".into())));
        assert_eq!(split("https://example.com?x"), Some(("example.com".into(), 443, "/?x".into())));
        assert_eq!(split("https://example.com"), Some(("example.com".into(), 443, "/".into())));
        for bad in ["http://example.com/", "ftp://x/", "https://", "https://:443/", "https://user@host/", "https://h:x/"] {
            assert_eq!(split(bad), None, "{bad}");
        }
    }

    #[test]
    fn test_feeds_name_local_files() {
        assert_eq!(local_file("file:///C:/Photon%20Test/feed.json"), Some(PathBuf::from("C:\\Photon Test\\feed.json")));
        assert_eq!(local_file("C:\\feeds\\a.json"), Some(PathBuf::from("C:\\feeds\\a.json")));
        assert_eq!(local_file("https://api.github.com/"), None);
    }
}
