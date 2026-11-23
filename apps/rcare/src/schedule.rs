//! The scheduler: rcare's slow work at a set time (at night, say), through
//! Windows' own Task Scheduler — a task under `\rcare\` that starts
//! `rcare.exe --run sfc,dism,chkdsk,clean` with the highest rights, while the
//! user's session is open (no password is kept). The user's choices,
//! 2026-11-23: the repairs (SFC, DISM, a chkdsk scan of each NTFS drive:
//! checking only) and cleaning the junk ticked in the Disk Cleaner (never the
//! registry or shortcuts). What ran is written to a log and to a list of runs
//! that the Scheduler page shows.

use std::path::{Path, PathBuf};

/// What a schedule runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Sfc,
    Dism,
    /// `chkdsk /scan` of every NTFS drive (checking only).
    Chkdsk,
    /// The Disk Cleaner's ticked targets, but the registry's and the shortcuts'.
    Clean,
}

impl Op {
    pub const ALL: [Op; 4] = [Op::Sfc, Op::Dism, Op::Chkdsk, Op::Clean];

    /// The word on the command line and in the files.
    pub fn word(self) -> &'static str {
        match self {
            Op::Sfc => "sfc",
            Op::Dism => "dism",
            Op::Chkdsk => "chkdsk",
            Op::Clean => "clean",
        }
    }

    pub fn from_word(w: &str) -> Option<Op> {
        Op::ALL.into_iter().find(|o| o.word().eq_ignore_ascii_case(w.trim()))
    }

    /// What the page calls it (a translation key).
    pub fn label(self) -> &'static str {
        match self {
            Op::Sfc => "System files (SFC)",
            Op::Dism => "Windows image (DISM)",
            Op::Chkdsk => "Check the drives (chkdsk)",
            Op::Clean => "Clean the junk ticked",
        }
    }
}

/// `sfc,dism` → the operations (unknown words left out, each once).
pub fn parse_ops(s: &str) -> Vec<Op> {
    let mut out = Vec::new();
    for o in s.split([',', ' ']).filter_map(Op::from_word) {
        if !out.contains(&o) {
            out.push(o);
        }
    }
    out
}

pub fn ops_text(ops: &[Op]) -> String {
    ops.iter().map(|o| o.word()).collect::<Vec<_>>().join(",")
}

/// How often.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    /// On a weekday: 0 Monday … 6 Sunday.
    Weekly(u8),
    /// On a day of the month (1–28, so every month has it).
    Monthly(u8),
}

const DAYS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];

impl Freq {
    pub fn word(self) -> String {
        match self {
            Freq::Daily => "daily".into(),
            Freq::Weekly(d) => format!("weekly:{}", DAYS[usize::from(d.min(6))].to_lowercase()),
            Freq::Monthly(d) => format!("monthly:{}", d.clamp(1, 28)),
        }
    }

    pub fn from_word(w: &str) -> Option<Freq> {
        let w = w.trim().to_lowercase();
        if w == "daily" {
            return Some(Freq::Daily);
        }
        if let Some(d) = w.strip_prefix("weekly:") {
            return DAYS.iter().position(|x| x.eq_ignore_ascii_case(d)).map(|i| Freq::Weekly(i as u8));
        }
        w.strip_prefix("monthly:")?.parse::<u8>().ok().filter(|d| (1..=28).contains(d)).map(Freq::Monthly)
    }
}

/// A schedule rcare made: its task's name, what it runs, when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    /// The task's name under `\rcare\` (`Schedule 1`…).
    pub name: String,
    pub ops: Vec<Op>,
    pub freq: Freq,
    pub hour: u8,
    pub minute: u8,
}

impl Schedule {
    pub const FILE: &'static str = "schedules.txt";

    /// The task's full name in Task Scheduler.
    pub fn task(&self) -> String {
        format!("\\rcare\\{}", self.name)
    }

    pub fn time(&self) -> String {
        format!("{:02}:{:02}", self.hour.min(23), self.minute.min(59))
    }

    pub fn line(&self) -> String {
        format!("{}\t{}\t{}\t{}", self.name, ops_text(&self.ops), self.freq.word(), self.time())
    }

    pub fn from_line(l: &str) -> Option<Schedule> {
        let f: Vec<&str> = l.trim_end_matches('\r').split('\t').collect();
        let [name, ops, freq, time, ..] = f.as_slice() else { return None };
        let (h, m) = time.split_once(':')?;
        let ops = parse_ops(ops);
        let (hour, minute) = (h.trim().parse::<u8>().ok().filter(|h| *h < 24)?, m.trim().parse::<u8>().ok().filter(|m| *m < 60)?);
        let freq = Freq::from_word(freq)?;
        (!name.trim().is_empty() && !ops.is_empty()).then(|| Schedule { name: name.trim().into(), ops, freq, hour, minute })
    }

    /// `schtasks` arguments that make (or replace) its task: the highest rights, only while the user's session is open.
    pub fn create_args(&self, exe: &Path) -> Vec<String> {
        let mut a: Vec<String> =
            vec!["/Create".into(), "/TN".into(), self.task(), "/TR".into(), format!("\"{}\" --run {}", exe.display(), ops_text(&self.ops))];
        match self.freq {
            Freq::Daily => a.extend(["/SC".into(), "DAILY".into()]),
            Freq::Weekly(d) => a.extend(["/SC".into(), "WEEKLY".into(), "/D".into(), DAYS[usize::from(d.min(6))].into()]),
            Freq::Monthly(d) => a.extend(["/SC".into(), "MONTHLY".into(), "/D".into(), d.clamp(1, 28).to_string()]),
        }
        a.extend(["/ST".into(), self.time(), "/RL".into(), "HIGHEST".into(), "/IT".into(), "/F".into()]);
        a
    }
}

/// The schedules kept in rcare's data folder.
pub fn load(dir: &Path) -> Vec<Schedule> {
    std::fs::read_to_string(dir.join(Schedule::FILE)).map(|t| t.lines().filter_map(Schedule::from_line).collect()).unwrap_or_default()
}

pub fn save(dir: &Path, list: &[Schedule]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut t = String::from("# rcare: the schedules (task name, what runs, how often, time)\n");
    for s in list {
        t += &s.line();
        t.push('\n');
    }
    std::fs::write(dir.join(Schedule::FILE), t)
}

/// A free name: `Schedule 1`, `Schedule 2`…
pub fn free_name(list: &[Schedule]) -> String {
    (1..).map(|n| format!("Schedule {n}")).find(|n| !list.iter().any(|s| &s.name == n)).unwrap_or_default()
}

// ------------------------------------------------------------ Task Scheduler

/// What Task Scheduler says of a task.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskStatus {
    /// `Ready`, `Running`, `Disabled` (Windows' words).
    pub state: String,
    /// `2026-11-24T02:00:00`, or empty.
    pub next: String,
    pub last: String,
    pub result: Option<i64>,
}

/// The PowerShell that lists rcare's tasks, one a line.
pub const STATUS_SCRIPT: &str = r#"$ErrorActionPreference='SilentlyContinue'
foreach($t in Get-ScheduledTask -TaskPath '\rcare\'){
  $i = $t | Get-ScheduledTaskInfo
  $n = if($i.NextRunTime){$i.NextRunTime.ToString('s')}
  $l = if($i.LastRunTime -and $i.LastRunTime.Year -gt 2000){$i.LastRunTime.ToString('s')}
  "task`t$($t.TaskName)`t$($t.State)`t$n`t$l`t$($i.LastTaskResult)"
}"#;

pub fn parse_status(text: &str) -> Vec<(String, TaskStatus)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim_end_matches('\r').split('\t').collect();
            let ["task", name, state, next, last, result, ..] = f.as_slice() else { return None };
            Some((
                name.trim().to_string(),
                TaskStatus {
                    state: state.trim().into(),
                    next: next.trim().into(),
                    last: last.trim().into(),
                    result: result.trim().parse().ok(),
                },
            ))
        })
        .collect()
}

/// Asks Task Scheduler. None: PowerShell could not be run.
pub fn status() -> Option<Vec<(String, TaskStatus)>> {
    crate::ps::run(STATUS_SCRIPT).map(|t| parse_status(&t))
}

fn schtasks(args: &[String]) -> Result<(), String> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match crate::ps::command("schtasks.exe", &refs) {
        Some((true, _)) => Ok(()),
        Some((false, text)) => Err(crate::ps::last_line(&text)),
        None => Err("schtasks".into()),
    }
}

/// Makes (or replaces) the task of `s`, to run this program.
pub fn create(s: &Schedule) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    schtasks(&s.create_args(&exe))
}

pub fn delete(s: &Schedule) -> Result<(), String> {
    schtasks(&["/Delete".into(), "/TN".into(), s.task(), "/F".into()])
}

pub fn run_now(s: &Schedule) -> Result<(), String> {
    schtasks(&["/Run".into(), "/TN".into(), s.task()])
}

// ------------------------------------------------------------ the runs

/// One operation of a run, as the list of runs keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// When it ended (`2026-11-24 02:14`).
    pub when: String,
    pub op: Op,
    pub ok: bool,
    /// What it came to, in a line.
    pub detail: String,
}

impl Run {
    pub const FILE: &'static str = "schedule-runs.txt";
    pub const LOG: &'static str = "schedule.log";
    /// The most runs kept in the list.
    pub const KEEP: usize = 200;

    pub fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.when,
            self.op.word(),
            if self.ok { "ok" } else { "failed" },
            self.detail.replace(['\t', '\n', '\r'], " ")
        )
    }

    pub fn from_line(l: &str) -> Option<Run> {
        let f: Vec<&str> = l.trim_end_matches('\r').splitn(4, '\t').collect();
        let [when, op, ok, detail] = f.as_slice() else { return None };
        Some(Run { when: when.to_string(), op: Op::from_word(op)?, ok: *ok == "ok", detail: detail.to_string() })
    }
}

/// The runs, the latest first.
pub fn runs(dir: &Path) -> Vec<Run> {
    let mut v: Vec<Run> =
        std::fs::read_to_string(dir.join(Run::FILE)).map(|t| t.lines().filter_map(Run::from_line).collect()).unwrap_or_default();
    v.reverse();
    v
}

fn now() -> String {
    let t = rt_sys::local_time();
    format!("{:04}-{:02}-{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute)
}

/// Adds runs to the list (keeping the last `KEEP`) and text to the log.
fn record(dir: &Path, new: &[Run], log: &str) {
    let _ = std::fs::create_dir_all(dir);
    let mut lines: Vec<String> =
        std::fs::read_to_string(dir.join(Run::FILE)).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
    lines.extend(new.iter().map(Run::line));
    let start = lines.len().saturating_sub(Run::KEEP);
    let _ = std::fs::write(dir.join(Run::FILE), lines[start..].join("\n") + "\n");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(Run::LOG)) {
        let _ = f.write_all(log.as_bytes());
    }
}

/// Runs a repair to its end: its exit code (or why it did not start) and what it wrote.
fn run_task(task: &crate::repair::Task) -> (Result<i32, String>, String) {
    let (send, recv) = std::sync::mpsc::channel();
    let send = std::sync::Mutex::new(send);
    crate::repair::run(task, move |e| {
        if let Ok(s) = send.lock() {
            let _ = s.send(e);
        }
    });
    let mut out = crate::repair::Output::default();
    loop {
        match recv.recv() {
            Ok(crate::repair::Event::Text(t)) => out.feed(&t),
            Ok(crate::repair::Event::Done(code)) => return (code, out.all().join("\r\n")),
            Err(_) => return (Err("ended without a word".into()), out.all().join("\r\n")),
        }
    }
}

/// What `rcare --run …` does: each operation in turn, recorded in `dir`. The exit code: 0 when all went well.
pub fn run(ops: &[Op], dir: &Path, elevated: bool) -> i32 {
    let mut runs = Vec::new();
    let mut log = format!("== {} — rcare --run {}\r\n", now(), ops_text(ops));
    for &op in ops {
        let (ok, detail, text) = match op {
            Op::Sfc | Op::Dism => {
                let task = if op == Op::Sfc { crate::repair::Task::Sfc } else { crate::repair::Task::Dism };
                let (code, text) = run_task(&task);
                match code {
                    Ok(0) => (true, "finished".to_string(), text),
                    Ok(n) => (false, format!("ended with code {n}"), text),
                    Err(e) => (false, e, text),
                }
            }
            Op::Chkdsk => {
                let drives: Vec<char> = crate::disks::read()
                    .map(|v| v.into_iter().filter(|v| v.file_system.eq_ignore_ascii_case("NTFS")).map(|v| v.letter).collect())
                    .unwrap_or_default();
                let mut all_ok = true;
                let mut parts = Vec::new();
                let mut text = String::new();
                for d in drives {
                    let (code, t) = run_task(&crate::repair::Task::Check(d));
                    text += &t;
                    text += "\r\n";
                    let word = match code.as_ref().map(|c| crate::repair::check_result(*c)) {
                        Ok(crate::repair::CheckResult::Clean) => "no errors".to_string(),
                        Ok(crate::repair::CheckResult::Fixed) => "errors fixed".to_string(),
                        Ok(crate::repair::CheckResult::NeedsFix) => {
                            all_ok = false;
                            "errors to fix (Repair page)".to_string()
                        }
                        Ok(crate::repair::CheckResult::Other(n)) => {
                            all_ok = false;
                            format!("code {n}")
                        }
                        Err(e) => {
                            all_ok = false;
                            e.clone()
                        }
                    };
                    parts.push(format!("{d}: {word}"));
                }
                if parts.is_empty() {
                    (false, "no NTFS drive found".to_string(), text)
                } else {
                    (all_ok, parts.join(", "), text)
                }
            }
            Op::Clean => clean_junk(dir, elevated),
        };
        log += &format!("-- {} {}: {}\r\n{}\r\n", now(), op.word(), detail, text.trim_end());
        runs.push(Run { when: now(), op, ok, detail });
    }
    record(dir, &runs, &(log + "\r\n"));
    i32::from(!runs.iter().all(|r| r.ok))
}

/// The Disk Cleaner's ticked targets, but the registry's and the shortcuts' (never without the user looking).
fn clean_junk(dir: &Path, elevated: bool) -> (bool, String, String) {
    use rclean::targets::Group;
    use std::sync::atomic::AtomicBool;
    let mut location = crate::storage::detect();
    let settings = crate::settings::Settings::load(&location.load());
    let app = rclean::app::App::new(rclean::targets::all(&rclean::targets::Env::here()), elevated, &settings.ticks);
    let stop = AtomicBool::new(false);
    let backups = dir.join("backups");
    let (mut bytes, mut files, mut failed) = (0u64, 0u64, 0u64);
    let mut text = String::new();
    for it in app.items.iter().filter(|it| it.ticked && !matches!(it.target.group, Group::Registry | Group::Shortcuts)) {
        let found = rt_clean::clean::analyse(&it.target, &stop);
        let c = rt_clean::clean::clean(&it.target, &found, &stop, &backups);
        bytes += c.bytes;
        files += c.files;
        failed += c.failed;
        text += &format!(
            "{}: {} files, {} bytes{}\r\n",
            it.target.id,
            c.files,
            c.bytes,
            c.error.map(|e| format!(" ({e})")).unwrap_or_default()
        );
    }
    let detail = format!(
        "{} freed in {} files{}",
        rsize::format::size(bytes),
        files,
        if failed > 0 { format!(", {failed} in use") } else { String::new() }
    );
    (true, detail, text)
}

/// The command line of a scheduled start: the operations after `--run`, if it is one.
pub fn run_args(args: &[String]) -> Option<Vec<Op>> {
    let i = args.iter().position(|a| a == "--run")?;
    Some(args.get(i + 1).map(|s| parse_ops(s)).unwrap_or_default())
}

/// The log's path.
pub fn log_file(dir: &Path) -> PathBuf {
    dir.join(Run::LOG)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, freq: Freq) -> Schedule {
        Schedule { name: name.into(), ops: vec![Op::Sfc, Op::Clean], freq, hour: 2, minute: 30 }
    }

    #[test]
    fn words_and_lines() {
        assert_eq!(parse_ops("sfc, DISM,clean,sfc,nothing"), vec![Op::Sfc, Op::Dism, Op::Clean]);
        for f in [Freq::Daily, Freq::Weekly(0), Freq::Weekly(6), Freq::Monthly(1), Freq::Monthly(28)] {
            assert_eq!(Freq::from_word(&f.word()), Some(f));
        }
        assert_eq!(Freq::from_word("monthly:31"), None);
        let a = s("Schedule 1", Freq::Weekly(2));
        assert_eq!(a.line(), "Schedule 1\tsfc,clean\tweekly:wed\t02:30");
        assert_eq!(Schedule::from_line(&a.line()), Some(a.clone()));
        assert_eq!(Schedule::from_line("x\t\tdaily\t02:00"), None, "nothing to run");
        assert_eq!(Schedule::from_line("x\tsfc\tdaily\t25:00"), None);
        assert_eq!(free_name(&[a.clone(), s("Schedule 3", Freq::Daily)]), "Schedule 2");
        assert_eq!(run_args(&["--run".into(), "sfc,chkdsk".into()]), Some(vec![Op::Sfc, Op::Chkdsk]));
        assert_eq!(run_args(&["--user".into()]), None);
    }

    #[test]
    fn task_scheduler_arguments() {
        let exe = Path::new("C:\\Programs\\rcare\\rcare.exe");
        let a = s("Schedule 1", Freq::Weekly(0)).create_args(exe);
        assert_eq!(a[..5], ["/Create", "/TN", "\\rcare\\Schedule 1", "/TR", "\"C:\\Programs\\rcare\\rcare.exe\" --run sfc,clean"]);
        assert!(a.windows(4).any(|w| w == ["/SC", "WEEKLY", "/D", "MON"]));
        assert!(a.windows(2).any(|w| w == ["/ST", "02:30"]) && a.windows(2).any(|w| w == ["/RL", "HIGHEST"]));
        assert!(a.contains(&"/IT".to_string()) && a.contains(&"/F".to_string()));
        let m = s("Schedule 2", Freq::Monthly(15)).create_args(exe);
        assert!(m.windows(4).any(|w| w == ["/SC", "MONTHLY", "/D", "15"]));
        let st = parse_status("task\tSchedule 1\tReady\t2026-11-24T02:30:00\t\t267011\r\nnoise\n");
        assert_eq!(st[0].0, "Schedule 1");
        assert_eq!((st[0].1.state.as_str(), st[0].1.result), ("Ready", Some(267011)));
    }

    #[test]
    fn files_round_trip() {
        let dir = std::env::temp_dir().join(format!("rcare-sched-{}", std::process::id()));
        let list = vec![s("Schedule 1", Freq::Daily), s("Schedule 2", Freq::Monthly(3))];
        save(&dir, &list).unwrap();
        assert_eq!(load(&dir), list);
        let r = Run { when: "2026-11-24 02:40".into(), op: Op::Sfc, ok: true, detail: "finished\tok".into() };
        record(&dir, &[r.clone(), Run { op: Op::Clean, ..r.clone() }], "log text\r\n");
        let back = runs(&dir);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].op, Op::Clean, "the latest first");
        assert_eq!(back[1].detail, "finished ok");
        assert!(std::fs::read_to_string(log_file(&dir)).unwrap().contains("log text"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(load(&dir).is_empty() && runs(&dir).is_empty());
    }
}
