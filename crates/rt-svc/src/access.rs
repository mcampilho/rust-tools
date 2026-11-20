//! Windows tokens: what the service makes of a client, and who owns the
//! other end of a pipe.

use crate::proto::{decide, Level, TokenFacts};
use std::io;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, LookupAccountSidW, RevertToSelf, TokenElevation, TokenGroups, TokenUser, OWNER_SECURITY_INFORMATION,
    TOKEN_ELEVATION, TOKEN_GROUPS, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

/// SIDs in the text form, to compare without building them.
const SE_GROUP_ENABLED: u32 = 0x4;
const SE_GROUP_USE_FOR_DENY_ONLY: u32 = 0x10;

pub const SID_SYSTEM: &str = "S-1-5-18";
pub const SID_ADMINISTRATORS: &str = "S-1-5-32-544";
pub const SID_ANONYMOUS: &str = "S-1-5-7";
pub const SID_TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

fn last_error() -> io::Error {
    // SAFETY: no arguments.
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

/// A token handle closed on drop.
pub struct Token(pub HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle is ours and closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// The text form of a SID.
///
/// # Safety
/// `sid` must point to a valid SID.
pub unsafe fn sid_text(sid: *mut core::ffi::c_void) -> Option<String> {
    let mut s: *mut u16 = null_mut();
    if ConvertSidToStringSidW(sid, &mut s) == 0 {
        return None;
    }
    let mut n = 0;
    while *s.add(n) != 0 {
        n += 1;
    }
    let text = String::from_utf16_lossy(std::slice::from_raw_parts(s, n));
    LocalFree(s as *mut _);
    Some(text)
}

/// The name of the account of a SID, without the domain.
///
/// # Safety
/// `sid` must point to a valid SID.
pub unsafe fn account_name(sid: *mut core::ffi::c_void) -> Option<String> {
    let (mut name, mut domain) = ([0u16; 256], [0u16; 256]);
    let (mut n, mut d, mut kind) = (name.len() as u32, domain.len() as u32, 0);
    if LookupAccountSidW(std::ptr::null(), sid, name.as_mut_ptr(), &mut n, domain.as_mut_ptr(), &mut d, &mut kind) == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&name[..n as usize]))
}

/// A buffer of `GetTokenInformation` for one class.
fn token_info(token: HANDLE, class: i32) -> io::Result<Vec<u64>> {
    let mut len = 0u32;
    // SAFETY: asking for the size with a null buffer.
    unsafe { GetTokenInformation(token, class, null_mut(), 0, &mut len) };
    if len == 0 {
        return Err(last_error());
    }
    // u64 elements: the structures inside need 8-byte alignment.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: the buffer has `len` bytes at least.
    if unsafe { GetTokenInformation(token, class, buf.as_mut_ptr() as *mut _, len, &mut len) } == 0 {
        return Err(last_error());
    }
    Ok(buf)
}

/// What a token says about its owner.
#[derive(Clone, Debug)]
pub struct Who {
    pub facts: TokenFacts,
    /// The user's SID in text, for the log.
    pub sid: String,
    /// The account name, without the domain (empty if it cannot be found).
    pub name: String,
    pub level: Level,
}

/// The facts of `token` (an impersonation token or a primary one).
pub fn read_token(token: HANDLE) -> io::Result<Who> {
    let user = token_info(token, TokenUser)?;
    // SAFETY: the buffer holds a TOKEN_USER filled by the system.
    let sid_ptr = unsafe { (*(user.as_ptr() as *const TOKEN_USER)).User.Sid };
    // SAFETY: the SID belongs to the buffer, alive here.
    let sid = unsafe { sid_text(sid_ptr) }.unwrap_or_default();

    // SAFETY: the SID belongs to the buffer, alive here.
    let name = unsafe { account_name(sid_ptr) }.unwrap_or_default();

    let elev = token_info(token, TokenElevation)?;
    // SAFETY: the buffer holds a TOKEN_ELEVATION.
    let elevated = unsafe { (*(elev.as_ptr() as *const TOKEN_ELEVATION)).TokenIsElevated } != 0;

    let groups = token_info(token, TokenGroups)?;
    // SAFETY: the buffer holds a TOKEN_GROUPS and its array, filled by the system.
    let admin = unsafe {
        let g = &*(groups.as_ptr() as *const TOKEN_GROUPS);
        std::slice::from_raw_parts(g.Groups.as_ptr(), g.GroupCount as usize).iter().any(|a| {
            // Enabled, and not only there to deny.
            a.Attributes & SE_GROUP_ENABLED != 0
                && a.Attributes & SE_GROUP_USE_FOR_DENY_ONLY == 0
                && sid_text(a.Sid).as_deref() == Some(SID_ADMINISTRATORS)
        })
    };
    // Anonymous, or a guest (the relative id 501 of the machine's or domain's guest account).
    let anonymous = sid == SID_ANONYMOUS;
    let guest = sid.starts_with("S-1-5-21-") && sid.ends_with("-501");
    let facts = TokenFacts { anonymous, guest, elevated, admin };
    Ok(Who { facts, sid, name, level: decide(facts) })
}

/// The token of the process that is running this code.
pub fn current_process_token() -> io::Result<Token> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut t: HANDLE = null_mut();
    // SAFETY: valid pseudo-handle and out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut t) } == 0 {
        return Err(last_error());
    }
    Ok(Token(t))
}

/// Is this process elevated (an administrator with the full token)?
pub fn is_elevated() -> bool {
    current_process_token().and_then(|t| read_token(t.0)).is_ok_and(|w| w.level == Level::Elevated)
}

/// Who is on the other end of `pipe` (a pipe handle the service accepted).
/// The client is impersonated only long enough to read its token.
pub fn client_of(conn: &crate::pipe::Conn) -> io::Result<Who> {
    let pipe = conn.handle();
    // SAFETY: `pipe` is a connected server end of a named pipe.
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return Err(last_error());
    }
    let mut t: HANDLE = null_mut();
    // SAFETY: the current thread is impersonating; open its token (as the impersonated one).
    let opened = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut t) };
    let err = if opened == 0 { Some(last_error()) } else { None };
    // Always go back to the service's own identity first.
    // SAFETY: no arguments.
    unsafe { RevertToSelf() };
    if let Some(e) = err {
        return Err(e);
    }
    let token = Token(t);
    read_token(token.0)
}

/// The text of the SID of the owner of the object behind `handle`.
pub fn owner_of(conn: &crate::pipe::Conn) -> io::Result<String> {
    let handle = conn.handle();
    let mut owner: *mut core::ffi::c_void = null_mut();
    let mut sd: *mut core::ffi::c_void = null_mut();
    // SAFETY: `handle` is a valid kernel object handle; the out pointers are valid.
    let r = unsafe {
        GetSecurityInfo(handle, SE_KERNEL_OBJECT, OWNER_SECURITY_INFORMATION, &mut owner, null_mut(), null_mut(), null_mut(), &mut sd)
    };
    if r != 0 {
        return Err(io::Error::from_raw_os_error(r as i32));
    }
    // SAFETY: `owner` points into the descriptor `sd`, alive until LocalFree.
    let text = unsafe { sid_text(owner) };
    // SAFETY: allocated by GetSecurityInfo.
    unsafe { LocalFree(sd as *mut _) };
    text.ok_or_else(|| io::Error::other("the owner has no SID"))
}

/// Equal SIDs, for the callers that hold pointers.
///
/// # Safety
/// Both must point to valid SIDs.
pub unsafe fn same_sid(a: *mut core::ffi::c_void, b: *mut core::ffi::c_void) -> bool {
    EqualSid(a, b) != 0
}
