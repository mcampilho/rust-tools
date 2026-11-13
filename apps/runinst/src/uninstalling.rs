//! Uninstalling the programs selected: asking first, a queue on a thread
//! of its own (one uninstaller at a time, each waited for with what it
//! starts), the line above the list while it runs, reading the programs
//! again, and a report of what is gone and what is still there.

use super::*;
use runinst::app::Mark;
use runinst::leftovers;
use runinst::uninstall::{self, How, Outcome, Plan};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// What the queue's thread says.
pub(super) enum JobMsg {
    /// It starts the `i`th (0-based).
    Start(usize),
    Done(usize, Outcome),
    Finished,
}

/// The queue under way.
pub(super) struct Job {
    pub stop: Arc<AtomicBool>,
    /// Each program: name, where it was found (to see whether it is still
    /// there), and how it went.
    pub items: Vec<(String, String, Option<Outcome>)>,
    /// The programs, in the same order (to look for their leftovers).
    pub programs: Vec<runinst::inventory::Program>,
    pub modify: bool,
}

/// A reason a plan cannot be carried out, as shown.
fn reason_text(why: &str) -> String {
    match why {
        uninstall::OTHER_USER => tr("it was installed for another user: uninstall it from their account"),
        uninstall::NO_UNINSTALLER => tr("it has no uninstall command"),
        uninstall::NO_REMOVE => tr("it says it cannot be removed"),
        uninstall::NO_MODIFY => tr("it has no command to change or repair it"),
        uninstall::NEEDS_ROOT => tr("it needs root, and pkexec was not found: start runinst as root"),
        uninstall::NOT_ELEVATED_PACKAGE => tr("Store apps are removed for the user runinst runs as: start runinst without administrator rights"),
        other => other.to_string(),
    }
}

fn outcome_text(o: &Outcome) -> String {
    match o {
        Outcome::Ended(code, tail) if tail.is_empty() => tf("it ended with code {}", &[&code.to_string()]),
        Outcome::Ended(code, tail) => format!("{}\n      {}", tf("it ended with code {}", &[&code.to_string()]), tail.replace('\n', "\n      ")),
        Outcome::Failed(e) => e.clone(),
        Outcome::Cannot(why) => reason_text(why),
    }
}

impl Shell {
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Asks, then uninstalls (or changes) the programs selected.
    pub(super) fn uninstall(&mut self, how: How) {
        let Some(found) = &self.app.found else { return };
        let me = &found.inv.me;
        let mut chosen = self.app.chosen_programs();
        if how == How::Modify {
            chosen.truncate(1);
        }
        let plans: Vec<(String, String, Plan)> = chosen.iter().map(|p| (p.name.clone(), p.key.clone(), uninstall::plan(p, how, self.admin, me))).collect();
        if plans.is_empty() {
            return;
        }
        let progs: Vec<runinst::inventory::Program> = chosen.iter().map(|p| (*p).clone()).collect();
        let (can, cannot): (Vec<_>, Vec<_>) = plans.into_iter().zip(progs).partition(|((_, _, p), _)| !matches!(p, Plan::Cannot(_)));
        let programs: Vec<runinst::inventory::Program> = can.iter().chain(cannot.iter()).map(|(_, p)| p.clone()).collect();
        let can: Vec<(String, String, Plan)> = can.into_iter().map(|(x, _)| x).collect();
        let cannot: Vec<(String, String, Plan)> = cannot.into_iter().map(|(x, _)| x).collect();
        let cannot_text: Vec<String> = cannot
            .iter()
            .map(|(n, _, p)| match p {
                Plan::Cannot(why) => format!("  {n}: {}", reason_text(why)),
                _ => String::new(),
            })
            .collect();
        if can.is_empty() {
            let head = if how == How::Modify { tr("This program cannot be changed from here:") } else { tr("None of these programs can be uninstalled from here:") };
            self.info(&tr("Uninstall"), &format!("{head}\n\n{}", cannot_text.join("\n")));
            return;
        }
        if how != How::Modify {
            let mut text = tr("These programs will be uninstalled, one after another:");
            text += "\n\n";
            text += &can.iter().map(|(n, _, _)| format!("  {n}")).collect::<Vec<_>>().join("\n");
            if how == How::Quiet {
                text += "\n\n";
                text += &tr("Where their uninstallers allow it, without their questions.");
            }
            if !cannot_text.is_empty() {
                text += "\n\n";
                text += &tr("These cannot be uninstalled from here:");
                text += "\n";
                text += &cannot_text.join("\n");
            }
            if !self.confirm(&tr("Uninstall"), &text) {
                return;
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let mut items: Vec<(String, String, Option<Outcome>)> = can.iter().map(|(n, k, _)| (n.clone(), k.clone(), None)).collect();
        items.extend(cannot.iter().map(|(n, k, p)| {
            let why = match p {
                Plan::Cannot(w) => w.clone(),
                _ => String::new(),
            };
            (n.clone(), k.clone(), Some(Outcome::Cannot(why)))
        }));
        let queue = can.iter().map(|(n, k, _)| (k.clone(), n.clone(), Mark::Waiting)).collect();
        self.app.busy = Some(runinst::app::Busy {
            items: queue,
            phase: 0.0,
            name: can[0].0.clone(),
            index: 1,
            total: can.len(),
            stopping: false,
            modify: how == How::Modify,
        });
        self.job_started = Some(std::time::Instant::now());
        self.job = Some(Job { stop: stop.clone(), items, programs, modify: how == How::Modify });
        let proxy = self.proxy.clone();
        let plans: Vec<Plan> = can.into_iter().map(|(_, _, p)| p).collect();
        std::thread::spawn(move || {
            for (i, plan) in plans.iter().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let _ = proxy.send_event(UserEvent::Job(JobMsg::Start(i)));
                let out = uninstall::carry_out(plan);
                let _ = proxy.send_event(UserEvent::Job(JobMsg::Done(i, out)));
            }
            let _ = proxy.send_event(UserEvent::Job(JobMsg::Finished));
        });
        self.menu_dirty = true;
        self.redraw();
        self.update_pointer();
    }

    /// "Stop after this one".
    pub(super) fn stop_job(&mut self) {
        if let (Some(job), Some(b)) = (&self.job, &mut self.app.busy) {
            job.stop.store(true, Ordering::Relaxed);
            b.stopping = true;
            self.redraw();
        }
    }

    pub(super) fn job_event(&mut self, msg: JobMsg) {
        let Some(job) = &mut self.job else { return };
        match msg {
            JobMsg::Start(i) => {
                if let (Some((name, _, _)), Some(b)) = (job.items.get(i), &mut self.app.busy) {
                    b.name = name.clone();
                    b.index = i + 1;
                    if let Some(it) = b.items.get_mut(i) {
                        it.2 = Mark::Running;
                    }
                }
            }
            JobMsg::Done(i, out) => {
                if let Some(b) = &mut self.app.busy {
                    if let Some(it) = b.items.get_mut(i) {
                        it.2 = if out.ok() { Mark::Done } else { Mark::Failed };
                    }
                }
                if let Some(item) = job.items.get_mut(i) {
                    item.2 = Some(out);
                }
            }
            JobMsg::Finished => {
                self.app.busy = None;
                self.job_started = None;
                // The report waits for the list read again.
                self.report = self.job.take();
                self.refresh();
            }
        }
        self.menu_dirty = true;
        self.redraw();
        self.update_pointer();
    }

    /// The pointer's shape follows whether it is working.
    fn update_pointer(&mut self) {
        if let Some((x, y)) = self.cursor {
            self.pointer(x, y);
        }
    }

    /// After reading again: what is gone, what is still there.
    pub(super) fn show_report(&mut self) {
        let Some(job) = self.report.take() else { return };
        let Some(found) = &self.app.found else { return };
        // A dpkg package whose settings are all that is left is gone.
        let still = |key: &str, name: &str| {
            runinst::app::Tab::LISTS
                .iter()
                .any(|&t| found.list(t).iter().any(|p| p.key == key && p.name == name && !p.problems.contains(&runinst::inventory::Problem::ConfigOnly)))
        };
        let (mut gone, mut there, mut failed, mut skipped) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut restart = false;
        let mut removed = Vec::new();
        for ((name, key, out), prog) in job.items.iter().zip(&job.programs) {
            match out {
                None => skipped.push(format!("  {name}")),
                Some(o @ Outcome::Cannot(_)) => failed.push(format!("  {name}: {}", outcome_text(o))),
                Some(o) if o.ok() => {
                    restart |= o.needs_restart();
                    if job.modify {
                        gone.push(format!("  {name}"));
                    } else if still(key, name) {
                        there.push(format!("  {name}"));
                    } else {
                        gone.push(format!("  {name}"));
                        removed.push(prog.clone());
                    }
                }
                Some(o) => failed.push(format!("  {name}: {}", outcome_text(o))),
            }
        }
        // A change that went well needs no report.
        if job.modify && failed.is_empty() {
            return;
        }
        let mut parts = Vec::new();
        for (head, list) in [
            (if job.modify { "Changed ({}):" } else { "Uninstalled ({}):" }, &gone),
            ("Still installed: the uninstaller may have been cancelled ({}):", &there),
            ("Did not work ({}):", &failed),
            ("Not started ({}):", &skipped),
        ] {
            if !list.is_empty() {
                parts.push(format!("{}\n{}", tf(head, &[&list.len().to_string()]), list.join("\n")));
            }
        }
        if restart {
            parts.push(tr("Restart Windows to finish."));
        }
        if !removed.is_empty() {
            parts.push(tr("runinst is now looking for what they left behind: see the Leftovers tab."));
            self.look_for_leftovers(removed);
        }
        self.info(&tr("Uninstall"), &parts.join("\n\n"));
    }

    /// Looks, on a thread, for what the programs uninstalled left behind.
    fn look_for_leftovers(&mut self, gone: Vec<runinst::inventory::Program>) {
        self.search_leftovers(Some(gone));
    }

    /// "Find Leftovers": what programs removed before left, without
    /// uninstalling; its tab is shown at once.
    pub(super) fn look_for_orphans(&mut self) {
        self.search_leftovers(None);
    }

    /// Looks on a thread: for the leftovers of `gone`, or (None) of programs
    /// removed before.
    fn search_leftovers(&mut self, gone: Option<Vec<runinst::inventory::Program>>) {
        let Some(found) = &self.app.found else { return };
        let me = found.inv.me.clone();
        let installed: Vec<runinst::inventory::Program> =
            runinst::app::Tab::LISTS.iter().flat_map(|&t| found.list(t).iter().cloned()).collect();
        let shortcuts = found.shortcuts.clone();
        let admin = self.admin;
        let proxy = self.proxy.clone();
        let asked = gone.is_none();
        self.app.set_leftovers(Some(runinst::app::Leftovers { searching: true, ..Default::default() }), asked);
        std::thread::spawn(move || {
            let stop = AtomicBool::new(false);
            let cx = leftovers::Context { me: &me, admin, installed: &installed, shortcuts: &shortcuts };
            let items = match &gone {
                Some(g) => leftovers::find(g, &cx, &stop),
                None => leftovers::find_orphans(&cx, &stop),
            };
            let _ = proxy.send_event(UserEvent::Leftovers(items));
        });
        self.menu_dirty = true;
        self.redraw();
    }

    pub(super) fn leftovers_found(&mut self, items: Vec<leftovers::Leftover>) {
        if items.is_empty() {
            // Asked for and looked at: say so.
            let watching = self.app.tab == runinst::app::Tab::Leftovers;
            self.app.set_leftovers(None, false);
            self.app.note = Some(tr("Nothing was left behind."));
            if watching {
                self.info(&tr("Find Leftovers"), &tr("Nothing was left behind."));
            }
        } else {
            let keep_tab = self.app.tab;
            self.app.leftovers = Some(runinst::app::Leftovers::new(items));
            self.app.tab = keep_tab;
            self.app.rebuild();
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// The folder of the leftover selected (a file's: the one holding it).
    pub(super) fn leftover_folder(&self) -> Option<std::path::PathBuf> {
        let l = self.app.selected_index().and_then(|i| self.app.leftover(i))?;
        let p = std::path::PathBuf::from(&l.path);
        match l.kind {
            leftovers::Kind::Folder if p.is_dir() => Some(p),
            leftovers::Kind::File => p.parent().filter(|d| d.is_dir()).map(|d| d.to_path_buf()),
            _ => None,
        }
    }

    /// Asks, then removes the leftovers ticked on a thread.
    pub(super) fn remove_leftovers(&mut self) {
        let Some(lo) = &self.app.leftovers else { return };
        let chosen = lo.chosen();
        if chosen.is_empty() {
            return;
        }
        let items: Vec<leftovers::Leftover> = chosen.iter().map(|&i| lo.items[i].clone()).collect();
        let files = items.iter().filter(|l| matches!(l.kind, leftovers::Kind::Folder | leftovers::Kind::File)).count();
        let keys = items.len() - files;
        let stamp = {
            let t = rt_sys::local_time();
            format!("{:04}-{:02}-{:02} {:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
        };
        let dir = self
            .location
            .dir
            .clone()
            .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())))
            .unwrap_or_else(std::env::temp_dir)
            .join("backups");
        let backup = dir.join(format!("leftovers {stamp}.reg"));
        let mut text = tf("{} leftovers will be removed:", &[&items.len().to_string()]);
        text += "\n\n";
        let list: Vec<String> = items.iter().take(12).map(|l| format!("  {}", runinst::ui::leftovers_view::what(l))).collect();
        text += &list.join("\n");
        if items.len() > 12 {
            text += &format!("\n  {}", tf("and {} more", &[&(items.len() - 12).to_string()]));
        }
        if files > 0 {
            text += "\n\n";
            text += &tf("{} files and folders go to the Recycle Bin.", &[&files.to_string()]);
        }
        if keys > 0 {
            text += "\n\n";
            text += &tf("{} registry keys and values are saved first to {}.", &[&keys.to_string(), &backup.display().to_string()]);
        }
        if !self.confirm(&tr("Remove the leftovers"), &text) {
            return;
        }
        if let Some(lo) = &mut self.app.leftovers {
            lo.removing = true;
        }
        self.app.note = Some(tr("Removing the leftovers…"));
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let results = leftovers::remove(&items, &backup);
            let _ = proxy.send_event(UserEvent::Removed(chosen.into_iter().zip(results).collect(), backup));
        });
        self.menu_dirty = true;
        self.redraw();
    }

    pub(super) fn leftovers_removed(&mut self, results: Vec<(usize, leftovers::Removed)>, backup: &std::path::Path) {
        let Some(lo) = &mut self.app.leftovers else { return };
        lo.removing = false;
        let mut done = vec![false; lo.items.len()];
        let mut failed = Vec::new();
        let mut keys = false;
        for (i, r) in results {
            match r {
                Ok(()) => {
                    done[i] = true;
                    keys |= matches!(lo.items[i].kind, leftovers::Kind::Key | leftovers::Kind::Value);
                }
                Err(e) => {
                    failed.push(format!("  {}: {e}", runinst::ui::leftovers_view::what(&lo.items[i])));
                    lo.failed[i] = Some(e);
                    lo.ticked[i] = false;
                }
            }
        }
        let n = done.iter().filter(|d| **d).count();
        // What is gone leaves the list.
        let mut k = 0;
        lo.items.retain(|_| {
            k += 1;
            !done[k - 1]
        });
        let mut k = 0;
        lo.ticked.retain(|_| {
            k += 1;
            !done[k - 1]
        });
        let mut k = 0;
        lo.failed.retain(|_| {
            k += 1;
            !done[k - 1]
        });
        let empty = lo.items.is_empty();
        self.app.marked.clear();
        self.app.note = None;
        if empty {
            self.app.set_leftovers(None, false);
        } else {
            self.app.rebuild();
        }
        let mut parts = vec![tf("{} leftovers removed.", &[&n.to_string()])];
        if keys {
            parts.push(tf("The registry keys and values were saved to {}: open it to put them back.", &[&backup.display().to_string()]));
        }
        if !failed.is_empty() {
            parts.push(format!("{}\n{}", tf("Did not work ({}):", &[&failed.len().to_string()]), failed.join("\n")));
        }
        self.info(&tr("Remove the leftovers"), &parts.join("\n\n"));
        self.menu_dirty = true;
    }

    /// Asks before doing something; true for yes.
    fn confirm(&mut self, title: &str, text: &str) -> bool {
        #[cfg(windows)]
        {
            use rt_sys::win::dialogs::{message, Answer, Buttons, Icon};
            let hwnd = self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd);
            message(hwnd, title, text, Buttons::OkCancel, Icon::Warning) == Answer::Ok
        }
        #[cfg(not(windows))]
        {
            rt_sys::linux::dialogs::confirm(title, text)
        }
    }
}
