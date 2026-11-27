//! `lib\download.ps1` and `cache_path`: where a download is kept, what its
//! hash must be and what is sent with the request. Nothing is fetched here.

use crate::install::Scoop;
use crate::json::Json;
use crate::manifest::{strings, Arch, Manifest};
use regex_lite::Regex;
use sha2::Digest;
use std::io::Read;
use std::path::{Path, PathBuf};

fn leaf(url: &str) -> &str {
    url.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(url)
}

/// `url_filename`: the name the file gets in the app's folder.
pub fn file_name(url: &str) -> &str {
    leaf(url).split('?').next().unwrap_or("")
}

/// `[System.IO.Path]::GetExtension`, with the dot.
fn extension(url: &str) -> &str {
    let name = url.rsplit(['/', '\\']).next().unwrap_or(url);
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[i..],
        _ => "",
    }
}

/// `cache_path`: `app#version#sha7.ext`, or the older name when it is there.
pub fn cache_path(cache: &Path, app: &str, version: &str, url: &str) -> PathBuf {
    let mut underscored = String::new();
    for c in url.chars() {
        if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' {
            underscored.push(c);
        } else if !underscored.ends_with('_') || underscored.is_empty() {
            underscored.push('_');
        }
    }
    let old = cache.join(format!("{app}#{version}#{underscored}"));
    if old.is_file() {
        return old;
    }
    let sha = rt_core::sha256::hex(url.as_bytes());
    cache.join(format!("{app}#{version}#{}{}", &sha[..7], extension(url)))
}

/// The expected hash of `url`: the one in the same place of the list.
pub fn expected_hash(manifest: &Manifest, arch: Arch, url: &str) -> Option<String> {
    let hashes = strings(manifest.arch("hash", arch));
    let index = manifest.urls(arch).iter().position(|u| u == url)?;
    hashes.get(index).cloned()
}

#[derive(Debug, PartialEq, Eq)]
pub enum HashError {
    UnknownKind(String),
    Read(String),
    /// (expected, actual)
    Differs(String, String),
}

fn digest<D: Digest>(file: &Path) -> std::io::Result<String> {
    let mut reader = std::fs::File::open(file)?;
    let mut hasher = D::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `check_hash`: `[md5:|sha1:|sha256:|sha512:]hex`, SHA-256 when not said.
pub fn check_hash(file: &Path, expected: &str) -> Result<(), HashError> {
    let (kind, hex) = expected.split_once(':').unwrap_or(("sha256", expected));
    let actual = match kind.to_ascii_lowercase().as_str() {
        "md5" => digest::<md5::Md5>(file),
        "sha1" => digest::<sha1::Sha1>(file),
        "sha256" => digest::<sha2::Sha256>(file),
        "sha512" => digest::<sha2::Sha512>(file),
        _ => return Err(HashError::UnknownKind(kind.to_string())),
    }
    .map_err(|e| HashError::Read(e.to_string()))?;
    if actual.eq_ignore_ascii_case(hex) {
        Ok(())
    } else {
        Err(HashError::Differs(hex.to_ascii_lowercase(), actual))
    }
}

/// An address only Scoop knows how to turn into a download.
pub fn needs_scoop(scoop: &Scoop, url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let token = !scoop.setting("gh_token").str().is_empty();
    lower.starts_with("ftp:")
        || lower.contains("fosshub.com/")
        // A release of a private repository needs the API.
        || (token && lower.contains("github.com/") && lower.contains("/releases/download/"))
}

/// The address to ask for: without the `#/name` fragment, and SourceForge's
/// direct form.
pub fn request_url(url: &str) -> String {
    let url = url.split('#').next().unwrap_or(url);
    let sourceforge = Regex::new(r"(?:downloads\.)?sourceforge\.net/projects?/([^/]+)/(?:files/)?(.*?)(?:$|/download|\?)");
    if let Some(c) = sourceforge.ok().and_then(|r| r.captures(url)) {
        return format!("https://downloads.sourceforge.net/project/{}/{}", &c[1], &c[2]);
    }
    url.to_string()
}

/// What `Invoke-Download` sends with the request for `url`.
pub fn headers(scoop: &Scoop, manifest: &Manifest, url: &str, github_token: Option<&str>) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    if !(url.contains("sourceforge.net") || url.contains("portableapps.com")) {
        headers.push(("Referer".into(), url[..url.len() - leaf(url).len()].to_string()));
    }
    if url.starts_with("https://api.github.com/repos") {
        headers.push(("Accept".into(), "application/octet-stream".into()));
        if let Some(token) = github_token {
            headers.push(("Authorization".into(), format!("Bearer {token}")));
        }
        headers.push(("X-GitHub-Api-Version".into(), "2022-11-28".into()));
    }
    if let Json::Obj(cookies) = manifest.get("cookie") {
        let value: Vec<String> = cookies.iter().map(|(k, v)| format!("{k}={}", v.str())).collect();
        headers.push(("Cookie".into(), value.join(";")));
    }
    for host in scoop.setting("private_hosts").items() {
        let matches = Regex::new(&format!("(?i){}", host.get("match").str())).is_ok_and(|r| r.is_match(url));
        if !host.get("match").str().is_empty() && matches {
            for (name, value) in host.get("headers").str().lines().filter_map(|l| l.split_once('=')) {
                headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name.trim()));
                headers.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Tree;

    #[test]
    fn names_in_the_cache() {
        let t = Tree::new("cache-names");
        let cache = t.dir("cache");
        let url = "https://example.org/a/tool-1.0.exe#/dl.7z";
        let sha = &rt_core::sha256::hex(url.as_bytes())[..7];
        assert_eq!(cache_path(&cache, "tool", "1.0", url), cache.join(format!("tool#1.0#{sha}.7z")));
        assert_eq!(file_name(url), "dl.7z");
        assert_eq!(file_name("https://x/get.php?file=a.zip"), "get.php");
        assert_eq!(extension("https://x.org/download?id=1"), "");
        // The older name, when the file is already there.
        let old = t.file("cache/tool#1.0#https_example.org_a_tool-1.0.exe_dl.7z", "");
        assert_eq!(cache_path(&cache, "tool", "1.0", url), old);
    }

    #[test]
    fn hashes_of_every_kind() {
        let t = Tree::new("hashes");
        let file = t.file("f", "abc");
        for hash in [
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "SHA256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
            "md5:900150983cd24fb0d6963f7d28e17f72",
            "sha1:a9993e364706816aba3e25717850c26c9cd0d89d",
            "sha512:ddaf35a193617aba cc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
        ] {
            assert_eq!(check_hash(&file, &hash.replace(' ', "")), Ok(()), "{hash}");
        }
        assert!(matches!(check_hash(&file, "00"), Err(HashError::Differs(..))));
        assert_eq!(check_hash(&file, "crc:00"), Err(HashError::UnknownKind("crc".into())));
    }

    #[test]
    fn what_is_sent() {
        let t = Tree::new("headers");
        t.file("root/config.json", "");
        let mut s = t.scoop();
        s.config = crate::json::parse(r#"{"private_hosts":[{"match":"example\\.org","headers":"Token = abc\nReferer=r"}]}"#).unwrap();
        let m = Manifest(crate::json::parse(r#"{"cookie":{"a":"1","b":"2"}}"#).unwrap());
        let sent = headers(&s, &m, "https://example.org/x/y.zip", None);
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(sent, [pair("Cookie", "a=1;b=2"), pair("Token", "abc"), pair("Referer", "r")]);
        let sent = headers(&s, &Manifest(Json::Null), "https://other.net/x/y.zip", None);
        assert_eq!(sent, [pair("Referer", "https://other.net/x/")]);
        assert_eq!(request_url("https://x/a.exe#/dl.7z"), "https://x/a.exe");
        assert_eq!(
            request_url("https://sourceforge.net/projects/tool/files/v1/tool.zip/download"),
            "https://downloads.sourceforge.net/project/tool/v1/tool.zip"
        );
        assert!(needs_scoop(&s, "ftp://x/y") && needs_scoop(&s, "https://www.fosshub.com/a.html/b.exe"));
        assert!(!needs_scoop(&s, "https://github.com/a/b/releases/download/v1/x.zip"));
    }
}
