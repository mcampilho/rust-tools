//! HTTPS downloads: WinHTTP on Windows (the system's TLS and proxy settings,
//! nothing to ship), the `curl` program on Linux.
//!
//! rwall is the only tool of the repository that uses the network, and only
//! for this: GET requests to the picture services the user turned on. No
//! cookies, no account, no telemetry.

use std::time::Duration;

/// Sent with every request.
pub const USER_AGENT: &str = concat!("rwall/", env!("CARGO_PKG_VERSION"));

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
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else {
        (false, url.strip_prefix("http://")?)
    };
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

/// Downloads `url`, refusing answers larger than `limit` bytes and any
/// status but 200.
pub fn get(url: &str, limit: usize) -> Result<Response, String> {
    split_url(url).ok_or_else(|| format!("not a web address: {url}"))?;
    imp::get(url, limit)
}

#[cfg(windows)]
mod imp {
    use super::{split_url, Response, TIMEOUT, USER_AGENT};
    use rt_sys::win::{from_wide, wide};
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
        format!("{what}: {}", std::io::Error::last_os_error())
    }

    pub fn get(url: &str, limit: usize) -> Result<Response, String> {
        let u = split_url(url).ok_or("bad address")?;
        let (agent, host, path, verb) = (wide(USER_AGENT), wide(u.host), wide(u.path), wide("GET"));
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
            let request = H(WinHttpOpenRequest(connect.0, verb.as_ptr(), path.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags));
            if request.0.is_null() {
                return Err(last_error(u.host));
            }
            if WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0
                || WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0
            {
                return Err(last_error(u.host));
            }
            let mut status: u32 = 0;
            let mut len = std::mem::size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                (&mut status as *mut u32).cast(),
                &mut len,
                std::ptr::null_mut(),
            );
            if status != 200 {
                return Err(format!("{}: HTTP {status}", u.host));
            }
            // The address after redirects.
            let mut buf = vec![0u16; 4096];
            let mut bytes = (buf.len() * 2) as u32;
            let final_url = if WinHttpQueryOption(request.0, WINHTTP_OPTION_URL, buf.as_mut_ptr().cast(), &mut bytes) != 0 {
                from_wide(&buf)
            } else {
                url.to_string()
            };
            let mut body = Vec::new();
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                let mut read = 0u32;
                if WinHttpReadData(request.0, chunk.as_mut_ptr().cast(), chunk.len() as u32, &mut read) == 0 {
                    return Err(last_error(u.host));
                }
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read as usize]);
                if body.len() > limit {
                    return Err(format!("{}: answer larger than {} MB", u.host, limit >> 20));
                }
            }
            Ok(Response { body, url: final_url })
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Response, TIMEOUT, USER_AGENT};

    pub fn get(url: &str, limit: usize) -> Result<Response, String> {
        // The address after redirects goes to stderr, the body to stdout.
        let out = std::process::Command::new("curl")
            .args(["--silent", "--show-error", "--fail", "--location", "--proto", "=https,http"])
            .args(["--max-time", &TIMEOUT.as_secs().to_string(), "--max-filesize", &limit.to_string()])
            .args(["--user-agent", USER_AGENT, "--write-out", "%{stderr}%{url_effective}"])
            .arg("--")
            .arg(url)
            .output()
            .map_err(|e| format!("curl: {e}"))?;
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        if !out.status.success() {
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
    fn query_values_are_encoded() {
        assert_eq!(encode("blue sky & sea/ç"), "blue+sky+%26+sea%2F%C3%A7");
    }

    /// Reaches Bing (run on request: it needs the network).
    #[test]
    #[ignore]
    fn downloads_from_bing() {
        let r = get("https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=en-US", 1 << 20).unwrap();
        assert!(crate::json::parse(&String::from_utf8_lossy(&r.body)).is_ok());
    }
}
