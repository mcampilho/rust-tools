//! Reading the registry: a key opened in the 64-bit or the 32-bit view, its
//! subkeys and values, and a user's hive file (`NTUSER.DAT`) loaded on its
//! own to be read. Nothing here writes.

use super::{from_wide, wide};
use std::path::Path;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegLoadAppKeyW, RegOpenKeyExW, RegQueryInfoKeyW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    HKEY_USERS, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_MULTI_SZ, REG_QWORD, REG_SZ,
};

/// Where a key starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    LocalMachine,
    CurrentUser,
    Users,
}

/// Which view of the registry: 64-bit programs and 32-bit ones see some
/// keys (`Software`) in different places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Bits64,
    Bits32,
}

/// A value, by its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Str(String),
    /// `REG_EXPAND_SZ`, as stored (`%ProgramFiles%\...`).
    Expand(String),
    Dword(u32),
    Qword(u64),
    Multi(Vec<String>),
    Binary(Vec<u8>),
    /// Another type: its number and size.
    Other(u32, usize),
}

impl Value {
    /// The value as text, as `regedit` shows it.
    pub fn text(&self) -> String {
        match self {
            Value::Str(s) | Value::Expand(s) => s.clone(),
            Value::Dword(v) => format!("0x{v:08x} ({v})"),
            Value::Qword(v) => format!("0x{v:016x} ({v})"),
            Value::Multi(v) => v.join(" · "),
            Value::Binary(b) => b.iter().take(64).map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ") + if b.len() > 64 { " …" } else { "" },
            Value::Other(t, n) => format!("(type {t}, {n} bytes)"),
        }
    }
}

/// An open key, closed when dropped. A hive loaded by [`Key::load_hive`] is
/// unloaded with it.
#[derive(Debug)]
pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by this module and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

fn root_handle(r: Root) -> HKEY {
    match r {
        Root::LocalMachine => HKEY_LOCAL_MACHINE,
        Root::CurrentUser => HKEY_CURRENT_USER,
        Root::Users => HKEY_USERS,
    }
}

fn view_flag(v: View) -> u32 {
    match v {
        View::Bits64 => KEY_WOW64_64KEY,
        View::Bits32 => KEY_WOW64_32KEY,
    }
}

fn open_under(parent: HKEY, path: &str, view: View) -> Option<Key> {
    let p = wide(path);
    let mut out: HKEY = std::ptr::null_mut();
    // SAFETY: a valid parent handle, a NUL-terminated path, an output handle.
    let rc = unsafe { RegOpenKeyExW(parent, p.as_ptr(), 0, KEY_READ | view_flag(view), &mut out) };
    (rc == ERROR_SUCCESS).then_some(Key(out))
}

impl Key {
    /// Opens `path` under a root, for reading, in a view.
    pub fn open(root: Root, path: &str, view: View) -> Option<Key> {
        open_under(root_handle(root), path, view)
    }

    /// Opens a key below this one.
    pub fn sub(&self, path: &str, view: View) -> Option<Key> {
        open_under(self.0, path, view)
    }

    /// Loads a hive file (another user's `NTUSER.DAT`) on its own, to read;
    /// it is unloaded when the key is dropped. Fails while the file is in use
    /// (that user has a session: read it under `HKEY_USERS`) or without the
    /// rights to read it. Err: the system's error code.
    pub fn load_hive(file: &Path) -> Result<Key, u32> {
        let f = wide(&file.to_string_lossy());
        let mut out: HKEY = std::ptr::null_mut();
        // SAFETY: a NUL-terminated path and an output handle.
        let rc = unsafe { RegLoadAppKeyW(f.as_ptr(), &mut out, KEY_READ, 0, 0) };
        if rc == ERROR_SUCCESS {
            Ok(Key(out))
        } else {
            Err(rc)
        }
    }

    /// The names of the subkeys.
    pub fn subkeys(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = vec![0u16; 256];
        for i in 0.. {
            let mut len = buf.len() as u32;
            // SAFETY: the buffer holds `len` characters; the other outputs are optional.
            let rc = unsafe {
                RegEnumKeyExW(self.0, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        out
    }

    /// All the values, with their names (empty: the default value).
    pub fn values(&self) -> Vec<(String, Value)> {
        let (mut max_name, mut max_data) = (0u32, 0u32);
        // SAFETY: only the two sizes are asked for; the rest are null.
        unsafe {
            RegQueryInfoKeyW(
                self.0,
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
            )
        };
        let mut name = vec![0u16; max_name as usize + 2];
        let mut data = vec![0u8; max_data as usize + 4];
        let mut out = Vec::new();
        for i in 0.. {
            let mut nlen = name.len() as u32;
            let mut dlen = data.len() as u32;
            let mut kind = 0u32;
            // SAFETY: both buffers are as large as stated.
            let rc = unsafe { RegEnumValueW(self.0, i, name.as_mut_ptr(), &mut nlen, std::ptr::null(), &mut kind, data.as_mut_ptr(), &mut dlen) };
            if rc != ERROR_SUCCESS {
                break;
            }
            let n = String::from_utf16_lossy(&name[..nlen as usize]);
            out.push((n, decode(kind, &data[..dlen as usize])));
        }
        out
    }

    /// A value by its name.
    pub fn value(&self, name: &str) -> Option<Value> {
        self.values().into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }

    /// A text value (`REG_SZ` or `REG_EXPAND_SZ`, environment variables
    /// expanded), trimmed; None when empty.
    pub fn string(&self, name: &str) -> Option<String> {
        let s = match self.value(name)? {
            Value::Str(s) => s,
            Value::Expand(s) => expand(&s),
            _ => return None,
        };
        let s = s.trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// A number value (`REG_DWORD`, or `REG_QWORD`, or text holding a number).
    pub fn number(&self, name: &str) -> Option<u64> {
        match self.value(name)? {
            Value::Dword(v) => Some(v as u64),
            Value::Qword(v) => Some(v),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

fn utf16(data: &[u8]) -> Vec<u16> {
    data.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()
}

fn decode(kind: u32, data: &[u8]) -> Value {
    match kind {
        REG_SZ => Value::Str(from_wide(&utf16(data))),
        REG_EXPAND_SZ => Value::Expand(from_wide(&utf16(data))),
        REG_DWORD if data.len() >= 4 => Value::Dword(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
        REG_QWORD if data.len() >= 8 => Value::Qword(u64::from_le_bytes(data[..8].try_into().unwrap_or([0; 8]))),
        REG_MULTI_SZ => {
            let w = utf16(data);
            Value::Multi(w.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect())
        }
        REG_BINARY => Value::Binary(data.to_vec()),
        k => Value::Other(k, data.len()),
    }
}

/// `%VAR%` in a text replaced by the variables of this process.
pub fn expand(s: &str) -> String {
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    if !s.contains('%') {
        return s.to_string();
    }
    let w = wide(s);
    let mut out = vec![0u16; 1024];
    // SAFETY: a NUL-terminated input and an output buffer of the size given.
    let n = unsafe { ExpandEnvironmentStringsW(w.as_ptr(), out.as_mut_ptr(), out.len() as u32) } as usize;
    if n == 0 {
        return s.to_string();
    }
    if n > out.len() {
        out = vec![0u16; n];
        // SAFETY: as above, with the size asked for.
        unsafe { ExpandEnvironmentStringsW(w.as_ptr(), out.as_mut_ptr(), out.len() as u32) };
    }
    from_wide(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_system() {
        let k = Key::open(Root::LocalMachine, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", View::Bits64).expect("CurrentVersion");
        assert!(k.string("ProductName").is_some());
        assert!(k.number("CurrentMajorVersionNumber").is_some_and(|v| v >= 10));
        assert!(!k.subkeys().is_empty());
        assert!(Key::open(Root::LocalMachine, "SOFTWARE\\No such key here", View::Bits64).is_none());
        let env = Key::open(Root::CurrentUser, "Environment", View::Bits64).expect("Environment");
        assert!(!env.values().is_empty());
        assert!(expand("%SystemRoot%\\x").to_lowercase().ends_with("windows\\x"));
        assert_eq!(expand("plain"), "plain");
        assert_eq!(decode(REG_MULTI_SZ, &[b'a', 0, 0, 0, b'b', 0, 0, 0, 0, 0]), Value::Multi(vec!["a".into(), "b".into()]));
        assert!(Key::load_hive(Path::new("C:\\no\\such\\NTUSER.DAT")).is_err());
        // This user's key under HKEY_USERS is the same as HKEY_CURRENT_USER.
        let sid = crate::win::user_sid().expect("the user's SID");
        assert!(sid.starts_with("S-1-5-"), "{sid}");
        assert!(Key::open(Root::Users, &format!("{sid}\\Environment"), View::Bits64).is_some());
        // Not elevated as someone else: the session's user is this process's.
        let me = crate::win::session_user().expect("the session's user");
        assert_eq!(crate::win::sid_of(&me).as_deref(), Some(sid.as_str()));
        assert!(crate::win::sid_of("no such account here 123").is_none());
    }
}
