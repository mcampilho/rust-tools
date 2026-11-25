//! A few threads doing tasks that make more tasks (a folder listed gives what
//! is in it), the newest first, so a tree is taken depth first and the list of
//! what is waiting stays short.

use crate::Control;
use std::sync::{Condvar, Mutex};

struct State<T> {
    waiting: Vec<T>,
    working: usize,
}

/// Does `first` and all they lead to with `work`, on `threads` threads (this
/// one included), until nothing is left or `ctl` says to stop.
pub(crate) fn run<T: Send>(first: Vec<T>, threads: usize, ctl: &Control, work: impl Fn(T, &mut Vec<T>) + Sync) {
    let state = Mutex::new(State { waiting: first, working: 0 });
    let changed = Condvar::new();
    let worker = || {
        let mut more = Vec::new();
        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if ctl.stopped() || (s.waiting.is_empty() && s.working == 0) {
                changed.notify_all();
                return;
            }
            let Some(task) = s.waiting.pop() else {
                s = changed.wait(s).unwrap_or_else(|e| e.into_inner());
                continue;
            };
            s.working += 1;
            drop(s);
            work(task, &mut more);
            s = state.lock().unwrap_or_else(|e| e.into_inner());
            s.working -= 1;
            let found = !more.is_empty();
            s.waiting.append(&mut more);
            if found || s.working == 0 {
                changed.notify_all();
            }
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads.max(1) {
            scope.spawn(worker);
        }
        worker();
    });
}
