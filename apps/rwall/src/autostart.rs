//! Starting rwall with the session ("Start with Windows"): `rwall --tray`,
//! through `rt_sys::autostart` (shared by every app).

/// Whether this rwall starts with the session.
pub fn enabled() -> bool {
    rt_sys::autostart::enabled("rwall")
}

/// Turns starting with the session on or off.
pub fn set(on: bool) -> Result<(), String> {
    rt_sys::autostart::set("rwall", on)
}
