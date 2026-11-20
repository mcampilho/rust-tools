//! When is the computer idle enough for the service to work in the background?
//! A service is in session 0 and cannot see the keyboard of the user, so idle
//! means: on mains power (or unknown) and the processor busy for less than
//! a third of the time since the last look.

use std::sync::Mutex;
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::Threading::{GetCurrentThread, GetSystemTimes, SetThreadPriority};

/// `THREAD_MODE_BACKGROUND_BEGIN`: low processor and low disk priority for this thread.
const BACKGROUND_BEGIN: i32 = 0x0001_0000;

fn ticks(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

/// The share of the processor busy between two looks, in percent.
pub fn busy_percent(before: (u64, u64), after: (u64, u64)) -> u64 {
    let (idle, total) = (after.0.saturating_sub(before.0), after.1.saturating_sub(before.1));
    if total == 0 {
        return 100;
    }
    100 - (idle * 100 / total).min(100)
}

/// Whether the busy share and the power say "idle".
pub fn is_idle(busy: u64, on_mains: bool) -> bool {
    on_mains && busy < 33
}

fn times() -> Option<(u64, u64)> {
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut idle, mut kernel, mut user) = (zero, zero, zero);
    // SAFETY: valid out pointers.
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    // Kernel time includes the idle time.
    Some((ticks(idle), ticks(kernel) + ticks(user)))
}

fn on_mains() -> bool {
    let mut st = SYSTEM_POWER_STATUS {
        ACLineStatus: 0,
        BatteryFlag: 0,
        BatteryLifePercent: 0,
        SystemStatusFlag: 0,
        BatteryLifeTime: 0,
        BatteryFullLifeTime: 0,
    };
    // SAFETY: valid out pointer.
    if unsafe { GetSystemPowerStatus(&mut st) } == 0 {
        return true;
    }
    // 0 on battery, 1 on mains, 255 unknown.
    st.ACLineStatus != 0
}

/// Remembers the last look at the processor.
#[derive(Default)]
pub struct Idle {
    last: Mutex<Option<(u64, u64)>>,
}

impl Idle {
    /// Is the computer idle since the last call? The first call has nothing to compare with: no.
    pub fn check(&self) -> bool {
        let Some(now) = times() else { return false };
        let Ok(mut last) = self.last.lock() else { return false };
        let before = last.replace(now);
        before.is_some_and(|b| is_idle(busy_percent(b, now), on_mains()))
    }
}

/// Makes the calling thread work at low priority (processor and disk) until it ends.
pub fn work_in_background() {
    // SAFETY: the pseudo-handle of the current thread.
    unsafe { SetThreadPriority(GetCurrentThread(), BACKGROUND_BEGIN) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_busy_share_is_worked_out_from_two_looks() {
        assert_eq!(busy_percent((0, 0), (900, 1000)), 10);
        assert_eq!(busy_percent((100, 200), (100, 300)), 100);
        assert_eq!(busy_percent((5, 5), (5, 5)), 100, "no time passed: not idle");
        assert!(is_idle(10, true) && !is_idle(10, false) && !is_idle(60, true));
    }

    #[test]
    fn the_first_look_is_never_idle() {
        let i = Idle::default();
        assert!(!i.check());
        // The second look compares; either answer is fine, it must not fail.
        let _ = i.check();
    }
}
