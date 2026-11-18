//! Registry keys and values saved to a `.reg` file before they are deleted
//! (double-click the file to put them back), and the deleting. Used by
//! rinst's leftovers and rclean's registry cleaning.
//!
//! The lines of a `.reg` file are plain text, written (and tested) on any
//! system; exporting and deleting are Windows' (`reg.exe`, 64-bit view).

/// The first line of a `.reg` file.
pub const HEADER: &str = "Windows Registry Editor Version 5.00";

/// A value's data, by its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Data {
    Str(String),
    /// `REG_EXPAND_SZ`, as stored.
    Expand(String),
    Dword(u32),
    Qword(u64),
    Multi(Vec<String>),
    Binary(Vec<u8>),
}

fn hex(bytes: impl IntoIterator<Item = u8>) -> String {
    bytes.into_iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(",")
}

fn utf16z(s: &str) -> impl Iterator<Item = u8> + '_ {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes())
}

/// A value as a `.reg` line: `"name"="text"`, `dword:…`, `hex(2):…`…
/// (`@` for the default value).
pub fn value_line(name: &str, d: &Data) -> String {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let n = if name.is_empty() { "@".to_string() } else { format!("\"{}\"", esc(name)) };
    match d {
        Data::Str(s) => format!("{n}=\"{}\"", esc(s)),
        Data::Expand(s) => format!("{n}=hex(2):{}", hex(utf16z(s))),
        Data::Dword(v) => format!("{n}=dword:{v:08x}"),
        Data::Qword(v) => format!("{n}=hex(b):{}", hex(v.to_le_bytes())),
        Data::Multi(v) => {
            let mut b: Vec<u8> = v.iter().flat_map(|s| utf16z(s).collect::<Vec<_>>()).collect();
            b.extend([0, 0]);
            format!("{n}=hex(7):{}", hex(b))
        }
        Data::Binary(b) => format!("{n}=hex:{}", hex(b.iter().copied())),
    }
}

/// A `.reg` file's bytes: UTF-16 with its mark, as `reg export` writes it.
pub fn encode(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
    bytes
}

/// A key or a value to save and delete. `key` as `regedit` writes it
/// (`HKEY_CURRENT_USER\Software\X`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    /// A value of the key (empty: the default one); None: the whole key.
    pub value: Option<String>,
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::*;
    use crate::win::reg::{Key, Root, Value, View};
    use std::path::Path;

    /// A key's root and the rest: `HKEY_LOCAL_MACHINE\SOFTWARE\X`.
    pub fn split_key(path: &str) -> Option<(Root, &str)> {
        let (root, rest) = path.split_once('\\')?;
        let r = match root.to_uppercase().as_str() {
            "HKEY_LOCAL_MACHINE" | "HKLM" => Root::LocalMachine,
            "HKEY_CURRENT_USER" | "HKCU" => Root::CurrentUser,
            "HKEY_USERS" | "HKU" => Root::Users,
            _ => return None,
        };
        Some((r, rest))
    }

    pub fn key_exists(path: &str) -> bool {
        split_key(path).is_some_and(|(r, rest)| Key::open(r, rest, View::Bits64).is_some())
    }

    /// A value's data, for its `.reg` line (other types are not saved).
    pub fn data_of(v: Value) -> Option<Data> {
        Some(match v {
            Value::Str(s) => Data::Str(s),
            Value::Expand(s) => Data::Expand(s),
            Value::Dword(v) => Data::Dword(v),
            Value::Qword(v) => Data::Qword(v),
            Value::Multi(v) => Data::Multi(v),
            Value::Binary(b) => Data::Binary(b),
            Value::Other(..) => return None,
        })
    }

    /// Runs a tool of Windows without a window; what it said when it fails.
    pub fn run_hidden(exe: &str, args: &[&str]) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        let o = std::process::Command::new(exe)
            .args(args)
            .creation_flags(NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if o.status.success() {
            return Ok(());
        }
        let mut t = String::from_utf8_lossy(&o.stderr).trim().to_string();
        if t.is_empty() {
            t = String::from_utf8_lossy(&o.stdout).trim().to_string();
        }
        Err(t)
    }

    /// A `.reg` file as `reg export` writes it (UTF-16), as text.
    fn read_reg(path: &Path) -> Option<String> {
        let b = std::fs::read(path).ok()?;
        let u: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        Some(String::from_utf16_lossy(&u).trim_start_matches('\u{feff}').to_string())
    }

    /// A whole key (and what is below it) as `.reg` text, without the header.
    pub fn export_key(path: &str) -> Result<String, String> {
        let tmp = std::env::temp_dir()
            .join(format!("rt-regfile-{}-{:?}.reg", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
        let r = run_hidden("reg.exe", &["export", path, &tmp.to_string_lossy(), "/y", "/reg:64"])
            .and_then(|_| read_reg(&tmp).ok_or_else(|| "export".to_string()));
        let _ = std::fs::remove_file(&tmp);
        r.map(|t| {
            let mut t = t.trim_start().trim_start_matches(HEADER).trim_start_matches(['\r', '\n']).to_string();
            t += "\r\n";
            t
        })
    }

    /// One value as `.reg` text (`[key]` and its line); None when it is not
    /// there or of a type not saved.
    pub fn export_value(key: &str, name: &str) -> Option<String> {
        let (r, rest) = split_key(key)?;
        let data = data_of(Key::open(r, rest, View::Bits64)?.value(name)?)?;
        Some(format!("[{key}]\r\n{}\r\n\r\n", value_line(name, &data)))
    }

    pub fn delete_key(path: &str) -> Result<(), String> {
        run_hidden("reg.exe", &["delete", path, "/f", "/reg:64"])
    }

    pub fn delete_value(key: &str, name: &str) -> Result<(), String> {
        if name.is_empty() {
            run_hidden("reg.exe", &["delete", key, "/ve", "/f", "/reg:64"])
        } else {
            run_hidden("reg.exe", &["delete", key, "/v", name, "/f", "/reg:64"])
        }
    }

    /// Saves every entry to `backup` (one `.reg` file), then deletes those
    /// saved. Nothing is deleted without its copy; an entry already gone
    /// counts as deleted.
    pub fn save_then_delete(entries: &[Entry], backup: &Path) -> Vec<Result<(), String>> {
        let mut results: Vec<Result<(), String>> = vec![Ok(()); entries.len()];
        let mut text = format!("{HEADER}\r\n");
        let mut saved = vec![false; entries.len()];
        for (i, e) in entries.iter().enumerate() {
            let part = match &e.value {
                None => export_key(&e.key),
                Some(n) => export_value(&e.key, n).ok_or_else(|| "value".to_string()),
            };
            match part {
                Ok(t) => {
                    text += &t;
                    saved[i] = true;
                }
                // Already gone: nothing to save, nothing to delete.
                Err(_) if !key_exists(&e.key) => {}
                Err(err) => results[i] = Err(err),
            }
        }
        if saved.iter().any(|s| *s) {
            let written = backup.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(backup, encode(&text)));
            if let Err(err) = written {
                for (i, s) in saved.iter_mut().enumerate() {
                    if *s {
                        results[i] = Err(err.to_string());
                        *s = false;
                    }
                }
            }
        }
        for (i, e) in entries.iter().enumerate() {
            if !saved[i] {
                continue;
            }
            results[i] = match &e.value {
                None => delete_key(&e.key),
                // Its key went with another entry.
                Some(_) if !key_exists(&e.key) => Ok(()),
                Some(n) => delete_value(&e.key, n),
            };
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_lines() {
        assert_eq!(value_line("App", &Data::Str("\"C:\\A\\a.exe\" -s".into())), "\"App\"=\"\\\"C:\\\\A\\\\a.exe\\\" -s\"");
        assert_eq!(value_line("", &Data::Str("x".into())), "@=\"x\"");
        assert_eq!(value_line("E", &Data::Expand("a".into())), "\"E\"=hex(2):61,00,00,00");
        assert_eq!(value_line("C:\\x.dll", &Data::Dword(3)), "\"C:\\\\x.dll\"=dword:00000003");
        assert_eq!(value_line("M", &Data::Multi(vec!["a".into()])), "\"M\"=hex(7):61,00,00,00,00,00");
        assert_eq!(value_line("Q", &Data::Qword(1)), "\"Q\"=hex(b):01,00,00,00,00,00,00,00");
        assert_eq!(&encode("A")[..4], &[0xFF, 0xFE, 0x41, 0x00]);
    }

    /// Saves and deletes a key and a value of its own under HKCU, then puts
    /// them back from the `.reg` file.
    #[cfg(windows)]
    #[test]
    fn saves_then_deletes() {
        use crate::win::reg::{Key, Root, View};
        let key = format!("HKEY_CURRENT_USER\\Software\\rt-regfile-test-{}", std::process::id());
        run_hidden("reg.exe", &["add", &format!("{key}\\Sub"), "/v", "Keep", "/d", "1", "/f", "/reg:64"]).unwrap();
        run_hidden("reg.exe", &["add", &key, "/v", "Gone", "/t", "REG_DWORD", "/d", "7", "/f", "/reg:64"]).unwrap();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("regfile-{}", std::process::id()));
        let backup = dir.join("saved.reg");
        let r = save_then_delete(
            &[Entry { key: key.clone(), value: Some("Gone".into()) }, Entry { key: format!("{key}\\Sub"), value: None }],
            &backup,
        );
        assert_eq!(r, [Ok(()), Ok(())]);
        let rest = key.trim_start_matches("HKEY_CURRENT_USER\\");
        let k = Key::open(Root::CurrentUser, rest, View::Bits64).unwrap();
        assert!(k.value("Gone").is_none() && k.sub("Sub", View::Bits64).is_none());
        let text = String::from_utf16_lossy(
            &std::fs::read(&backup).unwrap()[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>(),
        );
        assert!(text.starts_with(HEADER) && text.contains("\"Gone\"=dword:00000007") && text.contains("\"Keep\"=\"1\""), "{text}");
        run_hidden("reg.exe", &["import", &backup.to_string_lossy(), "/reg:64"]).unwrap();
        assert!(Key::open(Root::CurrentUser, rest, View::Bits64).unwrap().value("Gone").is_some());
        let _ = delete_key(&key);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
