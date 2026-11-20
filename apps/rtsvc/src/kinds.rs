//! The snapshots the service makes, and what each client may see of them.

use crate::snapshots::{Kind, Viewer};
use rt_svc::proto::snapshots::{CLEAN, PROGRAMS};
use rt_svc::Level;

/// The programs: made again a day after, or when the keys of the programs change.
fn programs() -> Kind {
    Kind {
        name: PROGRAMS,
        max_age: 24 * 3600,
        min_gap: 300,
        produce: Box::new(|_| rt_inst::snapshot::encode(&rt_inst::inventory::read())),
        view: Box::new(programs_view),
    }
}

/// The machine's programs and the client's own; an elevated client sees everyone's.
pub fn programs_view(v: &Viewer, data: &[u8]) -> Option<Vec<u8>> {
    if v.level == Level::Denied {
        return None;
    }
    let inv = rt_inst::snapshot::decode(data).ok()?;
    Some(rt_inst::snapshot::encode(&rt_inst::snapshot::view_for(inv, &v.account, v.level == Level::Elevated)))
}

/// What the cleaner finds that needs administrator rights: sizes and counts only.
fn clean() -> Kind {
    Kind {
        name: CLEAN,
        max_age: 6 * 3600,
        min_gap: 300,
        produce: Box::new(rt_clean::snapshot::produce),
        view: Box::new(|v, d| (v.level != Level::Denied).then(|| d.to_vec())),
    }
}

pub fn all() -> Vec<Kind> {
    vec![programs(), clean()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_inst::inventory::{Inventory, Program, Scope};

    #[test]
    fn a_user_gets_a_filtered_inventory_and_the_denied_get_none() {
        let inv = Inventory {
            programs: vec![
                Program { name: "M".into(), ..Program::default() },
                Program { name: "A".into(), scope: Scope::User { name: "ana".into(), current: false }, ..Program::default() },
                Program { name: "R".into(), scope: Scope::User { name: "rui".into(), current: false }, ..Program::default() },
            ],
            ..Inventory::default()
        };
        let data = rt_inst::snapshot::encode(&inv);
        let seen = |level, account: &str| {
            let v = Viewer { level, account: account.into() };
            programs_view(&v, &data)
                .map(|b| rt_inst::snapshot::decode(&b).unwrap().programs.iter().map(|p| p.name.clone()).collect::<Vec<_>>())
        };
        assert_eq!(seen(Level::User, "ana"), Some(vec!["M".to_string(), "A".to_string()]));
        assert_eq!(seen(Level::Elevated, "ana").map(|v| v.len()), Some(3));
        assert_eq!(seen(Level::Denied, "ana"), None);
        assert_eq!(programs_view(&Viewer { level: Level::User, account: "ana".into() }, b"junk"), None);
    }
}
