//! HTTPS downloads, through `rt_sys::net` (WinHTTP on Windows, `curl` on
//! Linux), as rwall.
//!
//! rwall uses the network only for this: GET requests to the picture
//! services the user turned on, and the weather of a weather widget. No
//! cookies, no account, no telemetry.

pub use rt_sys::net::{encode, split_url, Response, Url, TIMEOUT};

/// Sent with every request.
pub const USER_AGENT: &str = concat!("rwall/", env!("CARGO_PKG_VERSION"));

/// Downloads `url`, refusing answers larger than `limit` bytes and any
/// status but 200.
pub fn get(url: &str, limit: usize) -> Result<Response, String> {
    rt_sys::net::get(url, limit, USER_AGENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reaches Bing (run on request: it needs the network).
    #[test]
    #[ignore]
    fn downloads_from_bing() {
        let r = get("https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=en-US", 1 << 20).unwrap();
        assert!(crate::json::parse(&String::from_utf8_lossy(&r.body)).is_ok());
    }
}
