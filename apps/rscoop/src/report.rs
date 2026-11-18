//! Text and JSON reports.

use crate::checks::{CheckResult, Context, Problem, Sev};
use crate::fix::{FixLog, Status};
use crate::paths::{exists, is_dir};
use crate::text::T;

/// A JSON string literal.
pub fn js(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn jopt(s: &Option<String>) -> String {
    s.as_deref().map(js).unwrap_or_else(|| "null".into())
}

fn args_of(p: &Problem) -> Vec<(&str, &str)> {
    p.args.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

pub fn message(t: &T, p: &Problem) -> String {
    t.fill(&p.code, &args_of(p))
}

/// (errors, warnings, checks with problems)
pub fn summary(results: &[CheckResult]) -> (usize, usize, usize) {
    let e = results.iter().map(|r| r.count(Sev::Error)).sum();
    let w = results.iter().map(|r| r.count(Sev::Warning)).sum();
    (e, w, results.iter().filter(|r| !r.problems.is_empty()).count())
}

fn other_roots(ctx: &Context) -> Vec<String> {
    let mut v: Vec<String> = ctx.discovered.lock().unwrap().values().cloned().collect();
    v.sort_by_key(|s| s.to_lowercase());
    v
}

fn pad(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(s.chars().count())))
}

fn info_name(t: &T, label: &str) -> String {
    label.strip_prefix("var:").map(str::to_string).unwrap_or_else(|| t.get(label).to_string())
}

/// Extra lines of the roots section: the caches used.
pub struct Extras {
    pub cache: String,
    pub scoop_db: String,
}

pub fn text_report(ctx: &Context, results: &[CheckResult], t: &T, max: usize, seconds: Option<f64>, extras: &Extras) -> String {
    let mut out: Vec<String> = Vec::new();
    let title = t.get("report.title");
    out.push(title.to_string());
    out.push("=".repeat(title.chars().count()));
    let labels: Vec<String> = ["info.root", "info.global_root", "info.other_roots", "info.cache", "info.scoop_db"]
        .iter()
        .map(|k| format!("{}:", t.get(k)))
        .collect();
    let w = labels.iter().map(|s| s.chars().count()).max().unwrap_or(0);
    out.push(format!("  {} {}  ({})", pad(&labels[0], w), ctx.root, t.get(&format!("src.{}", ctx.roots.root_source))));
    let gstate = t.get(if is_dir(&ctx.global_root) { "status.exists" } else { "status.missing" });
    out.push(format!("  {} {}  ({}, {gstate})", pad(&labels[1], w), ctx.global_root, t.get(&format!("src.{}", ctx.roots.global_source))));
    let others = other_roots(ctx);
    for (i, o) in others.iter().enumerate() {
        let state = t.get(if is_dir(o) { "status.exists" } else { "status.missing" });
        out.push(format!("  {} {o}  ({state})", pad(if i == 0 { &labels[2] } else { "" }, w)));
    }
    if others.is_empty() {
        out.push(format!("  {} {}", pad(&labels[2], w), t.get("report.none")));
    }
    out.push(format!("  {} {}", pad(&labels[3], w), extras.cache));
    out.push(format!("  {} {}", pad(&labels[4], w), extras.scoop_db));
    out.push(String::new());

    for r in results {
        let mut parts = vec![t.fill("report.checked", &[("n", &r.checked.to_string())])];
        let (e, wn) = (r.count(Sev::Error), r.count(Sev::Warning));
        if e > 0 {
            parts.push(t.fill("report.errors", &[("n", &e.to_string())]));
        }
        if wn > 0 {
            parts.push(t.fill("report.warnings", &[("n", &wn.to_string())]));
        }
        if e == 0 && wn == 0 {
            parts.push(t.get("report.ok").to_string());
        }
        let took = r.seconds.map(|s| format!("  ({s:.1} s)")).unwrap_or_default();
        out.push(format!("[{}] {}{took}", t.get(&format!("check.{}", r.id)), parts.join(", ")));
        for (label, value, status) in &r.info {
            out.push(format!("    {} = {value}  ({})", info_name(t, label), t.get(status)));
        }
        let mut problems: Vec<&Problem> = r.problems.iter().collect();
        problems.sort_by_key(|p| (p.severity != Sev::Error, p.item.to_lowercase()));
        let shown = if max == 0 { problems.len() } else { problems.len().min(max) };
        for p in &problems[..shown] {
            let sev = t.get(if p.severity == Sev::Error { "sev.error" } else { "sev.warning" });
            out.push(format!("  {} {}", pad(sev, 6), p.item));
            out.push(format!("         {}", message(t, p)));
            if let Some(a) = p.actual.as_deref().filter(|a| !a.is_empty()) {
                out.push(format!("         {}: {a}", t.get("report.found")));
            }
            if let Some(x) = p.expected.as_deref().filter(|x| !x.is_empty()) {
                if x.contains('<') {
                    out.push(format!("         {}: {x}", t.get("report.expected")));
                } else {
                    let state = t.get(if exists(x) { "report.exists" } else { "report.not_exists" });
                    out.push(format!("         {}: {x}  ({state})", t.get("report.expected")));
                }
            }
        }
        if problems.len() > shown {
            out.push(format!("  {}", t.fill("report.more", &[("n", &(problems.len() - shown).to_string())])));
        }
        out.push(String::new());
    }
    let (errors, warnings, with) = summary(results);
    let line = t.fill(
        "report.summary_line",
        &[
            ("errors", &errors.to_string()),
            ("warnings", &warnings.to_string()),
            ("checks", &with.to_string()),
            ("total", &results.len().to_string()),
        ],
    );
    out.push(format!("{}: {line}", t.get("report.summary")));
    if let Some(s) = seconds {
        out.push(t.fill("report.duration", &[("s", &format!("{s:.1}"))]));
    }
    out.join("\n") + "\n"
}

const FIX_STATUSES: [Status; 4] = [Status::Done, Status::Planned, Status::Failed, Status::Skipped];

fn entry_message(t: &T, key: &str, args: &[(String, String)]) -> String {
    let a: Vec<(&str, &str)> = args.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    t.fill(key, &a)
}

pub fn fix_report(log: &FixLog, backup_dir: &str, dry: bool, t: &T, max: usize) -> String {
    let mut out = vec![String::new()];
    let title = t.get(if dry { "fix.title_dry" } else { "fix.title" });
    out.push(title.to_string());
    out.push("=".repeat(title.chars().count()));
    for s in FIX_STATUSES {
        let entries: Vec<_> = log.entries.iter().filter(|e| e.status == s).collect();
        if entries.is_empty() {
            continue;
        }
        out.push(format!("[{}] {}", t.get(&format!("fix.status.{}", s.as_str())), entries.len()));
        let shown = if max == 0 { entries.len() } else { entries.len().min(max) };
        for e in &entries[..shown] {
            out.push(format!("  {} {}", pad(e.check, 10), e.item));
            out.push(format!("             {}", entry_message(t, e.key, &e.args)));
        }
        if entries.len() > shown {
            out.push(format!("  {}", t.fill("report.more", &[("n", &(entries.len() - shown).to_string())])));
        }
        out.push(String::new());
    }
    if log.entries.is_empty() {
        out.push(t.get("fix.nothing").to_string());
    }
    let c = |s| log.count(s).to_string();
    out.push(t.fill(
        "fix.summary",
        &[("done", &c(Status::Done)), ("planned", &c(Status::Planned)), ("failed", &c(Status::Failed)), ("skipped", &c(Status::Skipped))],
    ));
    if log.count(Status::Done) > 0 {
        out.push(t.fill("fix.backup", &[("path", backup_dir)]));
        out.push(t.get("fix.rerun").to_string());
    }
    out.join("\n") + "\n"
}

fn args_json(args: &[(String, String)]) -> String {
    let parts: Vec<String> = args.iter().map(|(k, v)| format!("{}: {}", js(k), js(v))).collect();
    format!("{{{}}}", parts.join(", "))
}

pub fn json_report(ctx: &Context, results: &[CheckResult], t: &T, seconds: Option<f64>, fix: Option<(&FixLog, &str, bool)>) -> String {
    let (errors, warnings, with) = summary(results);
    let mut o = String::from("{\n");
    o += &format!("  \"root\": {},\n", js(&ctx.root));
    o += &format!("  \"root_source\": {},\n", js(&ctx.roots.root_source));
    o += &format!("  \"global_root\": {},\n", js(&ctx.global_root));
    o += &format!("  \"global_root_exists\": {},\n", is_dir(&ctx.global_root));
    o += &format!("  \"config_file\": {},\n", js(&ctx.roots.config_path));
    let others: Vec<String> = other_roots(ctx).iter().map(|r| format!("{{\"path\": {}, \"exists\": {}}}", js(r), is_dir(r))).collect();
    o += &format!("  \"other_roots\": [{}],\n", others.join(", "));
    o += &format!("  \"summary\": {{\"errors\": {errors}, \"warnings\": {warnings}, \"checks_with_problems\": {with}}},\n");
    if let Some(s) = seconds {
        o += &format!("  \"seconds\": {s:.2},\n");
    }
    let mut checks = Vec::new();
    for r in results {
        let info: Vec<String> = r
            .info
            .iter()
            .map(|(l, v, s)| {
                let name = l.strip_prefix("var:").unwrap_or(l);
                format!("{{\"name\": {}, \"value\": {}, \"status\": {}}}", js(name), js(v), js(s))
            })
            .collect();
        let problems: Vec<String> = r
            .problems
            .iter()
            .map(|p| {
                format!(
                    "      {{\"severity\": {}, \"item\": {}, \"code\": {}, \"message\": {}, \"found\": {}, \"expected\": {}, \"args\": {}}}",
                    js(p.severity.as_str()),
                    js(&p.item),
                    js(&p.code),
                    js(&message(t, p)),
                    jopt(&p.actual),
                    jopt(&p.expected),
                    args_json(&p.args)
                )
            })
            .collect();
        let secs = r.seconds.map(|s| format!("{s:.2}")).unwrap_or_else(|| "null".into());
        checks.push(format!(
            "    {{\"id\": {}, \"title\": {}, \"checked\": {}, \"seconds\": {secs}, \"info\": [{}], \"problems\": [\n{}\n    ]}}",
            js(r.id),
            js(t.get(&format!("check.{}", r.id))),
            r.checked,
            info.join(", "),
            problems.join(",\n")
        ));
    }
    o += &format!("  \"checks\": [\n{}\n  ]", checks.join(",\n"));
    if let Some((log, backup, dry)) = fix {
        let actions: Vec<String> = log
            .entries
            .iter()
            .map(|e| {
                format!(
                    "      {{\"status\": {}, \"check\": {}, \"item\": {}, \"code\": {}, \"message\": {}, \"args\": {}}}",
                    js(e.status.as_str()),
                    js(e.check),
                    js(&e.item),
                    js(e.key),
                    js(&entry_message(t, e.key, &e.args)),
                    args_json(&e.args)
                )
            })
            .collect();
        let b = if log.count(Status::Done) > 0 { js(backup) } else { "null".into() };
        o += &format!(",\n  \"fix\": {{\"dry_run\": {dry}, \"backup_dir\": {b}, \"actions\": [\n{}\n  ]}}", actions.join(",\n"));
    }
    o + "\n}\n"
}
