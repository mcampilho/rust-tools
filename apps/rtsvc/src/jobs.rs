//! Jobs: work an elevated client asks for and the service does in the
//! background, one job of a kind at a time. A job outlives the connection
//! that asked for it; what it did is told line by line.

use rt_svc::proto::{JobInfo, JobKind, JobRequest, JobState, MAX_JOB_ITEMS};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The lines of a job kept.
const LOG_LINES: usize = 100;
/// The finished jobs remembered.
const KEEP: usize = 50;

/// What a job does with each item. The items are names the service resolves; an executor
/// never takes a path or a command from a client.
pub trait Executor: Send + Sync + 'static {
    /// Is this item one the service would act on? A job with a bad item is refused whole.
    fn check(&self, kind: JobKind, item: &str) -> Result<(), String>;
    /// Does it. Returns what to say of it. Stops early when `cancel` is set.
    fn run(&self, kind: JobKind, item: &str, cancel: &AtomicBool) -> Result<String, String>;
}

struct Job {
    info: JobInfo,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Inner {
    jobs: BTreeMap<u64, Job>,
    next: u64,
}

pub struct Jobs {
    inner: Arc<Mutex<Inner>>,
    exec: Arc<dyn Executor>,
    /// One at a time for each kind.
    locks: [Arc<Mutex<()>>; 2],
}

fn slot(k: JobKind) -> usize {
    k as usize
}

impl Jobs {
    pub fn new(exec: Arc<dyn Executor>) -> Jobs {
        Jobs { inner: Arc::default(), exec, locks: [Arc::default(), Arc::default()] }
    }

    /// Checks the items and starts the job. Returns its number.
    pub fn submit(&self, req: JobRequest) -> Result<u64, String> {
        if req.items.is_empty() || req.items.len() > MAX_JOB_ITEMS {
            return Err("a job has between 1 and 200 items".into());
        }
        for i in &req.items {
            self.exec.check(req.kind, i).map_err(|e| format!("{i}: {e}"))?;
        }
        let (id, cancel) = {
            let mut g = self.inner.lock().map_err(|_| "the service is failing")?;
            g.next += 1;
            let id = g.next;
            let cancel = Arc::new(AtomicBool::new(false));
            let info = JobInfo { id, kind: req.kind, state: JobState::Queued, done: 0, total: req.items.len() as u32, log: Vec::new() };
            g.jobs.insert(id, Job { info, cancel: cancel.clone() });
            // Old finished jobs go.
            let finished: Vec<u64> = g.jobs.iter().filter(|(_, j)| j.info.state.finished()).map(|(i, _)| *i).collect();
            if finished.len() > KEEP {
                for old in &finished[..finished.len() - KEEP] {
                    g.jobs.remove(old);
                }
            }
            (id, cancel)
        };
        let (inner, exec, lock) = (self.inner.clone(), self.exec.clone(), self.locks[slot(req.kind)].clone());
        std::thread::spawn(move || {
            let _one_at_a_time = lock.lock();
            let set = |f: &mut dyn FnMut(&mut JobInfo)| {
                if let Ok(mut g) = inner.lock() {
                    if let Some(j) = g.jobs.get_mut(&id) {
                        f(&mut j.info);
                    }
                }
            };
            if cancel.load(Ordering::SeqCst) {
                set(&mut |i| i.state = JobState::Cancelled);
                return;
            }
            set(&mut |i| i.state = JobState::Running);
            let mut failed = false;
            for item in &req.items {
                if cancel.load(Ordering::SeqCst) {
                    set(&mut |i| i.state = JobState::Cancelled);
                    return;
                }
                let line = match exec.run(req.kind, item, &cancel) {
                    Ok(m) => format!("{item}: {m}"),
                    Err(e) => {
                        failed = true;
                        format!("{item}: failed: {e}")
                    }
                };
                set(&mut |i| {
                    i.done += 1;
                    if i.log.len() < LOG_LINES {
                        i.log.push(line.clone());
                    }
                });
            }
            set(&mut |i| i.state = if failed { JobState::Failed } else { JobState::Done });
        });
        Ok(id)
    }

    pub fn get(&self, id: u64) -> Option<JobInfo> {
        self.inner.lock().ok()?.jobs.get(&id).map(|j| j.info.clone())
    }

    /// Tells a job to stop at its next item.
    pub fn cancel(&self, id: u64) -> Option<JobInfo> {
        let g = self.inner.lock().ok()?;
        let j = g.jobs.get(&id)?;
        j.cancel.store(true, Ordering::SeqCst);
        Some(j.info.clone())
    }
}

/// An executor that accepts nothing, for the tests of the handler.
#[cfg(test)]
pub mod tests_support {
    use super::*;

    pub struct Nothing;

    impl Executor for Nothing {
        fn check(&self, _: JobKind, _: &str) -> Result<(), String> {
            Err("nothing is accepted".into())
        }

        fn run(&self, _: JobKind, _: &str, _: &AtomicBool) -> Result<String, String> {
            Err("nothing is done".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Items "ok", "bad" (fails), "slow" (waits for a cancel), and "refused" (not accepted).
    struct Fake;

    impl Executor for Fake {
        fn check(&self, _: JobKind, item: &str) -> Result<(), String> {
            if item == "refused" {
                Err("not in the catalogue".into())
            } else {
                Ok(())
            }
        }

        fn run(&self, _: JobKind, item: &str, cancel: &AtomicBool) -> Result<String, String> {
            match item {
                "bad" => Err("it did not work".into()),
                "slow" => {
                    let end = Instant::now() + Duration::from_secs(5);
                    while !cancel.load(Ordering::SeqCst) && Instant::now() < end {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Ok("stopped".into())
                }
                _ => Ok("done".into()),
            }
        }
    }

    fn req(items: &[&str]) -> JobRequest {
        JobRequest { kind: JobKind::Clean, items: items.iter().map(|s| s.to_string()).collect() }
    }

    fn wait(j: &Jobs, id: u64, want: impl Fn(&JobInfo) -> bool) -> JobInfo {
        for _ in 0..500 {
            let i = j.get(id).unwrap();
            if want(&i) {
                return i;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the job did not get there: {:?}", j.get(id));
    }

    #[test]
    fn a_job_does_its_items_and_tells_what_happened() {
        let j = Jobs::new(Arc::new(Fake));
        let id = j.submit(req(&["ok", "ok"])).unwrap();
        let i = wait(&j, id, |i| i.state.finished());
        assert_eq!((i.state, i.done, i.total), (JobState::Done, 2, 2));
        assert_eq!(i.log, ["ok: done", "ok: done"]);
        let id = j.submit(req(&["ok", "bad", "ok"])).unwrap();
        let i = wait(&j, id, |i| i.state.finished());
        assert_eq!((i.state, i.done), (JobState::Failed, 3), "an item that fails does not stop the others");
        assert!(i.log[1].contains("it did not work"));
    }

    #[test]
    fn a_job_with_an_item_that_is_not_accepted_does_not_start() {
        let j = Jobs::new(Arc::new(Fake));
        assert!(j.submit(req(&["ok", "refused"])).unwrap_err().contains("not in the catalogue"));
        assert!(j.submit(req(&[])).is_err());
        let many: Vec<String> = (0..=MAX_JOB_ITEMS).map(|n| n.to_string()).collect();
        assert!(j.submit(JobRequest { kind: JobKind::Clean, items: many }).is_err());
        assert!(j.get(1).is_none(), "nothing was started");
    }

    #[test]
    fn a_job_can_be_stopped_and_the_next_waits_its_turn() {
        let j = Jobs::new(Arc::new(Fake));
        let a = j.submit(req(&["slow", "ok"])).unwrap();
        let b = j.submit(req(&["ok"])).unwrap();
        wait(&j, a, |i| i.state == JobState::Running);
        assert_eq!(j.get(b).unwrap().state, JobState::Queued, "one at a time for a kind");
        j.cancel(a).unwrap();
        let i = wait(&j, a, |i| i.state.finished());
        assert_eq!((i.state, i.done), (JobState::Cancelled, 1), "stopped before the second item");
        assert_eq!(wait(&j, b, |i| i.state.finished()).state, JobState::Done);
        assert!(j.cancel(999).is_none() && j.get(999).is_none());
    }
}
