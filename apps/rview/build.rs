//! Embeds the application icon and version information into the Windows
//! executable. The icon is rendered from `src/ui/appicon.rs` (so there is no
//! binary asset in the repository), packed into an `.ico`, and compiled with
//! the Windows resource compiler (`rc.exe` for MSVC, `windres` for GNU).
//! If no resource compiler is found the build still succeeds, without an icon.

#[allow(dead_code)]
#[path = "src/ui/appicon.rs"]
mod appicon;

use std::path::{Path, PathBuf};
use std::process::Command;

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

fn write_ico(path: &Path) {
    let sizes = [16u32, 20, 24, 32, 40, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = sizes.iter().map(|&n| png(&appicon::render(n), n)).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (n, img) in sizes.iter().zip(&images) {
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
    std::fs::write(path, ico).expect("write ico");
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

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="rview" version="1.0.0.0"/>
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
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=src/ui/appicon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = out.join("rview.ico");
    write_ico(&ico);
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<String> = version.split('.').map(|p| p.split('-').next().unwrap_or("0").to_string()).collect();
    parts.resize(4, "0".into());
    let numeric = parts.join(",");
    let rc = out.join("rview.rc");
    let rc_text = format!(
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
      VALUE "FileDescription", "rview Document Viewer"
      VALUE "ProductName", "rview"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "rview.exe"
      VALUE "LegalCopyright", "MIT License"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = ico.display().to_string().replace('\\', "\\\\"),
    );
    // Manifest: common controls v6 (themed native dialogs), per-monitor
    // DPI awareness and long paths.
    let manifest = out.join("rview.manifest");
    std::fs::write(&manifest, MANIFEST).expect("write manifest");
    let rc_text = format!("{rc_text}1 24 \"{}\"\n", manifest.display().to_string().replace('\\', "\\\\"));
    std::fs::write(&rc, rc_text).expect("write rc");
    let gnu = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
    let ok = if gnu {
        let res = out.join("rview_res.o");
        find_tool(&["windres.exe", "x86_64-w64-mingw32-windres.exe", "windres"]).is_some_and(|w| {
            let st = Command::new(w)
                .arg("-O")
                .arg("coff")
                .arg("-i")
                .arg(&rc)
                .arg("-o")
                .arg(&res)
                .status();
            if st.is_ok_and(|s| s.success()) {
                println!("cargo:rustc-link-arg-bins={}", res.display());
                true
            } else {
                false
            }
        })
    } else {
        let res = out.join("rview.res");
        find_tool(&["rc.exe"]).is_some_and(|rcexe| {
            let st = Command::new(rcexe).arg("/nologo").arg("/fo").arg(&res).arg(&rc).status();
            if st.is_ok_and(|s| s.success()) {
                println!("cargo:rustc-link-arg-bins={}", res.display());
                true
            } else {
                false
            }
        })
    };
    if !ok {
        println!("cargo:warning=No Windows resource compiler found (rc.exe / windres); building without the executable icon.");
    }
}
