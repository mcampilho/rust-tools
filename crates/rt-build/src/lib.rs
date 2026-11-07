//! Embeds the application icon, version information and (optionally) a
//! manifest into the Windows executable, from the app's `build.rs`.
//!
//! The icon is rendered by the app's own drawing code (so there is no
//! binary asset in the repository), packed into an `.ico`, and compiled with
//! the Windows resource compiler (`rc.exe` for MSVC, `windres` for GNU). If
//! no resource compiler is found the build still succeeds, without an icon.
//! On other targets nothing is done.
//!
//! ```ignore
//! // build.rs
//! #[allow(dead_code)]
//! #[path = "src/ui/appicon.rs"]
//! mod appicon;
//!
//! fn main() {
//!     rt_build::windows_resources(&rt_build::Resources {
//!         name: "rpad",
//!         description: "rpad Text Editor",
//!         icon: appicon::render,
//!         icon_source: "src/ui/appicon.rs",
//!         manifest: true,
//!     });
//! }
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

/// What goes into the executable.
pub struct Resources {
    /// Executable name without `.exe`.
    pub name: &'static str,
    /// "File description" in the file properties.
    pub description: &'static str,
    /// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
    pub icon: fn(u32) -> Vec<u8>,
    /// The file `icon` comes from, to rebuild when it changes.
    pub icon_source: &'static str,
    /// Adds a manifest: common controls v6 (themed native dialogs),
    /// per-monitor DPI awareness and long paths.
    pub manifest: bool,
}

/// Icon sizes in the `.ico`.
pub const ICON_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 128, 256];

fn png(rgba: &[u8], n: u32) -> Vec<u8> {
    // tiny-skia encodes premultiplied pixmaps; the icon data is straight
    // alpha, so build the pixmap from premultiplied values first.
    let mut pm = tiny_skia::Pixmap::new(n, n).expect("pixmap");
    for (dst, src) in pm.pixels_mut().iter_mut().zip(rgba.chunks(4)) {
        let c = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]);
        *dst = c.premultiply();
    }
    pm.encode_png().expect("png")
}

/// The `.ico` file: one PNG image per size of [`ICON_SIZES`].
pub fn ico(icon: fn(u32) -> Vec<u8>) -> Vec<u8> {
    let images: Vec<Vec<u8>> = ICON_SIZES.iter().map(|&n| png(&icon(n), n)).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(ICON_SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * ICON_SIZES.len() as u32;
    for (n, img) in ICON_SIZES.iter().zip(&images) {
        let d = if *n >= 256 { 0 } else { *n as u8 };
        ico.extend_from_slice(&[d, d, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes()); // planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        ico.extend_from_slice(&(img.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in &images {
        ico.extend_from_slice(img);
    }
    ico
}

fn find_tool(names: &[&str]) -> Option<PathBuf> {
    for n in names {
        if let Ok(p) = std::env::var(n.to_ascii_uppercase().replace(".EXE", "")) {
            if !p.is_empty() {
                return Some(p.into());
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for n in names {
            let c = dir.join(n);
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

fn manifest(name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="{name}" version="1.0.0.0"/>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </windowsSettings>
  </application>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
    </application>
  </compatibility>
</assembly>
"#
    )
}

/// Version `1.2.3-beta` as `1,2,3,0` for the VERSIONINFO block.
fn numeric_version(version: &str) -> String {
    let mut parts: Vec<String> = version.split('.').map(|p| p.split('-').next().unwrap_or("0").to_string()).collect();
    parts.resize(4, "0".into());
    parts.join(",")
}

fn rc_path(p: &Path) -> String {
    p.display().to_string().replace('\\', "\\\\")
}

/// Writes the resources and tells Cargo to link them (Windows targets only).
pub fn windows_resources(res: &Resources) {
    println!("cargo:rerun-if-changed={}", res.icon_source);
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let name = res.name;
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico_path = out.join(format!("{name}.ico"));
    std::fs::write(&ico_path, ico(res.icon)).expect("write ico");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let numeric = numeric_version(&version);
    let mut rc_text = format!(
        r#"1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "{description}"
      VALUE "ProductName", "{name}"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "{name}.exe"
      VALUE "LegalCopyright", "MIT License"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = rc_path(&ico_path),
        description = res.description,
    );
    if res.manifest {
        let path = out.join(format!("{name}.manifest"));
        std::fs::write(&path, manifest(name)).expect("write manifest");
        rc_text.push_str(&format!("1 24 \"{}\"\n", rc_path(&path)));
    }
    let rc = out.join(format!("{name}.rc"));
    std::fs::write(&rc, rc_text).expect("write rc");
    let gnu = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
    let ok = if gnu {
        let res = out.join(format!("{name}_res.o"));
        find_tool(&["windres.exe", "x86_64-w64-mingw32-windres.exe", "windres"]).is_some_and(|w| {
            let st = Command::new(w)
                .arg("-O")
                .arg("coff")
                .arg("-i")
                .arg(&rc)
                .arg("-o")
                .arg(&res)
                .status();
            st.is_ok_and(|s| s.success()) && {
                println!("cargo:rustc-link-arg-bins={}", res.display());
                true
            }
        })
    } else {
        let res = out.join(format!("{name}.res"));
        find_tool(&["rc.exe"]).is_some_and(|rcexe| {
            let st = Command::new(rcexe).arg("/nologo").arg("/fo").arg(&res).arg(&rc).status();
            st.is_ok_and(|s| s.success()) && {
                println!("cargo:rustc-link-arg-bins={}", res.display());
                true
            }
        })
    };
    if !ok {
        println!("cargo:warning=No Windows resource compiler found (rc.exe / windres); building without the executable icon.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(n: u32) -> Vec<u8> {
        vec![0x80; (n * n * 4) as usize]
    }

    #[test]
    fn ico_has_every_size() {
        let data = ico(square);
        assert_eq!(&data[..4], &[0, 0, 1, 0]);
        assert_eq!(u16::from_le_bytes([data[4], data[5]]) as usize, ICON_SIZES.len());
        // The first image starts right after the directory and is a PNG.
        let offset = u32::from_le_bytes(data[18..22].try_into().unwrap()) as usize;
        assert_eq!(offset, 6 + 16 * ICON_SIZES.len());
        assert_eq!(&data[offset + 1..offset + 4], b"PNG");
    }

    #[test]
    fn versions_and_manifest() {
        assert_eq!(numeric_version("1.2.0"), "1,2,0,0");
        assert_eq!(numeric_version("2.0.1-beta"), "2,0,1,0");
        let m = manifest("rx");
        assert!(m.contains(r#"name="rx""#) && m.contains("{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"));
    }
}
