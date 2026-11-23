//! Work off the window's thread: reading the registry, the services or the
//! disk can take seconds when Windows is busy (SFC or DISM running), and the
//! window must keep answering. A [`Job`] runs a function in a thread; the
//! page takes its result when it comes (`take`, from its `animate`).

use std::sync::mpsc::{channel, Receiver, TryRecvError};

pub struct Job<T> {
    recv: Option<Receiver<T>>,
}

impl<T> Default for Job<T> {
    fn default() -> Self {
        Job { recv: None }
    }
}

impl<T: Send + 'static> Job<T> {
    /// Starts `f` in a thread; false (and nothing started) while the last one runs.
    pub fn start(&mut self, f: impl FnOnce() -> T + Send + 'static) -> bool {
        if self.busy() {
            return false;
        }
        let (send, recv) = channel();
        std::thread::spawn(move || {
            let _ = send.send(f());
        });
        self.recv = Some(recv);
        true
    }

    pub fn busy(&self) -> bool {
        self.recv.is_some()
    }

    /// The result, once it is there (the thread ended without one: None, and not busy any more).
    pub fn take(&mut self) -> Option<T> {
        let r = self.recv.as_ref()?;
        match r.try_recv() {
            Ok(v) => {
                self.recv = None;
                Some(v)
            }
            Err(TryRecvError::Disconnected) => {
                self.recv = None;
                None
            }
            Err(TryRecvError::Empty) => None,
        }
    }

    /// Waits for the result (tests only: the window never waits).
    #[cfg(test)]
    pub fn wait(&mut self) -> Option<T> {
        let r = self.recv.take()?;
        r.recv().ok()
    }
}

/// When a page with work under way should be looked at again.
pub fn soon() -> std::time::Instant {
    std::time::Instant::now() + std::time::Duration::from_millis(120)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_at_a_time() {
        let mut j: Job<u32> = Job::default();
        assert!(!j.busy() && j.take().is_none());
        let (go, wait) = channel::<()>();
        assert!(j.start(move || {
            let _ = wait.recv();
            7
        }));
        assert!(j.busy());
        assert!(!j.start(|| 8), "the first is still running");
        assert_eq!(j.take(), None);
        go.send(()).unwrap();
        assert_eq!(j.wait(), Some(7));
        assert!(!j.busy());
        // A thread that panics leaves nothing, and the job is free again.
        j.start(|| panic!("no"));
        let t = std::time::Instant::now();
        while j.busy() && t.elapsed().as_secs() < 5 {
            let _ = j.take();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!j.busy());
    }
}
