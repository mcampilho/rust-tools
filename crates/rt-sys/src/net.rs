//! HTTPS downloads: WinHTTP on Windows (the system's TLS and proxy settings,
//! nothing to ship), the `curl` program on Linux.
//!
//! The tools stay off the network. The exceptions are rwall (the picture
//! services and the weather the user turned on), rview (the pictures of a
//! document, only with View › Show Online Images on), checking for updates
//! when the user asks (`crate::update`) and rscoop-cli's `download` (the
//! addresses of a manifest, with the headers Scoop would send). Only GET
//! requests, no account, no telemetry.

use std::time::Duration;

/// Longest a request may take, from connecting to the last byte.
pub const TIMEOUT: Duration = Duration::from_secs(60);

/// What came back.
#[derive(Debug)]
pub struct Response {
    pub body: Vec<u8>,
    /// The address after redirects.
    pub url: String,
}

/// The parts of an `https://` (or `http://`) address.
#[derive(Debug, PartialEq, Eq)]
pub struct Url<'a> {
    pub secure: bool,
    pub host: &'a str,
    pub port: u16,
    /// Path and query, starting with '/'.
    pub path: &'a str,
}

/// Splits an absolute http(s) address.
pub fn split_url(url: &str) -> Option<Url<'_>> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") { (true, r) } else { (false, url.strip_prefix("http://")?) };
    let cut = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(cut);
    let path = path.split('#').next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h, p.parse().ok()?),
        _ => (authority, if secure { 443 } else { 80 }),
    };
    let path = if path.is_empty() { "/" } else { path };
    Some(Url { secure, host, port, path })
}

/// Percent-encodes `s` for a query value.
pub fn encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            b' ' => o.push('+'),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

const UNREACHABLE: &str = ": could not connect (error ";

/// The error of a request that never got an answer from `host`; `code` is
/// WinHTTP's, or `curl`'s exit code on Linux.
fn no_connection(host: &str, code: u32) -> String {
    format!("{host}{UNREACHABLE}{code})")
}

/// The host and code of such an error (also with text in front of it), so a
/// program can word it in the user's language.
pub fn unreachable(error: &str) -> Option<(&str, u32)> {
    let (before, after) = error.split_once(UNREACHABLE)?;
    let code = after.split_once(')')?.0.parse().ok()?;
    Some((before.rsplit(": ").next()?, code))
}

/// Downloads `url` as `agent` ("rwall/1.0"), refusing answers larger than
/// `limit` bytes and any status but 200.
pub fn get(url: &str, limit: usize, agent: &str) -> Result<Response, String> {
    split_url(url).ok_or_else(|| format!("not a web address: {url}"))?;
    imp::get(url, limit, agent)
}

/// What to send with the request for an address.
pub type Headers<'a> = dyn Fn(&str) -> Vec<(String, String)> + 'a;

/// Downloads `url` into the file `to`, with no limit of size; `progress` gets
/// the bytes so far and the total (0 when unknown). Returns the size. Any
/// status but 200 is an error, and the file is then not created.
///
/// Redirects are followed one at a time, and `headers` is asked again for
/// each address, so that what is meant for one host (a token, a cookie) is
/// decided anew for the next.
pub fn download(
    url: &str,
    to: &std::path::Path,
    headers: &Headers,
    agent: &str,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<u64, String> {
    split_url(url).ok_or_else(|| format!("not a web address: {url}"))?;
    imp::download(url, to, headers, agent, progress)
}

/// The headers for `url`, refused if one would break the request.
fn headers_for(headers: &Headers, url: &str) -> Result<Vec<(String, String)>, String> {
    let sent = headers(url);
    if sent.iter().any(|(k, v)| k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n'])) {
        return Err("a header with a line break".into());
    }
    Ok(sent)
}

/// The address a redirect from `base` to `location` goes to (`curl` follows
/// them itself).
#[cfg(any(windows, test))]
fn redirected(base: &str, location: &str) -> Option<String> {
    if split_url(location).is_some() {
        return Some(location.to_string());
    }
    let u = split_url(base)?;
    let scheme = if u.secure { "https" } else { "http" };
    if let Some(rest) = location.strip_prefix("//") {
        return Some(format!("{scheme}://{rest}"));
    }
    // Another scheme is not followed.
    if location.contains("://") {
        return None;
    }
    let origin = format!("{scheme}://{}:{}", u.host, u.port);
    if location.starts_with('/') {
        return Some(format!("{origin}{location}"));
    }
    let folder = u.path.split('?').next().unwrap_or("/").rsplit_once('/').map_or("", |(f, _)| f);
    Some(format!("{origin}{folder}/{location}"))
}
#[cfg(windows)]
mod imp {
    use super::{headers_for, no_connection, redirected, split_url, Headers, Response, TIMEOUT};
    use crate::win::{from_wide, wide};
    use std::ffi::c_void;
    use windows_sys::Win32::Networking::WinHttp::*;

    /// A WinHTTP handle, closed when dropped.
    struct H(*mut c_void);

    impl Drop for H {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a handle WinHTTP gave us, closed once.
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }

    fn last_error(what: &str) -> String {
        let e = std::io::Error::last_os_error();
        // WinHTTP's own codes have no text in the system's message table.
        match e.raw_os_error().map(|c| c as u32) {
            Some(
                c @ (ERROR_WINHTTP_TIMEOUT
                | ERROR_WINHTTP_NAME_NOT_RESOLVED
                | ERROR_WINHTTP_CANNOT_CONNECT
                | ERROR_WINHTTP_CONNECTION_ERROR),
            ) => no_connection(what, c),
            Some(c @ 12000..=12999) => format!("{what}: network error {c}"),
            _ => format!("{what}: {e}"),
        }
    }

    /// An answered request; the handles close in the order of the fields.
    struct Request {
        request: H,
        _connect: H,
        _session: H,
    }

    enum Opened {
        Answer(Request),
        /// The `Location` of a redirect, when they are not followed.
        Redirect(String),
    }

    /// Sends `GET url` and waits for status 200.
    fn open(url: &str, user_agent: &str, headers: &str, follow: bool) -> Result<Opened, String> {
        let u = split_url(url).ok_or("bad address")?;
        let (agent, host, path, verb) = (wide(user_agent), wide(u.host), wide(u.path), wide("GET"));
        let ms = TIMEOUT.as_millis() as i32;
        // SAFETY: every pointer is a live NUL-terminated string or a buffer
        // whose size is passed with it; handles are closed by `H`.
        unsafe {
            let session = H(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0));
            if session.0.is_null() {
                return Err(last_error("WinHttpOpen"));
            }
            WinHttpSetTimeouts(session.0, ms, ms, ms, ms);
            let connect = H(WinHttpConnect(session.0, host.as_ptr(), u.port, 0));
            if connect.0.is_null() {
                return Err(last_error(u.host));
            }
            let flags = if u.secure { WINHTTP_FLAG_SECURE } else { 0 };
            let request =
                H(WinHttpOpenRequest(connect.0, verb.as_ptr(), path.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags));
            if request.0.is_null() {
                return Err(last_error(u.host));
            }
            if !follow {
                let never: u32 = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
                if WinHttpSetOption(request.0, WINHTTP_OPTION_REDIRECT_POLICY, (&never as *const u32).cast(), 4) == 0 {
                    return Err(last_error(u.host));
                }
            }
            if !headers.is_empty() {
                let lines = wide(headers);
                if WinHttpAddRequestHeaders(request.0, lines.as_ptr(), u32::MAX, WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE) == 0
                {
                    return Err(last_error(u.host));
                }
            }
            if WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0
                || WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0
            {
                return Err(last_error(u.host));
            }
            let status = number(&request, WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
            if !follow && matches!(status, 301 | 302 | 303 | 307 | 308) {
                return location(&request).map(Opened::Redirect).ok_or_else(|| format!("{}: HTTP {status} without an address", u.host));
            }
            if status != 200 {
                return Err(format!("{}: HTTP {status}", u.host));
            }
            Ok(Opened::Answer(Request { request, _connect: connect, _session: session }))
        }
    }

    fn location(request: &H) -> Option<String> {
        let mut buf = vec![0u16; 8192];
        let mut bytes = (buf.len() * 2) as u32;
        // SAFETY: the buffer and its size in bytes.
        let ok = unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_LOCATION,
                std::ptr::null(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then(|| from_wide(&buf))
    }

    fn number(request: &H, header: u32) -> Option<u32> {
        let mut value: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        // SAFETY: `value` has the size given.
        let ok = unsafe {
            WinHttpQueryHeaders(
                request.0,
                header | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                (&mut value as *mut u32).cast(),
                &mut len,
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(value)
    }

    /// Reads the answer in pieces into `take`.
    fn read(request: &H, host: &str, take: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<(), String> {
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            // SAFETY: the buffer and its size.
            if unsafe { WinHttpReadData(request.0, chunk.as_mut_ptr().cast(), chunk.len() as u32, &mut read) } == 0 {
                return Err(last_error(host));
            }
            if read == 0 {
                return Ok(());
            }
            take(&chunk[..read as usize])?;
        }
    }

    pub fn download(
        url: &str,
        to: &std::path::Path,
        headers: &Headers,
        user_agent: &str,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<u64, String> {
        let mut address = url.to_string();
        let mut answered = None;
        for _ in 0..10 {
            let mut sent = headers_for(headers, &address)?;
            // Nothing private goes in the clear after a secure address.
            if url.starts_with("https:") && address.starts_with("http:") {
                sent.retain(|(name, _)| !["authorization", "cookie"].contains(&name.to_ascii_lowercase().as_str()));
            }
            let lines: String = sent.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
            match open(&address, user_agent, &lines, false)? {
                Opened::Answer(request) => {
                    answered = Some(request);
                    break;
                }
                Opened::Redirect(location) => {
                    address = redirected(&address, &location).ok_or_else(|| format!("redirected to {location}"))?;
                }
            }
        }
        let answered = answered.ok_or_else(|| format!("too many redirects: {url}"))?;
        let host = split_url(&address).ok_or("bad address")?.host;
        // A length over 4 GB does not fit the number; progress is then without a total.
        let total = number(&answered.request, WINHTTP_QUERY_CONTENT_LENGTH).map_or(0, u64::from);
        let fail = |e: std::io::Error| format!("{}: {e}", to.display());
        let mut file = std::fs::File::create(to).map_err(fail)?;
        let mut size = 0u64;
        progress(0, total);
        read(&answered.request, host, &mut |bytes| {
            std::io::Write::write_all(&mut file, bytes).map_err(fail)?;
            size += bytes.len() as u64;
            progress(size, total);
            Ok(())
        })?;
        if total > 0 && size != total {
            return Err(format!("{host}: {size} bytes of {total}"));
        }
        Ok(size)
    }

    pub fn get(url: &str, limit: usize, user_agent: &str) -> Result<Response, String> {
        let host = split_url(url).ok_or("bad address")?.host;
        let Opened::Answer(answered) = open(url, user_agent, "", true)? else { return Err(format!("{host}: redirect")) };
        let request = &answered.request;
        // SAFETY: the handle is live and the buffer has the size given.
        unsafe {
            // The address after redirects.
            let mut buf = vec![0u16; 4096];
            let mut bytes = (buf.len() * 2) as u32;
            let final_url = if WinHttpQueryOption(request.0, WINHTTP_OPTION_URL, buf.as_mut_ptr().cast(), &mut bytes) != 0 {
                from_wide(&buf)
            } else {
                url.to_string()
            };
            let mut body = Vec::new();
            read(request, host, &mut |bytes| {
                body.extend_from_slice(bytes);
                if body.len() > limit {
                    return Err(format!("{host}: answer larger than {} MB", limit >> 20));
                }
                Ok(())
            })?;
            Ok(Response { body, url: final_url })
        }
    }
}
#[cfg(not(windows))]
mod imp {
    use super::{headers_for, no_connection, split_url, Headers, Response, TIMEOUT};

    pub fn download(
        url: &str,
        to: &std::path::Path,
        headers: &Headers,
        user_agent: &str,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<u64, String> {
        let mut curl = std::process::Command::new("curl");
        curl.args(["--silent", "--show-error", "--fail", "--location", "--proto", "=https,http", "--user-agent", user_agent]);
        for (name, value) in headers_for(headers, url)? {
            curl.arg("--header").arg(format!("{name}: {value}"));
        }
        let out = curl.arg("--output").arg(to).arg("--").arg(url).output().map_err(|e| format!("curl: {e}"))?;
        if !out.status.success() {
            let _ = std::fs::remove_file(to);
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        let size = std::fs::metadata(to).map_err(|e| e.to_string())?.len();
        progress(size, size);
        Ok(size)
    }

    pub fn get(url: &str, limit: usize, user_agent: &str) -> Result<Response, String> {
        // The address after redirects goes to stderr, the body to stdout.
        let out = std::process::Command::new("curl")
            .args(["--silent", "--show-error", "--fail", "--location", "--proto", "=https,http"])
            .args(["--max-time", &TIMEOUT.as_secs().to_string(), "--max-filesize", &limit.to_string()])
            .args(["--user-agent", user_agent, "--write-out", "%{stderr}%{url_effective}"])
            .arg("--")
            .arg(url)
            .output()
            .map_err(|e| format!("curl: {e}"))?;
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        if !out.status.success() {
            // Name not resolved, no connection, timed out.
            if let Some(c @ (6 | 7 | 28)) = out.status.code() {
                return Err(no_connection(split_url(url).map_or(url, |u| u.host), c as u32));
            }
            return Err(err.lines().find(|l| l.starts_with("curl:")).unwrap_or("curl failed").to_string());
        }
        Ok(Response { body: out.stdout, url: err.lines().last().unwrap_or(url).to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_split() {
        assert_eq!(
            split_url("https://www.bing.com/HPImageArchive.aspx?format=js&n=8"),
            Some(Url { secure: true, host: "www.bing.com", port: 443, path: "/HPImageArchive.aspx?format=js&n=8" })
        );
        assert_eq!(split_url("http://example.org:8080"), Some(Url { secure: false, host: "example.org", port: 8080, path: "/" }));
        assert_eq!(split_url("https://a.b/x#frag").map(|u| u.path), Some("/x"));
        assert_eq!(split_url("ftp://a.b/"), None);
        assert_eq!(split_url("https://user@a.b/"), None);
        assert_eq!(split_url("https:///x"), None);
    }

    #[test]
    fn a_request_without_answer_is_recognised() {
        let e = format!("bing: {}", no_connection("www.bing.com", 12029));
        assert_eq!(unreachable(&e), Some(("www.bing.com", 12029)));
        assert_eq!(unreachable("www.bing.com: HTTP 500"), None);
    }

    #[test]
    fn redirects_are_resolved() {
        let base = "https://a.org/x/y.zip?q=1";
        assert_eq!(redirected(base, "http://b.org/z").as_deref(), Some("http://b.org/z"));
        assert_eq!(redirected(base, "//b.org/z").as_deref(), Some("https://b.org/z"));
        assert_eq!(redirected(base, "/z").as_deref(), Some("https://a.org:443/z"));
        assert_eq!(redirected(base, "z.zip").as_deref(), Some("https://a.org:443/x/z.zip"));
        assert_eq!(redirected(base, "file:///c:/x"), None);
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(encode("blue sky & sea/ç"), "blue+sky+%26+sea%2F%C3%A7");
    }
}
