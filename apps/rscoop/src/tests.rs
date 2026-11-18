//! Tests on a fake Scoop tree (junctions, hard links, shims, shortcuts, .reg
//! and config files) with an in-memory registry. The trees live under
//! `target\rscoop-tests`, inside the project.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fresh empty folder for one test.
pub fn scratch(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("rscoop-tests")
        .join(format!("{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Plain path text: no \\?\ prefix.
    PathBuf::from(crate::paths::normpath(&dir.to_string_lossy()))
}

#[cfg(windows)]
mod fake {
    use super::scratch;
    use crate::checks::{self, find_roots, CheckResult, Context, Sev};
    use crate::fix::{Fixer, LnkJob, Status};
    use crate::paths::{join, norm};
    use crate::registry::{FakeRegistry, Registry, MACHINE_ENV, USER_ENV};
    use crate::sys::{link_kind, make_junction, read_link, LinkKind};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    pub const OLD_ROOT: &str = r"Q:\Portable\scoop"; // a previous root on a drive that does not exist

    fn write(path: &str, text: &str) {
        std::fs::create_dir_all(crate::paths::dirname(path)).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn write_utf16(path: &str, text: &str) {
        let mut b = vec![0xFF, 0xFE];
        b.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
        std::fs::create_dir_all(crate::paths::dirname(path)).unwrap();
        std::fs::write(path, b).unwrap();
    }

    pub struct Fake {
        pub base: String,
        pub root: String,
        pub apps: String,
        pub shims: String,
        pub persist: String,
        pub buckets: String,
        pub start_menu: String,
    }

    impl Fake {
        pub fn current(&self, name: &str) -> String {
            join(&join(&self.apps, name), "current")
        }

        fn app(&self, name: &str, version: &str, bucket: &str, manifest: Option<&str>, install: bool, link: bool) -> String {
            let vdir = join(&join(&self.apps, name), version);
            std::fs::create_dir_all(&vdir).unwrap();
            write(&join(&vdir, &format!("{name}.exe")), "");
            if install {
                write(&join(&vdir, "scoop-install.json"), &format!("{{\"architecture\": \"64bit\", \"bucket\": \"{bucket}\"}}"));
                let m = manifest.map(str::to_string).unwrap_or_else(|| format!("{{\"version\": \"{version}\"}}"));
                write(&join(&vdir, "scoop-manifest.json"), &m);
            }
            if link {
                make_junction(&self.current(name), &vdir).unwrap();
            }
            vdir
        }

        pub fn build(name: &str) -> Fake {
            let base = scratch(name).to_string_lossy().to_string();
            let root = join(&base, "scoop");
            let f = Fake {
                apps: join(&root, "apps"),
                shims: join(&root, "shims"),
                persist: join(&root, "persist"),
                buckets: join(&root, "buckets"),
                start_menu: join(&base, "Scoop Apps"),
                root,
                base,
            };
            std::fs::create_dir_all(&f.shims).unwrap();
            std::fs::create_dir_all(join(&f.buckets, r"main\.git")).unwrap();
            for m in ["good", "failed", "broken", "empty", "wrongpersist"] {
                write(&join(&f.buckets, &format!(r"main\bucket\{m}.json")), "{}");
            }
            std::fs::create_dir_all(join(&f.buckets, r"nogit\bucket")).unwrap();

            // healthy app with a persisted folder and file
            let good = f.app("good", "1.0", "main", Some(r#"{"version": "1.0", "persist": ["data", "conf.ini"]}"#), true, true);
            std::fs::create_dir_all(join(&f.persist, r"good\data")).unwrap();
            write(&join(&f.persist, r"good\conf.ini"), "x=1\n");
            make_junction(&join(&good, "data"), &join(&f.persist, r"good\data")).unwrap();
            std::fs::hard_link(join(&f.persist, r"good\conf.ini"), join(&good, "conf.ini")).unwrap();
            // config and .reg files still pointing at the old root
            write(&join(&good, r"etc\gitconfig"), "[credential]\n\thelper = Q:/Portable/scoop/apps/git/2.56.1/mingw64/bin/gcm.exe\n");
            write(
                &join(&good, "settings.json"),
                &format!(
                    "{{\"exe\": {}, \"gone\": {}}}",
                    crate::report::js(&join(&good, "good.exe")),
                    crate::report::js(&join(&good, "missing.dll"))
                ),
            );
            write_utf16(
                &join(&good, "install-context.reg"),
                "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\Classes\\*\\shell\\good\\command]\r\n\
                 @=\"\\\"Q:\\\\Portable\\\\scoop\\\\apps\\\\good\\\\current\\\\good.exe\\\" \\\"%1\\\"\"\r\n",
            );
            // 'current' junction to a removed version folder
            let broken = f.app("broken", "2.0", "main", None, true, true);
            std::fs::remove_dir_all(broken).unwrap();
            // 'current' copied as an empty folder
            f.app("empty", "3.0", "main", None, true, false);
            std::fs::create_dir_all(f.current("empty")).unwrap();
            // no 'current' at all
            std::fs::create_dir_all(join(&f.apps, r"nocurrent\1.0")).unwrap();
            // failed install
            f.app("failed", "1.0", "main", None, false, true);
            write(&join(&f.current("failed"), "readme.txt"), "x");
            // manifest gone from its bucket, and a bucket that is gone
            f.app("gone", "1.0", "main", None, true, true);
            f.app("orphan", "1.0", "extras", None, true, true);
            // persisted folder linked to the wrong place, and one missing
            let wp = f.app("wrongpersist", "1.0", "main", Some(r#"{"version": "1.0", "persist": [["cfg", "config"], "logs"]}"#), true, true);
            std::fs::create_dir_all(join(&f.base, "elsewhere")).unwrap();
            make_junction(&join(&wp, "cfg"), &join(&f.base, "elsewhere")).unwrap();
            std::fs::create_dir_all(join(&f.persist, r"wrongpersist\config")).unwrap();

            // shims
            let good_exe = join(&f.current("good"), "good.exe");
            let s = |n: &str, t: &str| write(&join(&f.shims, n), t);
            s("good.exe", "");
            s("good.shim", &format!("path = \"{good_exe}\"\n"));
            s("old.exe", "");
            s("old.shim", &format!("path = \"{OLD_ROOT}\\apps\\old\\current\\old.exe\"\n"));
            s("lost.exe", "");
            s("lost.shim", &format!("path = \"{}\"\n", join(&f.current("good"), "lost.exe")));
            s("lonely.exe", "");
            s("noexe.shim", &format!("path = \"{good_exe}\"\n"));
            s("good.shim.other", &format!("path = \"{OLD_ROOT}\\apps\\x\\x.exe\"\n"));
            s("oldcmd.cmd", &format!("@rem {OLD_ROOT}\\apps\\x\\current\\x.ps1\r\n@echo off\r\n"));
            s("goodcmd.cmd", &format!("@rem {good_exe}\r\n@\"{good_exe}\" %*\r\n"));
            s("rel.ps1", "$path = Join-Path \"$PSScriptRoot\" \"..\\apps\\good\\current\\nothere.ps1\"\n");
            s("goodsh", &format!("#!/bin/sh\n# {good_exe}\n\"{good_exe}\" \"$@\"\n"));
            s("oldsh", &format!("#!/bin/sh\n# {OLD_ROOT}\\apps\\x\\x.exe\n"));

            // Start menu shortcuts
            std::fs::create_dir_all(join(&f.start_menu, "Sub")).unwrap();
            let lnk = crate::lnk::build(&good_exe, Some(&f.current("good")), Some(&format!("{good_exe},0")), None);
            std::fs::write(join(&f.start_menu, "Good.lnk"), lnk).unwrap();
            std::fs::write(join(&f.start_menu, r"Sub\Old.lnk"), crate::lnk::build(&format!(r"{OLD_ROOT}\apps\x\current\x.exe"), None, None, None))
                .unwrap();
            std::fs::write(join(&f.start_menu, "Junk.lnk"), b"not a shortcut").unwrap();
            f
        }

        pub fn registry(&self) -> FakeRegistry {
            let r = FakeRegistry::new();
            let root = &self.root;
            let (uh, uk) = USER_ENV;
            r.set(uh, uk, "SCOOP", root, 1);
            r.set(uh, uk, "Path", &format!(r"{root}\shims;{OLD_ROOT}\shims;C:\Windows"), 2);
            r.set(uh, uk, "JAVA_HOME", &format!(r"{root}\apps\good\current"), 1);
            r.set(uh, uk, "GOROOT", &format!(r"{root}\persist\go\missing"), 1);
            r.set(uh, uk, "OLD_HOME", &format!(r"{OLD_ROOT}\apps\x\current"), 1);
            r.set(uh, uk, "NOT_A_PATH", "hello", 1);
            r.set(MACHINE_ENV.0, MACHINE_ENV.1, "Path", r"C:\Windows;C:\Does\Not\Exist", 1);
            let c = r"Software\Classes";
            r.set("HKCU", &format!(r"{c}\goodfile\shell\open\command"), "", &format!("\"{root}\\apps\\good\\current\\good.exe\" \"%1\""), 1);
            r.set("HKCU", &format!(r"{c}\oldfile\shell\open\command"), "", &format!("\"{OLD_ROOT}\\apps\\x\\x.exe\" \"%1\""), 1);
            r.set("HKCU", &format!(r"{c}\lostfile\DefaultIcon"), "", &format!(r"{root}\apps\good\current\lost.ico,0"), 1);
            r.set("HKCU", &format!(r"{c}\notscoop\DefaultIcon"), "", r"C:\Nowhere\x.ico", 1);
            let run = r"Software\Microsoft\Windows\CurrentVersion\Run";
            r.set("HKCU", run, "Gone", r"C:\Nowhere\run.exe --tray", 1);
            r.set("HKCU", run, "Ok", r"C:\Windows\notepad.exe", 1);
            r
        }

        pub fn context<'a>(&self, reg: &'a FakeRegistry) -> Context<'a> {
            let home = join(&self.base, "home");
            write(&join(&home, r".config\scoop\config.json"), &format!("{{\"root_path\": {}}}", crate::report::js(&self.root)));
            let env: HashMap<String, String> = [("ProgramData".to_string(), join(&self.base, "pd"))].into();
            Context::new(find_roots(reg, &env, &home, None), reg)
        }
    }

    fn codes(r: &CheckResult, part: &str) -> Vec<String> {
        r.problems.iter().filter(|p| p.item.to_lowercase().contains(&part.to_lowercase())).map(|p| p.code.clone()).collect()
    }

    #[test]
    fn checks_on_a_fake_tree() {
        let f = Fake::build("checks");
        let reg = f.registry();
        let ctx = f.context(&reg);
        assert_eq!(norm(&ctx.root), norm(&f.root));
        assert_eq!(ctx.roots.root_source, "env_user");
        assert!(checks::check_roots(&ctx).problems.is_empty());

        let r = checks::check_shims(&ctx);
        assert!(r.problems.iter().all(|p| !p.item.ends_with("good.shim")));
        assert!(codes(&r, r"\good.exe").is_empty());
        assert!(codes(&r, "goodcmd").is_empty());
        assert!(codes(&r, "goodsh").is_empty());
        assert!(codes(&r, "old.shim").contains(&"other_root".into()));
        assert!(codes(&r, "lost.shim").contains(&"missing".into()));
        assert!(codes(&r, "lonely.exe").contains(&"exe_no_shim".into()));
        assert!(codes(&r, "noexe.shim").contains(&"shim_no_exe".into()));
        assert!(codes(&r, "oldcmd.cmd").contains(&"other_root".into()));
        assert!(codes(&r, "rel.ps1").contains(&"missing".into()));
        assert!(codes(&r, "oldsh").contains(&"other_root".into()));
        let alt: Vec<Sev> = r.problems.iter().filter(|p| p.item.ends_with("good.shim.other")).map(|p| p.severity).collect();
        assert_eq!(alt, vec![Sev::Warning]);
        let old = r.problems.iter().find(|p| p.item.ends_with("old.shim")).unwrap();
        assert_eq!(norm(old.expected.as_deref().unwrap()), norm(&format!(r"{}\apps\old\current\old.exe", f.root)));
        assert!(ctx.discovered.lock().unwrap().contains_key(&norm(OLD_ROOT)));

        let r = checks::check_current(&ctx);
        assert!(codes(&r, r"\good\").is_empty());
        assert!(codes(&r, r"\broken\").contains(&"link_broken".into()));
        assert!(codes(&r, r"\empty\").contains(&"current_empty".into()));
        assert!(codes(&r, "nocurrent").contains(&"no_current".into()));
        let e = r.problems.iter().find(|p| p.code == "current_empty").unwrap();
        assert!(e.expected.as_deref().unwrap().ends_with("3.0"));

        let r = checks::check_persist(&ctx);
        assert!(codes(&r, r"\good\").is_empty(), "{:?}", r.problems);
        assert!(codes(&r, r"\cfg").contains(&"link_wrong".into()));
        assert!(codes(&r, r"\logs").contains(&"persist_missing".into()));

        let r = checks::check_install(&ctx);
        assert!(codes(&r, r"\failed").contains(&"install_failed".into()));
        assert!(codes(&r, r"\gone").contains(&"manifest_gone".into()));
        assert!(codes(&r, "extras").contains(&"bucket_missing".into()));
        assert!(codes(&r, "nogit").contains(&"bucket_not_git".into()));
        assert!(codes(&r, r"\good").is_empty());
        assert!(codes(&r, r"\empty").is_empty());

        let r = checks::check_shortcuts(&ctx, Some(vec![f.start_menu.clone()]));
        assert_eq!(r.checked, 3);
        assert!(codes(&r, "Good.lnk").is_empty(), "{:?}", r.problems);
        assert!(codes(&r, "Old.lnk").contains(&"other_root".into()));
        assert!(codes(&r, "Junk.lnk").contains(&"lnk_unreadable".into()));

        let r = checks::check_regfiles(&ctx);
        assert_eq!(r.checked, 1);
        assert!(codes(&r, "install-context.reg").contains(&"other_root".into()));

        let r = checks::check_registry(&ctx);
        assert!(codes(&r, "goodfile").is_empty());
        assert!(codes(&r, "oldfile").contains(&"other_root".into()));
        assert!(codes(&r, "lostfile").contains(&"missing".into()));
        assert!(codes(&r, "notscoop").is_empty());
        assert!(codes(&r, "[Gone]").contains(&"missing".into()));
        assert!(codes(&r, "[Ok]").is_empty());

        let r = checks::check_env(&ctx);
        assert!(codes(&r, "Path (user)").contains(&"other_root".into()));
        assert!(codes(&r, "Path (machine)").contains(&"missing".into()));
        assert!(codes(&r, "JAVA_HOME").is_empty());
        assert!(codes(&r, "GOROOT").contains(&"missing".into()));
        assert!(codes(&r, "OLD_HOME").contains(&"other_root".into()));
        assert!(codes(&r, "NOT_A_PATH").is_empty());
        assert!(!codes(&r, "").contains(&"shims_not_in_path".into()));

        let r = checks::check_configs(&ctx);
        assert!(codes(&r, "gitconfig").contains(&"other_root".into()));
        assert!(codes(&r, "settings.json").contains(&"missing".into()));
        let git = r.problems.iter().find(|p| p.item.contains("gitconfig")).unwrap();
        assert_eq!(norm(git.expected.as_deref().unwrap()), norm(&format!(r"{}\apps\git\2.56.1\mingw64\bin\gcm.exe", f.root)));

        let mut ctx = f.context(&reg);
        ctx.config_depth = 0;
        ctx.persist_depth = 0;
        assert!(codes(&checks::check_configs(&ctx), "gitconfig").is_empty());

        let ctx = f.context(&reg).with_old_roots(&[r"Z:\Old\Place".to_string()]);
        assert!(matches!(ctx.classify(r"Z:\Old\Place\bin\x.exe"), checks::Class::Other(b) if b == r"Z:\Old\Place"));
        let _ = std::fs::remove_dir_all(&f.base);
    }

    #[test]
    fn configs_from_the_cache() {
        let f = Fake::build("cachecfg");
        let reg = f.registry();
        let db = std::path::PathBuf::from(&f.base).join("cache.db");
        let mut ctx = f.context(&reg);
        ctx.cache = crate::cache::Cache::load(&db);
        let first = checks::check_configs(&ctx);
        ctx.cache.save().unwrap();
        let mut ctx = f.context(&reg);
        ctx.cache = crate::cache::Cache::load(&db);
        let second = checks::check_configs(&ctx);
        assert!(ctx.cache.hits.load(std::sync::atomic::Ordering::Relaxed) > 0);
        assert_eq!(ctx.cache.misses.load(std::sync::atomic::Ordering::Relaxed), 0);
        let items = |r: &CheckResult| r.problems.iter().map(|p| (p.item.clone(), p.code.clone())).collect::<Vec<_>>();
        assert_eq!(items(&first), items(&second));
        let _ = std::fs::remove_dir_all(&f.base);
    }

    fn all_results(ctx: &Context, f: &Fake) -> Vec<CheckResult> {
        crate::fix::FIX_ORDER
            .iter()
            .map(|id| if *id == "shortcuts" { checks::check_shortcuts(ctx, Some(vec![f.start_menu.clone()])) } else { checks::run_check(ctx, id) })
            .collect()
    }

    type Jobs = Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>;

    fn runner(jobs: Jobs) -> crate::fix::PowerShellRunner {
        Box::new(move |js: &[LnkJob]| {
            let mut out = HashMap::new();
            for j in js {
                jobs.lock().unwrap().push((j.path.clone(), j.pairs.clone()));
                out.insert(j.path.clone(), None);
            }
            Ok(out)
        })
    }

    #[test]
    fn fix_dry_run_changes_nothing() {
        let f = Fake::build("fixdry");
        let reg = f.registry();
        let ctx = f.context(&reg);
        let before = std::fs::read_to_string(join(&f.shims, "old.shim")).unwrap();
        let jobs: Jobs = Arc::default();
        let backup = join(&f.base, "backup");
        let (log, _, _) = Fixer::new(&ctx, true, Some(&backup), Some(runner(jobs.clone()))).run(&all_results(&ctx, &f), None);
        assert!(log.count(Status::Planned) > 0);
        assert_eq!(log.count(Status::Done), 0);
        assert_eq!(std::fs::read_to_string(join(&f.shims, "old.shim")).unwrap(), before);
        assert_eq!(link_kind(&f.current("empty")), Some(LinkKind::Dir));
        assert!(jobs.lock().unwrap().is_empty());
        assert!(!std::path::Path::new(&backup).exists());
        let _ = std::fs::remove_dir_all(&f.base);
    }

    #[test]
    fn fix_repairs() {
        let f = Fake::build("fix");
        let reg = f.registry();
        let ctx = f.context(&reg);
        let jobs: Jobs = Arc::default();
        let (log, backup_dir, _) =
            Fixer::new(&ctx, false, Some(&join(&f.base, "backup")), Some(runner(jobs.clone()))).run(&all_results(&ctx, &f), None);
        let failed: Vec<_> = log.entries.iter().filter(|e| e.status == Status::Failed).map(|e| (&e.item, &e.args)).collect();
        assert!(failed.is_empty(), "{failed:?}");
        let root = &f.root;

        assert_eq!(link_kind(&f.current("empty")), Some(LinkKind::Junction));
        assert!(read_link(&f.current("empty")).unwrap().ends_with("3.0"));
        let cfg = join(&f.current("wrongpersist"), "cfg");
        assert_eq!(norm(&read_link(&cfg).unwrap()), norm(&join(&f.persist, r"wrongpersist\config")));
        assert!(std::path::Path::new(&join(&f.base, "elsewhere")).is_dir());

        let read = |p: String| std::fs::read_to_string(p).unwrap();
        assert!(read(join(&f.shims, "old.shim")).contains(root.as_str()));
        assert!(!read(join(&f.shims, "oldcmd.cmd")).contains(OLD_ROOT));
        assert!(read(join(&f.current("good"), r"etc\gitconfig")).contains(&format!("{}/apps/git/2.56.1", root.replace('\\', "/"))));
        let raw = std::fs::read(join(&f.current("good"), "install-context.reg")).unwrap();
        assert!(raw.starts_with(&[0xFF, 0xFE]));
        let units = crate::paths::u16s(&raw[2..], false);
        assert!(String::from_utf16(&units).unwrap().contains(&root.replace('\\', "\\\\")));

        let mut backups = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(&backup_dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else {
                    backups.push(e.file_name().to_string_lossy().to_string());
                }
            }
        }
        for name in ["old.shim", "gitconfig", "registry.json"] {
            assert!(backups.contains(&name.to_string()), "{name}: {backups:?}");
        }

        let jobs = jobs.lock().unwrap();
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].0.ends_with("Old.lnk"));
        assert_eq!(jobs[0].1[0].1, *root);

        let (v, _) = reg.get("HKCU", r"Software\Classes\oldfile\shell\open\command", "").unwrap();
        assert!(v.contains(root.as_str()));
        let (path, vtype) = reg.get("HKCU", "Environment", "Path").unwrap();
        assert_eq!(vtype, 2);
        assert!(!path.contains(OLD_ROOT));
        assert!(!reg.get("HKCU", "Environment", "OLD_HOME").unwrap().0.contains(OLD_ROOT));

        // a second check finds no more old-root problems in the repaired places
        let ctx = f.context(&reg);
        for r in all_results(&ctx, &f) {
            if ["shims", "regfiles", "configs", "registry", "env"].contains(&r.id) {
                assert!(r.problems.iter().all(|p| !p.code.starts_with("other_root")), "{}: {:?}", r.id, r.problems);
            }
            if r.id == "current" {
                assert!(r.problems.iter().all(|p| p.code != "current_empty"));
            }
        }
        let _ = std::fs::remove_dir_all(&f.base);
    }

    #[test]
    fn fix_only_some_checks() {
        let f = Fake::build("fixonly");
        let reg = f.registry();
        let ctx = f.context(&reg);
        let (log, _, _) = Fixer::new(&ctx, false, Some(&join(&f.base, "backup")), Some(runner(Arc::default())))
            .run(&all_results(&ctx, &f), Some(&["current".to_string()]));
        assert!(log.entries.iter().all(|e| e.check == "current"));
        assert!(std::fs::read_to_string(join(&f.shims, "old.shim")).unwrap().contains(OLD_ROOT));
        let _ = std::fs::remove_dir_all(&f.base);
    }

    fn run_cli(args: &[&str], reg: &FakeRegistry) -> (i32, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::cli::run(&args, reg, HashMap::new(), "", &mut out, &mut err, None);
        (code, String::from_utf8(out).unwrap())
    }

    #[test]
    fn cli_json_and_text() {
        let f = Fake::build("cli");
        let reg = f.registry();
        let base = ["--root", f.root.as_str(), "--old-root", OLD_ROOT, "-q", "--no-cache", "--skip", "shortcuts,configs"];
        let (code, out) = run_cli(&[&base[..], &["--json"]].concat(), &reg);
        assert_eq!(code, 1);
        let j = rt_core::json::parse(&out).unwrap();
        assert_eq!(norm(j.get("root").str()), norm(&f.root));
        assert!(j.get("other_roots").items().iter().any(|o| norm(o.get("path").str()) == norm(OLD_ROOT)));
        assert!(j.get("summary").get("errors").num().unwrap() > 0.0);
        assert!(j.get("checks").items().iter().all(|c| c.get("id").str() != "configs"));

        let (_, text) = run_cli(&[&base[..], &["--lang", "pt"]].concat(), &reg);
        assert!(text.contains("Verificação do Scoop"));
        assert!(text.contains("Resumo:"));
        assert!(text.contains(OLD_ROOT));

        assert_eq!(run_cli(&["--checks", "nope"], &reg).0, 2);
        assert_eq!(run_cli(&["--bogus"], &reg).0, 2);
        assert_eq!(run_cli(&["--help"], &reg).0, 0);
        let _ = std::fs::remove_dir_all(&f.base);
    }
}
