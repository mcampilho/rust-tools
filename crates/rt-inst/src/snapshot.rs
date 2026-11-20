//! The inventory as a snapshot of the privileged service: how it is written
//! and read, which part of it a client may see, and how a tool asks for it.

use crate::inventory::{Bits, Inventory, Me, Problem, Program, Scope, Source, WingetInfo};
use rt_svc::codec::{DecodeError, Reader, Writer};
use std::path::PathBuf;

const VERSION: u8 = 1;

const SOURCES: [Source; 15] = [
    Source::Registry,
    Source::Msi,
    Source::Store,
    Source::Winget,
    Source::Scoop,
    Source::Chocolatey,
    Source::Dpkg,
    Source::Rpm,
    Source::Flatpak,
    Source::Snap,
    Source::Dism,
    Source::Npm,
    Source::Pip,
    Source::Cargo,
    Source::Vcpkg,
];
const PROBLEMS: [Problem; 4] = [Problem::InstallFailed, Problem::ManifestRemoved, Problem::UninstallerMissing, Problem::ConfigOnly];
const BITS: [Bits; 3] = [Bits::B64, Bits::B32, Bits::Any];

fn index<T: PartialEq + Copy>(all: &[T], v: T) -> u8 {
    all.iter().position(|x| *x == v).unwrap_or(0) as u8
}

fn pick<T: Copy>(all: &[T], i: u8) -> Result<T, DecodeError> {
    all.get(i as usize).copied().ok_or(DecodeError::Invalid)
}

fn opt_str(w: &mut Writer, s: &Option<String>) {
    w.bool(s.is_some());
    if let Some(s) = s {
        w.str(s);
    }
}

fn get_opt_str(r: &mut Reader) -> Result<Option<String>, DecodeError> {
    Ok(if r.bool()? { Some(r.str()?.to_string()) } else { None })
}

fn put_program(w: &mut Writer, p: &Program) {
    w.str(&p.name).str(&p.publisher).str(&p.version);
    w.bool(p.size.is_some()).u64(p.size.unwrap_or(0));
    w.bool(p.date.is_some());
    let (y, m, d) = p.date.unwrap_or((0, 0, 0));
    w.u32(y as u32).u32(m).u32(d);
    opt_str(w, &p.location.as_ref().map(|l| l.to_string_lossy().into_owned()));
    opt_str(w, &p.uninstall);
    opt_str(w, &p.quiet);
    opt_str(w, &p.modify);
    opt_str(w, &p.icon.as_ref().map(|(f, _)| f.to_string_lossy().into_owned()));
    w.u32(p.icon.as_ref().map_or(0, |(_, n)| *n) as u32);
    opt_str(w, &p.msi);
    w.bool(p.system).bool(p.update).bool(p.no_remove);
    match &p.scope {
        Scope::Machine => w.u8(0),
        Scope::User { name, current } => w.u8(1).str(name).bool(*current),
    };
    w.u8(index(&BITS, p.bits)).str(&p.key);
    w.u32(p.values.len() as u32);
    for (k, v) in &p.values {
        w.str(k).str(v);
    }
    w.u32(p.sources.len() as u32);
    for s in &p.sources {
        w.u8(index(&SOURCES, *s));
    }
    opt_str(w, &p.package);
    opt_str(w, &p.app_id);
    w.bool(p.winget.is_some());
    if let Some(wi) = &p.winget {
        w.str(&wi.id);
        opt_str(w, &wi.available);
        w.str(&wi.repo);
    }
    w.u32(p.problems.len() as u32);
    for q in &p.problems {
        w.u8(index(&PROBLEMS, *q));
    }
}

fn get_program(r: &mut Reader) -> Result<Program, DecodeError> {
    let (name, publisher, version) = (r.str()?.to_string(), r.str()?.to_string(), r.str()?.to_string());
    let has_size = r.bool()?;
    let size = r.u64()?;
    let has_date = r.bool()?;
    let date = (r.u32()? as i32, r.u32()?, r.u32()?);
    let location = get_opt_str(r)?.map(PathBuf::from);
    let (uninstall, quiet, modify) = (get_opt_str(r)?, get_opt_str(r)?, get_opt_str(r)?);
    let icon_file = get_opt_str(r)?;
    let icon_index = r.u32()? as i32;
    let msi = get_opt_str(r)?;
    let (system, update, no_remove) = (r.bool()?, r.bool()?, r.bool()?);
    let scope = match r.u8()? {
        0 => Scope::Machine,
        1 => Scope::User { name: r.str()?.to_string(), current: r.bool()? },
        _ => return Err(DecodeError::Invalid),
    };
    let bits = pick(&BITS, r.u8()?)?;
    let key = r.str()?.to_string();
    let mut values = Vec::new();
    for _ in 0..r.count(8)? {
        values.push((r.str()?.to_string(), r.str()?.to_string()));
    }
    let mut sources = Vec::new();
    for _ in 0..r.count(1)? {
        sources.push(pick(&SOURCES, r.u8()?)?);
    }
    let (package, app_id) = (get_opt_str(r)?, get_opt_str(r)?);
    let winget = if r.bool()? {
        Some(WingetInfo { id: r.str()?.to_string(), available: get_opt_str(r)?, repo: r.str()?.to_string() })
    } else {
        None
    };
    let mut problems = Vec::new();
    for _ in 0..r.count(1)? {
        problems.push(pick(&PROBLEMS, r.u8()?)?);
    }
    Ok(Program {
        name,
        publisher,
        version,
        size: has_size.then_some(size),
        date: has_date.then_some(date),
        location,
        uninstall,
        quiet,
        modify,
        icon: icon_file.map(|f| (PathBuf::from(f), icon_index)),
        msi,
        system,
        update,
        no_remove,
        scope,
        bits,
        key,
        values,
        sources,
        package,
        app_id,
        winget,
        problems,
    })
}

/// The inventory as bytes.
pub fn encode(inv: &Inventory) -> Vec<u8> {
    let mut w = Writer::new();
    w.u8(VERSION);
    w.str(&inv.me.sid).str(&inv.me.name);
    opt_str(&mut w, &inv.me.home.as_ref().map(|h| h.to_string_lossy().into_owned()));
    w.bool(inv.me.runs_as);
    w.u32(inv.programs.len() as u32);
    for p in &inv.programs {
        put_program(&mut w, p);
    }
    w.u32(inv.unread_users.len() as u32);
    for u in &inv.unread_users {
        w.str(u);
    }
    w.u32(inv.other_users.len() as u32);
    for (n, h) in &inv.other_users {
        w.str(n).str(&h.to_string_lossy());
    }
    w.finish()
}

pub fn decode(b: &[u8]) -> Result<Inventory, DecodeError> {
    let mut r = Reader::new(b);
    if r.u8()? != VERSION {
        return Err(DecodeError::Invalid);
    }
    let (sid, name) = (r.str()?.to_string(), r.str()?.to_string());
    let home = get_opt_str(&mut r)?.map(PathBuf::from);
    let runs_as = r.bool()?;
    let mut programs = Vec::new();
    for _ in 0..r.count(40)? {
        programs.push(get_program(&mut r)?);
    }
    let mut unread_users = Vec::new();
    for _ in 0..r.count(4)? {
        unread_users.push(r.str()?.to_string());
    }
    let mut other_users = Vec::new();
    for _ in 0..r.count(8)? {
        other_users.push((r.str()?.to_string(), PathBuf::from(r.str()?)));
    }
    r.end()?;
    Ok(Inventory { me: Me { sid, name, home, runs_as }, programs, unread_users, other_users })
}

/// What the account `account` may see of a full inventory: the programs of
/// the machine and its own; with `elevated`, everything. `current` is set
/// from the account's point of view.
pub fn view_for(mut inv: Inventory, account: &str, elevated: bool) -> Inventory {
    let mine = |name: &str| name.eq_ignore_ascii_case(account);
    if !elevated {
        inv.programs.retain(|p| match &p.scope {
            Scope::Machine => true,
            Scope::User { name, .. } => mine(name),
        });
        inv.unread_users.clear();
        inv.other_users.clear();
    }
    for p in &mut inv.programs {
        if let Scope::User { name, current } = &mut p.scope {
            *current = mine(name);
        }
    }
    inv
}

/// The inventory of the service, if it has one at most `max_age_secs` old.
/// The programs of the user of this computer are told apart from the others'
/// by the name of the account, which the service knows from the token.
pub fn fetch(max_age_secs: u64) -> Option<(Inventory, u64)> {
    let mut c = rt_svc::client::connect("rt-inst", env!("CARGO_PKG_VERSION")).ok()?;
    let s = c.snapshot(rt_svc::proto::snapshots::PROGRAMS, max_age_secs).ok()?;
    let inv = decode(&s.data).ok()?;
    Some((inv, s.age_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(name: &str, scope: Scope) -> Program {
        Program {
            name: name.into(),
            publisher: "Ação, Lda".into(),
            version: "1.2.3".into(),
            size: Some(1 << 40),
            date: Some((2024, 11, 5)),
            location: Some(PathBuf::from(r"C:\Program Files\X")),
            uninstall: Some("\"C:\\x\\unins.exe\"".into()),
            icon: Some((PathBuf::from(r"C:\x\x.exe"), -3)),
            msi: Some("{GUID}".into()),
            update: true,
            scope,
            bits: Bits::B32,
            key: "HKLM\\...".into(),
            values: vec![("DisplayName".into(), name.into()), ("Empty".into(), String::new())],
            sources: vec![Source::Registry, Source::Winget, Source::Vcpkg],
            package: Some("pkg".into()),
            winget: Some(WingetInfo { id: "A.B".into(), available: Some("2".into()), repo: "winget".into() }),
            problems: vec![Problem::UninstallerMissing, Problem::ConfigOnly],
            ..Program::default()
        }
    }

    fn sample() -> Inventory {
        Inventory {
            me: Me { sid: "S-1-5-21-1".into(), name: "ana".into(), home: Some(PathBuf::from(r"C:\Users\ana")), runs_as: true },
            programs: vec![
                program("Machine", Scope::Machine),
                program("Ana's", Scope::User { name: "ana".into(), current: true }),
                program("Rui's", Scope::User { name: "rui".into(), current: false }),
                Program { name: "Bare".into(), ..Program::default() },
            ],
            unread_users: vec!["guest".into()],
            other_users: vec![("rui".into(), PathBuf::from(r"C:\Users\rui"))],
        }
    }

    #[test]
    fn an_inventory_round_trips() {
        let inv = sample();
        assert_eq!(decode(&encode(&inv)).unwrap(), inv);
        assert_eq!(decode(&encode(&Inventory::default())).unwrap(), Inventory::default());
    }

    #[test]
    fn hostile_bytes_are_errors() {
        let good = encode(&sample());
        for cut in (0..good.len()).step_by(7) {
            assert!(decode(&good[..cut]).is_err(), "cut at {cut}");
        }
        let mut bad = good.clone();
        bad[0] = 9;
        assert!(decode(&bad).is_err());
        let mut long = good;
        long.push(1);
        assert!(decode(&long).is_err());
    }

    #[test]
    fn a_user_sees_the_machine_and_their_own() {
        let v = view_for(sample(), "Ana", false);
        let names: Vec<&str> = v.programs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Machine", "Ana's", "Bare"]);
        assert!(v.unread_users.is_empty() && v.other_users.is_empty());
        assert!(matches!(&v.programs[1].scope, Scope::User { current: true, .. }));
    }

    #[test]
    fn an_elevated_client_sees_all_with_current_from_its_point_of_view() {
        let v = view_for(sample(), "rui", true);
        assert_eq!(v.programs.len(), 4);
        assert_eq!(v.other_users.len(), 1);
        let current: Vec<bool> =
            v.programs.iter().filter_map(|p| if let Scope::User { current, .. } = p.scope { Some(current) } else { None }).collect();
        assert_eq!(current, [false, true]);
    }
}
