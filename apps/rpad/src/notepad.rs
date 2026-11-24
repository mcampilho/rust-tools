//! rpad started in place of Windows Notepad (File › Windows Integration › Replace Windows Notepad): Windows runs
//! `"rpad.exe" --from-notepad <Notepad's own command line>`. This turns that into the arguments of a normal launch.

use std::path::Path;

/// `raw` as rpad's own arguments: without the flag, Notepad's path, and Notepad's switches (`/p`, `/pt`, `/a`,
/// `/w`); the rest of the command line is the file (Notepad takes it whole, spaces and all); a name without an
/// extension that is not there opens `name.txt` if that is. Arguments without the flag are returned as they are.
pub fn normalize(raw: &[String]) -> Vec<String> {
    let Some(at) = raw.iter().position(|a| a == rt_sys::win::notepad::FLAG) else { return raw.to_vec() };
    let mut out: Vec<String> = raw[..at].to_vec();
    let rest = &raw[at + 1..];
    let rest = match rest.split_first() {
        Some((first, tail)) if is_notepad(first) => tail,
        _ => rest,
    };
    let names: Vec<&String> = rest.iter().filter(|a| !(a.starts_with('/') && a.len() <= 3)).collect();
    let joined = names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
    if names.len() > 1 && Path::new(&joined).exists() {
        out.push(with_txt(joined));
    } else {
        out.extend(names.into_iter().map(|n| with_txt(n.clone())));
    }
    out
}

fn is_notepad(arg: &str) -> bool {
    let name = arg.rsplit(['\\', '/']).next().unwrap_or(arg).trim_matches('"');
    name.eq_ignore_ascii_case("notepad.exe") || name.eq_ignore_ascii_case("notepad")
}

/// Notepad adds `.txt` to a name that has no extension and is not a file.
fn with_txt(name: String) -> String {
    let p = Path::new(&name);
    if !p.exists() && p.extension().is_none() {
        let t = format!("{name}.txt");
        if Path::new(&t).exists() {
            return t;
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_normal_launch_is_left_alone() {
        let a = args(&["--tray", "a.txt", "/p"]);
        assert_eq!(normalize(&a), a);
    }

    #[test]
    fn notepads_own_path_and_switches_go() {
        let n = rt_sys::win::notepad::FLAG;
        assert_eq!(normalize(&args(&[n, r"C:\Windows\System32\notepad.exe"])), Vec::<String>::new());
        assert_eq!(normalize(&args(&[n, "notepad.exe", "a.txt"])), args(&["a.txt"]));
        assert_eq!(normalize(&args(&[n, "NOTEPAD", "/p", "a.txt"])), args(&["a.txt"]));
        assert_eq!(
            normalize(&args(&["--tray", n, r"C:\Windows\notepad.exe", "/pt", "a.txt", "Printer"])).first().map(String::as_str),
            Some("--tray")
        );
        // Without Notepad's path first, everything is arguments.
        assert_eq!(normalize(&args(&[n, "a.txt"])), args(&["a.txt"]));
    }

    #[test]
    fn the_rest_of_the_line_is_one_file_and_a_name_gets_its_extension() {
        let dir = std::env::temp_dir().join(format!("rpad-notepad-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let spaced = dir.join("my notes.txt");
        let plain = dir.join("todo.txt");
        std::fs::write(&spaced, "x").expect("file");
        std::fs::write(&plain, "x").expect("file");
        let n = rt_sys::win::notepad::FLAG;
        // `notepad C:\...\my notes.txt` unquoted arrives as two arguments.
        let (a, b) = (dir.join("my").display().to_string(), "notes.txt".to_string());
        assert_eq!(normalize(&[n.to_string(), "notepad.exe".to_string(), a, b]), vec![spaced.display().to_string()]);
        let bare = dir.join("todo").display().to_string();
        assert_eq!(normalize(&[n.to_string(), "notepad.exe".to_string(), bare.clone()]), vec![plain.display().to_string()]);
        // A name that is not there stays as it is (rpad opens an empty tab to create it).
        let missing = dir.join("new").display().to_string();
        assert_eq!(normalize(&[n.to_string(), "notepad.exe".to_string(), missing.clone()]), vec![missing]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
