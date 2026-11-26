//! HTTPS downloads, through `rt_sys::net` (WinHTTP on Windows, `curl` on
//! Linux), as rwall.
//!
//! rwall uses the network only for this: GET requests to the picture
//! services the user turned on, and the weather of a weather widget. No
//! cookies, no account, no telemetry.

pub use rt_sys::net::{encode, split_url, Response, Url, TIMEOUT};

use crate::i18n::tf;

/// Sent with every request.
pub const USER_AGENT: &str = concat!("rwall/", env!("CARGO_PKG_VERSION"));

/// Downloads `url`, refusing answers larger than `limit` bytes and any
/// status but 200.
pub fn get(url: &str, limit: usize) -> Result<Response, String> {
    rt_sys::net::get(url, limit, USER_AGENT)
}

/// `error` ("bing: …; folders: …") as shown to the user: the services that
/// could not be reached become one sentence, in the language of the thread,
/// that says where to look. Anything else stays as it is.
pub fn explain(error: &str) -> String {
    let mut first = None;
    let mut others = Vec::new();
    for part in error.split("; ") {
        match rt_sys::net::unreachable(part) {
            Some(found) => {
                first.get_or_insert(found);
            }
            None => others.push(part),
        }
    }
    let Some((host, code)) = first else { return error.to_string() };
    let sentence = tf("Could not connect to {} (error {}). Check the Internet connection and the firewall.", &[host, &code.to_string()]);
    std::iter::once(sentence.as_str()).chain(others).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_services_become_one_sentence() {
        let e = "bing: www.bing.com: could not connect (error 12029); picsum: picsum.photos: could not connect (error 12029)";
        let said = explain(e);
        assert_eq!(said.matches("12029").count(), 1);
        assert!(said.contains("www.bing.com") && said.contains("firewall"), "{said}");
        assert!(explain(&format!("{e}; folders: no pictures")).ends_with("; folders: no pictures"));
        assert_eq!(explain("bing: www.bing.com: HTTP 500"), "bing: www.bing.com: HTTP 500");
    }

    /// Reaches Bing (run on request: it needs the network).
    #[test]
    #[ignore]
    fn downloads_from_bing() {
        let r = get("https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=en-US", 1 << 20).unwrap();
        assert!(crate::json::parse(&String::from_utf8_lossy(&r.body)).is_ok());
    }
}
