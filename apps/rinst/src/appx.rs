//! Store and MSIX packages (ChatGPT, Windows Terminal, the inbox apps), from
//! the package manager of Windows (`Get-AppxPackage`): those of this user,
//! or, with administrator rights, of every user. They have no `Uninstall`
//! entry. Resources and bundles are left out; frameworks, the packages
//! signed as part of Windows, and those without an app to start are marked
//! as system components.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub full_name: String,
    pub display: String,
    pub publisher: String,
    pub version: String,
    pub location: Option<PathBuf>,
    pub date: Option<(i32, u32, u32)>,
    pub system: bool,
    /// The app to start (`Family!App`): its icon comes from the shell.
    pub app_id: Option<String>,
    /// The SIDs of the users who have it.
    pub users: Vec<String>,
}

#[cfg(windows)]
pub fn packages(all_users: bool, me: &str) -> Vec<Package> {
    use windows::core::HSTRING;
    use windows::ApplicationModel::PackageSignatureKind;
    use windows::Management::Deployment::{PackageManager, PackageTypes};
    // SAFETY: initialises WinRT on this thread (a worker of its own).
    let _ = unsafe { windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED) };
    let Ok(pm) = PackageManager::new() else { return Vec::new() };
    // The apps, and the frameworks they use (shown as system components).
    let types = PackageTypes(PackageTypes::Main.0 | PackageTypes::Framework.0);
    let found = if all_users { pm.FindPackagesWithPackageTypes(types) } else { pm.FindPackagesByUserSecurityIdWithPackageTypes(&HSTRING::new(), types) };
    let Ok(found) = found else { return Vec::new() };
    let mut out = Vec::new();
    for p in found {
        let Ok(id) = p.Id() else { continue };
        let full_name = id.FullName().map(|s| s.to_string()).unwrap_or_default();
        let name = id.Name().map(|s| s.to_string()).unwrap_or_default();
        let display = p.DisplayName().map(|s| s.to_string()).ok().filter(|s| !s.is_empty() && !s.starts_with("ms-resource:")).unwrap_or(name);
        let version = id.Version().map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision)).unwrap_or_default();
        let app_id = p.GetAppListEntries().ok().and_then(|list| list.into_iter().next()).and_then(|e| e.AppUserModelId().ok()).map(|s| s.to_string());
        let system = p.SignatureKind().is_ok_and(|k| k == PackageSignatureKind::System) || p.IsFramework().unwrap_or(false) || app_id.is_none();
        // 100 ns since 1601 → the day.
        let date = p.InstalledDate().ok().map(|d| d.UniversalTime / 10_000_000 - 11_644_473_600).filter(|&s| s > 0).map(|s| rt_scan::format::civil((s / 86_400) as i32));
        let users = if all_users {
            // Only those who have it installed (not merely staged for them).
            pm.FindUsers(&HSTRING::from(full_name.as_str()))
                .map(|it| it.into_iter().filter(|u| u.InstallState().is_ok_and(|s| s == windows::Management::Deployment::PackageInstallState::Installed)).filter_map(|u| u.UserSecurityId().ok().map(|s| s.to_string())).collect())
                .unwrap_or_default()
        } else {
            vec![me.to_string()]
        };
        out.push(Package {
            full_name,
            display,
            publisher: p.PublisherDisplayName().map(|s| s.to_string()).unwrap_or_default(),
            version,
            location: p.InstalledPath().ok().map(|s| PathBuf::from(s.to_string())).filter(|p| !p.as_os_str().is_empty()),
            date,
            system,
            app_id,
            users,
        });
    }
    out
}

#[cfg(not(windows))]
pub fn packages(_all_users: bool, _me: &str) -> Vec<Package> {
    Vec::new()
}

/// The icon of an app (`Family!App`), as the Start menu shows it:
/// (width, height, straight RGBA).
#[cfg(windows)]
pub fn icon(app_id: &str, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};
    let name = HSTRING::from(format!("shell:AppsFolder\\{app_id}"));
    // SAFETY: COM and GDI calls on objects made here and released before
    // returning; the buffer is as large as the bitmap.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).ok()?;
        let bmp = factory.GetImage(SIZE { cx: px as i32, cy: px as i32 }, SIIGBF_ICONONLY).ok()?;
        let mut info = BITMAP::default();
        let got = GetObjectW(HGDIOBJ(bmp.0), size_of::<BITMAP>() as i32, Some((&mut info as *mut BITMAP).cast()));
        let (w, h) = (info.bmWidth, info.bmHeight.abs());
        let result = if got > 0 && w > 0 && h > 0 {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
                ..Default::default()
            };
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let dc = GetDC(None);
            let rows = GetDIBits(dc, bmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS);
            ReleaseDC(None, dc);
            (rows > 0).then(|| {
                // BGRA, premultiplied → straight RGBA.
                for p in buf.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                    let a = p[3] as u32;
                    if a > 0 && a < 255 {
                        for c in &mut p[..3] {
                            *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                        }
                    }
                }
                (w as u32, h as u32, buf)
            })
        } else {
            None
        };
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        result
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn reads_this_users_packages() {
        let me = rt_sys::win::user_sid().unwrap_or_default();
        let all = super::packages(false, &me);
        // Every Windows 10/11 has apps of its own (Settings, Calculator, Store...).
        assert!(!all.is_empty());
        assert!(all.iter().all(|p| !p.full_name.is_empty() && !p.display.is_empty()));
        if let Some(app) = all.iter().find_map(|p| p.app_id.clone()) {
            let (w, h, px) = super::icon(&app, 32).expect("an app's icon");
            assert!(w > 0 && h > 0 && px.len() == (w * h * 4) as usize);
        }
    }
}
