//! The registry behind a small interface, so tests can use an in-memory one.

#[cfg(test)]
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Mutex;

pub const REG_SZ: u32 = 1;
pub const REG_EXPAND_SZ: u32 = 2;
pub const REG_MULTI_SZ: u32 = 7;

pub const USER_ENV: (&str, &str) = ("HKCU", "Environment");
pub const MACHINE_ENV: (&str, &str) = ("HKLM", r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment");

/// The data of a string value.
#[derive(Clone, Debug, PartialEq)]
pub enum RegData {
    Str(String),
    Multi(Vec<String>),
}

impl RegData {
    pub fn map(&self, f: &dyn Fn(&str) -> String) -> RegData {
        match self {
            RegData::Str(s) => RegData::Str(f(s)),
            RegData::Multi(v) => RegData::Multi(v.iter().map(|s| f(s)).collect()),
        }
    }

    pub fn strings(&self) -> Vec<String> {
        match self {
            RegData::Str(s) => vec![s.clone()],
            RegData::Multi(v) => v.clone(),
        }
    }
}

/// (key path, value name, one string of the value, type)
pub type WalkItem = (String, String, String, u32);

pub trait Registry: Sync {
    /// (name, string, type) for the string values of one key (one per string of a REG_MULTI_SZ).
    fn values(&self, hive: &str, path: &str) -> Vec<(String, String, u32)>;

    fn get(&self, hive: &str, path: &str, name: &str) -> Option<(String, u32)> {
        self.values(hive, path).into_iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)).map(|(_, v, t)| (v, t))
    }

    /// Every string value below `path`, at most `max_depth` keys down.
    fn walk(&self, hive: &str, path: &str, max_depth: usize) -> Vec<WalkItem>;

    /// Apply `f` to one string value, keeping its type: (old, new, type).
    fn replace_value(
        &self,
        hive: &str,
        path: &str,
        name: &str,
        f: &dyn Fn(&str) -> String,
        write: bool,
    ) -> Result<(RegData, RegData, u32), String>;
}

/// (shown key path, {value name: (data, type)})
#[cfg(test)]
type FakeKey = (String, BTreeMap<String, (RegData, u32)>);

/// In-memory registry for the tests: (hive, lower-case key) -> key.
#[cfg(test)]
#[derive(Default)]
pub struct FakeRegistry {
    data: Mutex<BTreeMap<(String, String), FakeKey>>,
}

#[cfg(test)]
impl FakeRegistry {
    pub fn new() -> FakeRegistry {
        FakeRegistry::default()
    }

    pub fn set(&self, hive: &str, path: &str, name: &str, value: &str, vtype: u32) {
        let mut d = self.data.lock().unwrap();
        let entry = d.entry((hive.to_string(), path.to_lowercase())).or_insert_with(|| (path.to_string(), BTreeMap::new()));
        entry.1.insert(name.to_string(), (RegData::Str(value.to_string()), vtype));
    }

    fn items(vals: &BTreeMap<String, (RegData, u32)>) -> Vec<(String, String, u32)> {
        vals.iter().flat_map(|(n, (d, t))| d.strings().into_iter().map(move |s| (n.clone(), s, *t))).collect()
    }
}

#[cfg(test)]
impl Registry for FakeRegistry {
    fn values(&self, hive: &str, path: &str) -> Vec<(String, String, u32)> {
        let d = self.data.lock().unwrap();
        d.get(&(hive.to_string(), path.to_lowercase())).map(|(_, v)| Self::items(v)).unwrap_or_default()
    }

    fn walk(&self, hive: &str, path: &str, max_depth: usize) -> Vec<WalkItem> {
        let prefix = path.to_lowercase();
        let d = self.data.lock().unwrap();
        let mut out = Vec::new();
        for ((h, p), (shown, vals)) in d.iter() {
            if h != hive || !(*p == prefix || p.starts_with(&format!("{prefix}\\"))) {
                continue;
            }
            if p.matches('\\').count() - prefix.matches('\\').count() > max_depth {
                continue;
            }
            for (n, s, t) in Self::items(vals) {
                out.push((shown.clone(), n, s, t));
            }
        }
        out
    }

    fn replace_value(
        &self,
        hive: &str,
        path: &str,
        name: &str,
        f: &dyn Fn(&str) -> String,
        write: bool,
    ) -> Result<(RegData, RegData, u32), String> {
        let mut d = self.data.lock().unwrap();
        let entry = d.get_mut(&(hive.to_string(), path.to_lowercase())).ok_or("value not found")?;
        let (old, t) = entry.1.get(name).cloned().ok_or("value not found")?;
        let new = old.map(f);
        if write && new != old {
            entry.1.insert(name.to_string(), (new.clone(), t));
        }
        Ok((old, new, t))
    }
}

#[cfg(windows)]
pub use win::WinRegistry;

#[cfg(windows)]
mod win {
    use super::*;
    use super::{REG_EXPAND_SZ, REG_MULTI_SZ, REG_SZ};
    use crate::sys::wide;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE,
    };

    pub struct WinRegistry;

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the key was opened by RegOpenKeyExW.
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open(hive: &str, path: &str, access: u32) -> Result<Key, String> {
        let root = if hive == "HKLM" { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
        let mut h: HKEY = std::ptr::null_mut();
        let p = wide(path);
        // SAFETY: valid strings and out-parameter.
        let r = unsafe { RegOpenKeyExW(root, p.as_ptr(), 0, access, &mut h) };
        if r != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(r as i32).to_string());
        }
        Ok(Key(h))
    }

    fn decode(data: &[u8], t: u32) -> Option<RegData> {
        let units = crate::paths::u16s(data, false);
        match t {
            REG_SZ | REG_EXPAND_SZ => {
                let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
                Some(RegData::Str(String::from_utf16_lossy(&units[..end])))
            }
            REG_MULTI_SZ => {
                let mut v = Vec::new();
                for part in units.split(|&u| u == 0) {
                    if part.is_empty() {
                        break;
                    }
                    v.push(String::from_utf16_lossy(part));
                }
                Some(RegData::Multi(v))
            }
            _ => None,
        }
    }

    fn encode(d: &RegData) -> Vec<u8> {
        let mut units: Vec<u16> = Vec::new();
        match d {
            RegData::Str(s) => {
                units.extend(s.encode_utf16());
                units.push(0);
            }
            RegData::Multi(v) => {
                for s in v {
                    units.extend(s.encode_utf16());
                    units.push(0);
                }
                units.push(0);
            }
        }
        units.iter().flat_map(|u| u.to_le_bytes()).collect()
    }

    fn all_values(k: &Key) -> Vec<(String, RegData, u32)> {
        let mut out = Vec::new();
        let (mut max_name, mut max_data) = (0u32, 0u32);
        // SAFETY: out-parameters only.
        unsafe {
            RegQueryInfoKeyW(
                k.0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut max_name,
                &mut max_data,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
        let mut name = vec![0u16; max_name as usize + 2];
        let mut data = vec![0u8; max_data as usize + 4];
        let mut i = 0;
        loop {
            let mut nlen = name.len() as u32;
            let mut dlen = data.len() as u32;
            let mut t = 0u32;
            // SAFETY: buffers are as long as stated.
            let r = unsafe { RegEnumValueW(k.0, i, name.as_mut_ptr(), &mut nlen, std::ptr::null(), &mut t, data.as_mut_ptr(), &mut dlen) };
            if r == 234 {
                // ERROR_MORE_DATA: a value grew meanwhile.
                data.resize(data.len() * 2 + 1024, 0);
                name.resize(name.len() * 2 + 64, 0);
                continue;
            }
            if r != ERROR_SUCCESS {
                break;
            }
            i += 1;
            if let Some(d) = decode(&data[..dlen as usize], t) {
                out.push((String::from_utf16_lossy(&name[..nlen as usize]), d, t));
            }
        }
        out
    }

    fn subkeys(k: &Key) -> Vec<String> {
        let mut out = Vec::new();
        let mut name = vec![0u16; 512];
        let mut i = 0;
        loop {
            let mut n = name.len() as u32;
            // SAFETY: buffer as long as stated.
            let r = unsafe {
                RegEnumKeyExW(
                    k.0,
                    i,
                    name.as_mut_ptr(),
                    &mut n,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if r != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..n as usize]));
            i += 1;
        }
        out
    }

    fn flatten(vals: Vec<(String, RegData, u32)>) -> Vec<(String, String, u32)> {
        vals.into_iter().flat_map(|(n, d, t)| d.strings().into_iter().map(move |s| (n.clone(), s, t))).collect()
    }

    impl Registry for WinRegistry {
        fn values(&self, hive: &str, path: &str) -> Vec<(String, String, u32)> {
            open(hive, path, KEY_READ).map(|k| flatten(all_values(&k))).unwrap_or_default()
        }

        fn walk(&self, hive: &str, path: &str, max_depth: usize) -> Vec<WalkItem> {
            let mut out = Vec::new();
            let mut stack = vec![(path.to_string(), 0usize)];
            while let Some((p, depth)) = stack.pop() {
                let Ok(k) = open(hive, &p, KEY_READ) else { continue };
                for (n, s, t) in flatten(all_values(&k)) {
                    out.push((p.clone(), n, s, t));
                }
                if depth < max_depth {
                    for s in subkeys(&k).into_iter().rev() {
                        stack.push((format!("{p}\\{s}"), depth + 1));
                    }
                }
            }
            out
        }

        fn replace_value(
            &self,
            hive: &str,
            path: &str,
            name: &str,
            f: &dyn Fn(&str) -> String,
            write: bool,
        ) -> Result<(RegData, RegData, u32), String> {
            let access = KEY_QUERY_VALUE | if write { KEY_SET_VALUE } else { 0 };
            let k = open(hive, path, access)?;
            let (_, old, t) =
                all_values(&k).into_iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)).ok_or_else(|| "value not found".to_string())?;
            let new = old.map(f);
            if write && new != old {
                let bytes = encode(&new);
                let n = wide(name);
                // SAFETY: valid key, name and data.
                let r = unsafe { RegSetValueExW(k.0, n.as_ptr(), 0, t, bytes.as_ptr(), bytes.len() as u32) };
                if r != ERROR_SUCCESS {
                    return Err(std::io::Error::from_raw_os_error(r as i32).to_string());
                }
            }
            Ok((old, new, t))
        }
    }
}
