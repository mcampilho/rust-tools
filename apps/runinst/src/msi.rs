//! The products of Windows Installer, from its own records
//! (`MsiEnumProductsEx`): what WMI's `Win32_Product` lists, read without
//! the side effect of that class (it has Windows Installer check, and at
//! times repair, every product it lists).

use std::path::PathBuf;

/// A product, and for whom (None: every user; Some: a user's SID).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Product {
    /// `{GUID}`, in capitals.
    pub code: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub location: Option<PathBuf>,
    pub date: Option<(i32, u32, u32)>,
    pub user: Option<String>,
}

#[cfg(windows)]
pub fn products(all_users: bool) -> Vec<Product> {
    use rt_sys::win::{from_wide, wide};
    use windows_sys::Win32::System::ApplicationInstallationAndServicing::{
        MsiEnumProductsExW, MsiGetProductInfoExW, MSIINSTALLCONTEXT, MSIINSTALLCONTEXT_ALL, MSIINSTALLCONTEXT_MACHINE,
    };
    // Every user's (administrators), or this user's and the machine's.
    let everyone = wide("s-1-1-0");
    let sid_filter = if all_users { everyone.as_ptr() } else { std::ptr::null() };
    let mut out = Vec::new();
    for index in 0.. {
        let mut code = [0u16; 39];
        let mut context: MSIINSTALLCONTEXT = 0;
        let mut sid = vec![0u16; 256];
        let mut sid_len = sid.len() as u32;
        // SAFETY: the buffers are as large as stated (a GUID is 38 characters).
        let rc = unsafe {
            MsiEnumProductsExW(std::ptr::null(), sid_filter, MSIINSTALLCONTEXT_ALL as u32, index, code.as_mut_ptr(), &mut context, sid.as_mut_ptr(), &mut sid_len)
        };
        if rc != 0 {
            break;
        }
        let code_s = from_wide(&code);
        let user = (context != MSIINSTALLCONTEXT_MACHINE).then(|| from_wide(&sid)).filter(|s| !s.is_empty());
        let user_w = user.as_deref().map(wide);
        let info = |prop: &str| -> String {
            let p = wide(prop);
            let c = wide(&code_s);
            let mut buf = vec![0u16; 1024];
            let mut len = buf.len() as u32;
            let u = user_w.as_ref().map_or(std::ptr::null(), |w| w.as_ptr());
            // SAFETY: NUL-terminated inputs; the buffer holds `len` characters.
            let rc = unsafe { MsiGetProductInfoExW(c.as_ptr(), u, context, p.as_ptr(), buf.as_mut_ptr(), &mut len) };
            if rc == 0 {
                from_wide(&buf).trim().to_string()
            } else {
                String::new()
            }
        };
        let name = info("ProductName");
        if name.is_empty() {
            continue;
        }
        let location = info("InstallLocation");
        out.push(Product {
            code: code_s.to_uppercase(),
            version: info("VersionString"),
            publisher: info("Publisher"),
            location: (!location.is_empty()).then(|| PathBuf::from(location)),
            date: crate::inventory::parse_date(&info("InstallDate")),
            user,
            name,
        });
    }
    out
}

#[cfg(not(windows))]
pub fn products(_all_users: bool) -> Vec<Product> {
    Vec::new()
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn reads_this_computer() {
        // Most Windows have some MSI product (runtimes, drivers); none is fine too.
        for p in super::products(false) {
            assert!(p.code.starts_with('{') && p.code.ends_with('}') && p.code.len() == 38, "{p:?}");
            assert!(!p.name.is_empty());
        }
    }
}
