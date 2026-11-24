//! Pictures of the window to look at: `cargo test -p rcare --test screens -- --ignored`
//! writes `apps/rcare/target/screens/*.png` (each page with the sidebar,
//! light and dark, open and collapsed, wide and small).

use rcare::pages::{index_of, PAGES};
use rcare::sidebar::{self, Hit, State};
use rclean::app::App;
use rclean::page::CleanPage;
use rclean::targets::{all, Env};
use rclean::ui::gfx::{Gfx, Rect};
use rt_ui::theme::Theme;
use std::path::PathBuf;
use std::sync::Arc;

/// A frame: the page drawn by `page` in the area right of the sidebar, with the page `id` selected.
fn shot(
    id: &str,
    dark: bool,
    collapsed: bool,
    hot: Option<Hit>,
    w: u32,
    h: u32,
    page: &mut dyn FnMut(&mut Gfx, Rect, &Theme),
) -> tiny_skia::Pixmap {
    rcare::i18n::set_lang(rcare::i18n::Lang::EnUS);
    let mut g = Gfx::new().expect("fonts");
    g.resize(w, h);
    let th = Theme::new(dark, None);
    let s = 1.0;
    let mut rest = Rect::new(0.0, 0.0, w as f32, h as f32);
    let side = rest.take_left(sidebar::width(collapsed, s));
    let l = sidebar::layout(side, PAGES.len(), s, sidebar::scroll_to(&sidebar::layout(side, PAGES.len(), s, 0), 0, index_of(id)));
    page(&mut g, rest, &th);
    sidebar::draw(&mut g, &l, &th, s, State { collapsed, selected: index_of(id), hot }, PAGES);
    let mut buf = vec![0u32; (w * h) as usize];
    g.copy_to(&mut buf);
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    for (d, p) in pm.data_mut().chunks_mut(4).zip(buf) {
        d.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]);
    }
    pm
}

fn clean(g: &mut Gfx, r: Rect, th: &Theme) {
    let mut page = CleanPage::new(App::new(all(&Env::default()), false, &[]), false, std::env::temp_dir(), Arc::new(|_| {}));
    page.render(g, r, th, 1.0, false);
}

fn dups(g: &mut Gfx, r: Rect, th: &Theme) {
    let mut page = rdup::page::DupPage::new(&rdup::settings::Settings::default(), &[], None, false, Arc::new(|_| {}));
    page.render(g, r, th, 1.0, false);
}

fn size(g: &mut Gfx, r: Rect, th: &Theme) {
    let mut page = rsize::page::SizePage::new(rsize::settings::Settings::default(), false, Arc::new(|_| {}));
    page.render(g, r, th, 1.0, false);
}

fn inst(g: &mut Gfx, r: Rect, th: &Theme) {
    let mut page = rinst::page::InstPage::new(&rinst::settings::Settings::default(), false, std::env::temp_dir(), Arc::new(|_| {}));
    page.render(g, r, th, 1.0, false);
}

/// Custom Clean with the three parts at different stages, the pointer on Analyze.
fn custom(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::custom::{CustomPage, Status};
    let mut page = CustomPage::default();
    page.view.folder = "C:\\Users\\Miguel".into();
    page.view.can_analyze = true;
    page.view.status =
        [Status::Done { bytes: 4_812_000_000, count: 23_410 }, Status::Done { bytes: 1_250_000_000, count: 87 }, Status::Working];
    page.render(g, r, th, 1.0);
    page.pointer(page.layout.analyze.x + 4.0, page.layout.analyze.y + 4.0, false);
    page.render(g, r, th, 1.0);
}

/// System Maintenance with its rows at different stages, the pointer on a row's button.
fn maint(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::custom::Status;
    use rcare::maint::MaintPage;
    let mut page = MaintPage::default();
    page.view.status = [
        Status::Done { bytes: 4_812_000_000, count: 23_410 },
        Status::Done { bytes: 0, count: 137 },
        Status::Done { bytes: 0, count: 0 },
        Status::Done { bytes: 0, count: 7 },
        Status::Working,
        Status::Done { bytes: 0, count: 1 },
    ];
    page.render(g, r, th, 1.0);
    let b = page.layout.buttons[2];
    page.pointer(b.x + 4.0, b.y + 4.0, false);
    page.render(g, r, th, 1.0);
}

/// The Startup Optimizer with a few items, one off, one gone, one of every user.
fn startup(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::startup::{Entry, Place};
    use rcare::startup_page::StartupPage;
    let e = |name: &str, place: Place, enabled: bool, missing: bool, cmd: &str| Entry {
        name: name.into(),
        command: cmd.into(),
        file: Some(std::path::PathBuf::from(cmd)),
        place,
        enabled,
        missing,
    };
    let mut page = StartupPage::new(false);
    page.entries = vec![
        e("Bdagent", Place::MachineRun, true, false, "C:\\Program Files\\Bitdefender\\Bitdefender Security App\\bdagent.exe"),
        e(
            "BraveSoftware Update",
            Place::UserRun,
            false,
            false,
            "C:\\Users\\Miguel\\AppData\\Local\\BraveSoftware\\Update\\BraveUpdateCore.exe",
        ),
        e("OneDrive", Place::UserRun, true, false, "\"C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe\" /background"),
        e("Old Tool.lnk", Place::UserFolder, true, true, "D:\\Tools\\old\\tool.exe"),
        e("rwall", Place::UserRun, true, false, "\"C:\\Users\\Miguel\\AppData\\Local\\Programs\\rust-tools\\rwall.exe\" --tray"),
    ];
    page.sel = Some(2);
    page.render(g, r, th, 1.0);
}

/// Speed Up with a few programs open, one ticked.
fn speed(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::speed::{Memory, OpenApp};
    use rcare::speed_page::SpeedPage;
    let a = |pid: u32, exe: &str, title: &str, mb: u64| OpenApp {
        pid,
        exe: exe.into(),
        title: title.into(),
        memory: mb << 20,
        windows: vec![],
    };
    let mut page = SpeedPage::new(true);
    page.memory = Some(Memory { total: 34_267_713_536, available: 17_337_028_608, standby: Some(14_248_808_448) });
    page.cpu = Some(21.0);
    page.game_mode = true;
    page.apps = vec![
        a(1, "wotblitz.exe", "WoT Blitz", 903),
        a(2, "chrome.exe", "DAZN | Home - Google Chrome", 279),
        a(3, "EXCEL.EXE", "scoop_classificados.xlsx - Excel", 173),
        a(4, "notepad.exe", "*Untitled - Notepad", 5),
    ];
    page.ticked = vec![3];
    page.sel = Some(2);
    page.render(g, r, th, 1.0);
}

/// The Program Deactivator with a few services, one turned off by rcare.
fn services(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::services::{Service, Start};
    use rcare::services_page::ServicesPage;
    let e = |name: &str, company: &str, start: Start, running: bool, cmd: &str| Service {
        name: name.into(),
        display: name.into(),
        company: Some(company.into()),
        command: cmd.into(),
        file: None,
        start,
        running,
        was: (start == Start::Manual).then_some(Start::Auto),
    };
    let mut page = ServicesPage::new(true, std::env::temp_dir());
    page.services = vec![
        e("Bitdefender Virus Shield", "Bitdefender", Start::Auto, true, "\"C:\\Program Files\\Bitdefender\\Bitdefender Security\\bdservicehost.exe\""),
        e("Everything", "voidtools", Start::Auto, true, "\"C:\\Programs\\Everything\\Everything.exe\" -svc"),
        e("Google Updater Service", "Google LLC", Start::Manual, false, "\"C:\\Program Files (x86)\\Google\\GoogleUpdater\\updater.exe\" --system --windows-service"),
        e("NVIDIA Display Container LS", "NVIDIA Corporation", Start::Auto, true, "C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\nv_dispi.inf_amd64\\Display.NvContainer\\NVDisplay.Container.exe -s NVDisplay.ContainerLocalSystem"),
        e("rust-tools service", "–", Start::Delayed, true, "\"C:\\Programs\\rust-tools\\rtsvc.exe\" run"),
    ];
    page.sel = Some(1);
    page.render(g, r, th, 1.0);
}

/// The Extensions page with a few extensions of two browsers.
fn extensions(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::extensions::{Browser, Extension, Source};
    use rcare::extensions_page::ExtensionsPage;
    let e = |name: &str, browser: Browser, profile: &str, enabled: bool, source: Source| Extension {
        id: name.to_lowercase(),
        name: name.into(),
        version: "1.0".into(),
        browser,
        profile_dir: "Default".into(),
        profile: profile.into(),
        enabled,
        source,
        folder: None,
    };
    let mut page = ExtensionsPage::default();
    page.list = vec![
        e("Bitdefender Anti-tracker", Browser::Chrome, "Miguel", true, Source::Program),
        e("Bitwarden Password Manager", Browser::Chrome, "Miguel", true, Source::Store),
        e("Dark Reader", Browser::Chrome, "Miguel", false, Source::Store),
        e("Google Docs Offline", Browser::Chrome, "Miguel", true, Source::Store),
        e("Tampermonkey", Browser::Chrome, "Work", false, Source::Store),
        e("Bitwarden Password Manager", Browser::Brave, "Personal", true, Source::Store),
        e("uBlock Origin", Browser::Firefox, "default-release", true, Source::Store),
    ];
    page.read = true;
    page.sel = Some(2);
    page.render(g, r, th, 1.0);
}

/// The Registry Cleaner after a scan, a category selected with a few entries.
fn registry(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::registry_page::RegistryPage;
    use rt_sys::regfile::Entry;
    let mut page = RegistryPage::new(false, std::env::temp_dir());
    let e = |key: &str, value: Option<&str>, why: &str| (Entry { key: key.into(), value: value.map(str::to_string) }, why.to_string());
    for c in &mut page.cats {
        c.found = Some(Vec::new());
    }
    let at = |p: &RegistryPage, id: &str| p.cats.iter().position(|c| c.id == id).unwrap();
    let run = at(&page, "reg.run");
    page.cats[run].found = Some(vec![
        e(
            "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            Some("OldSync"),
            "OldSync → C:\\Program Files\\OldSync\\sync.exe",
        ),
        e("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", Some("Updater"), "Updater → D:\\Tools\\updater\\upd.exe"),
    ]);
    let mui = at(&page, "reg.muicache");
    page.cats[mui].found = Some(vec![e(
        "HKEY_CURRENT_USER\\Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache",
        Some("C:\\Games\\old.exe.FriendlyAppName"),
        "C:\\Games\\old.exe",
    )]);
    let com = at(&page, "report.com");
    page.cats[com].found = Some(vec![e(
        "HKEY_LOCAL_MACHINE\\Software\\Classes\\CLSID\\{A530D54A-DBA0-4b17-9F99-51A7A2CC17CA}",
        None,
        "Tethering Setting Handler → C:\\Windows\\System32\\TetheringSettingHandler.dll",
    )]);
    page.sel = run;
    page.render(g, r, th, 1.0);
}

/// Protect System as this PC reported it (two antivirus programs on, Defender off), one firewall profile off.
fn protect(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::protect_page::ProtectPage;
    let mut page = ProtectPage::new(false);
    page.status = Some(rcare::protect::parse(
        "av\tIObit Malware Fighter\t266240\nav\tBitdefender Antivirus\t266240\nfw\tBitdefender Firewall\t266240\n\
mp\tNot running\tFalse\t65535\t4294967295\tFalse\nprofile\tDomain\tTrue\nprofile\tPrivate\tTrue\nprofile\tPublic\tFalse\n",
    ));
    page.render(g, r, th, 1.0);
}

/// Protect System with Defender as the antivirus.
fn protect_defender(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::protect_page::ProtectPage;
    let mut page = ProtectPage::new(true);
    page.status = Some(rcare::protect::parse(
        "mp\tNormal\tTrue\t1\t12\tTrue\ncfa\t0\nprofile\tDomain\tTrue\nprofile\tPrivate\tTrue\nprofile\tPublic\tTrue\n",
    ));
    page.render(g, r, th, 1.0);
}

/// Internet Booster: auto-tuning changed by another tool, two connections (one on Cloudflare), the first selected.
fn net(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::net_page::NetPage;
    let mut page = NetPage::new(true, std::env::temp_dir());
    page.net = Some(rcare::net::parse(
        "tcp\tDisabled\tDisabled\tDisabled\t1000\tCUBIC\tEnabled\tEnabled\n\
dns\t14\tEthernet\tIntel(R) Ethernet Connection (2) I219-V\tFalse\t192.168.1.1\t2a01:11:8020:f870::1\n\
dns\t7\tWi-Fi\tIntel(R) Wi-Fi 6 AX201\tTrue\t1.1.1.1,1.0.0.1\t2606:4700:4700::1111,2606:4700:4700::1001\n",
    ));
    page.render(g, r, th, 1.0);
}

/// Privacy Sweep after a scan of this PC's numbers, the clipboard ticked too.
fn privacy(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::privacy::Found;
    use rcare::privacy_page::PrivacyPage;
    let mut page = PrivacyPage::new(std::env::temp_dir());
    for (row, n) in page.rows.iter_mut().zip([114, 2, 0, 21, 32, 0]) {
        row.found = Some(Found { count: n, clipboard: row.trace.id == "clipboard", ..Found::default() });
    }
    page.rows[5].ticked = true;
    page.sel = 1;
    page.render(g, r, th, 1.0);
}

/// System Information as this PC reported it, a drive with a warning added.
fn sysinfo(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::sysinfo_page::SysPage;
    let mut page = SysPage::new(true, std::env::temp_dir().join("rcare-screens-sys"));
    page.info = Some(rcare::sysinfo::parse(
        "os\tMicrosoft Windows 10 Pro N\t22H2\t19045.6466\t64-bit\t2026-10-14\t38\n\
pc\tMCDESK1\tSystem manufacturer\tSystem Product Name\tASUSTeK COMPUTER INC. MAXIMUS VIII HERO\t1402 2016-01-07\t34267713536\n\
cpu\tIntel(R) Core(TM) i7-6700K CPU @ 4.00GHz\t4\t8\t4001\n\
gpu\tNVIDIA GeForce GTX 960\t32.0.15.8278\n\
disk\tSamsung SSD 970 EVO 1TB\tSSD\tNVMe\t1000204886016\tHealthy\n\
disk\tST3500418AS\tHDD\tSATA\t500107862016\tWarning\n\
disk\tST1000DX001-1NS162\tHDD\tSATA\t1000204886016\tHealthy\n\
device\tGenesys Logic USB2.0 Card Reader\tUSB\tCM_PROB_FAILED_START\n\
fast\t1\n",
    ));
    page.render(g, r, th, 1.0);
}

/// Repair while SFC runs (its progress), after a chkdsk scan of E: that found errors.
fn repair(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::repair::Task;
    use rcare::repair_page::RepairPage;
    let mut page = RepairPage::new(true);
    page.drives = Some(vec!['C', 'E', 'S']);
    page.results.insert(Task::Check('E'), Ok(3));
    page.results.insert(Task::Check('C'), Ok(0));
    page.results.insert(Task::Dism, Ok(0));
    page.output.feed(
        "> sfc /scannow\n\nBeginning system scan.  This process will take some time.\r\n\r\n\
Beginning verification phase of system scan.\r\nVerification 37% complete.\r",
    );
    page.sel = 3;
    page.render(g, r, th, 1.0);
}

/// The Scheduler with two schedules and a few runs.
fn scheduler(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::schedule::{Freq, Op, Run, Schedule, TaskStatus};
    use rcare::schedule_page::SchedulePage;
    let mut page = SchedulePage::new(true, std::env::temp_dir());
    page.list = vec![
        Schedule { name: "Schedule 1".into(), ops: vec![Op::Sfc, Op::Clean], freq: Freq::Weekly(6), hour: 3, minute: 0 },
        Schedule { name: "Schedule 2".into(), ops: vec![Op::Dism, Op::Chkdsk], freq: Freq::Monthly(1), hour: 2, minute: 30 },
    ];
    page.status.insert(
        "Schedule 1".into(),
        TaskStatus { state: "Ready".into(), next: "2026-11-29T03:00:00".into(), last: "2026-11-22T03:00:00".into(), result: Some(0) },
    );
    page.status.insert(
        "Schedule 2".into(),
        TaskStatus { state: "Ready".into(), next: "2026-12-01T02:30:00".into(), last: String::new(), result: None },
    );
    let run = |when: &str, op: Op, ok: bool, d: &str| Run { when: when.into(), op, ok, detail: d.into() };
    page.runs = vec![
        run("2026-11-22 03:21", Op::Clean, true, "1.84 GB freed in 12,402 files, 7 in use"),
        run("2026-11-22 03:19", Op::Sfc, true, "finished"),
        run("2026-11-15 03:24", Op::Sfc, false, "ended with code 1"),
    ];
    page.sel = Some(0);
    page.render(g, r, th, 1.0);
}

/// File Tools: empty folders found under a folder, and files waiting to be shredded.
fn files(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::files_page::{Empty, FilesPage};
    let mut page = FilesPage::default();
    let root = PathBuf::from(r"D:\Projects");
    page.root = Some(root.clone());
    page.empties = Some(
        [("old-site\\assets\\fonts", 1), ("scratch", 4), ("tools\\build\\obj", 2), ("tools\\cache", 1)]
            .iter()
            .map(|(p, n)| Empty { path: root.join(p), count: *n, ticked: true })
            .collect(),
    );
    page.items = vec![PathBuf::from(r"C:\Users\Ana\Documents\bank-statement-2026.pdf"), PathBuf::from(r"C:\Users\Ana\Desktop\old-keys")];
    page.render(g, r, th, 1.0);
}

/// Context Menu Manager: commands and shell extensions, one turned off, one needing administrator rights.
fn ctxmenu(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::ctxmenu::{How, Item};
    use rcare::ctxmenu_page::CtxMenuPage;
    let mut page = CtxMenuPage::new(false);
    let it = |name: &str, places: Vec<&'static str>, how: How, on: bool, company: Option<&str>, prog: &str| Item {
        name: name.into(),
        program: Some(PathBuf::from(prog)),
        places,
        how,
        enabled: on,
        missing: false,
        company: company.map(String::from),
    };
    let verb = |k: &str| How::Verb(vec![(false, k.into())]);
    let ext = How::Handler("{23170F69-40C1-278A-1000-000100020000}".into());
    page.items = vec![
        it("7-Zip", vec!["Files", "Folders"], ext.clone(), true, Some("Igor Pavlov"), r"C:\Program Files\7-Zip\7-zip.dll"),
        it("Open Git Bash here", vec!["Folders", "Folder background"], verb("a"), true, None, r"C:\Program Files\Git\git-bash.exe"),
        it(
            "Open with Code",
            vec!["Files", "Folders", "Folder background"],
            verb("b"),
            false,
            Some("Microsoft Corporation"),
            r"C:\Users\Ana\AppData\Local\Programs\Microsoft VS Code\Code.exe",
        ),
        it(
            "Open with Notepad++",
            vec!["Files"],
            verb("c"),
            true,
            Some("Don HO don.h@free.fr"),
            r"C:\Program Files\Notepad++\notepad++.exe",
        ),
        it(
            "Scan with Antivirus",
            vec!["Files", "Folders", "Drives"],
            ext,
            true,
            Some("Example Security"),
            r"C:\Program Files\Example\shellext.dll",
        ),
    ];
    page.read = true;
    page.sel = Some(2);
    page.render(g, r, th, 1.0);
}

/// Processes: a restart planned, a few processes (two readings, so the processor use shows), one selected.
fn procs(g: &mut Gfx, r: Rect, th: &Theme) {
    use rcare::processes::{Planned, Power, Proc};
    use rcare::processes_page::ProcessesPage;
    let mut page = ProcessesPage::new(&std::env::temp_dir().join("rcare-screens-procs"));
    let cpus = u64::from(rcare::processes::cpus());
    let list = |t: u64| -> Vec<Proc> {
        let p = |pid: u32, name: &str, path: Option<&str>, mem: u64, share: f64| Proc {
            pid,
            name: name.into(),
            path: path.map(PathBuf::from),
            memory: mem << 20,
            cpu_time: (t as f64 * share * cpus as f64) as u64,
            created: 1,
        };
        vec![
            p(4, "System", None, 1, 0.012),
            p(612, "csrss.exe", None, 6, 0.001),
            p(4120, "explorer.exe", Some(r"C:\Windows\explorer.exe"), 182, 0.008),
            p(7311, "Code.exe", Some(r"C:\Users\Ana\AppData\Local\Programs\Microsoft VS Code\Code.exe"), 640, 0.163),
            p(9024, "firefox.exe", Some(r"C:\Program Files\Mozilla Firefox\firefox.exe"), 1210, 0.071),
            p(10220, "rcare.exe", Some(r"C:\Users\Ana\scoop\apps\rcare\current\rcare.exe"), 38, 0.004),
            p(11872, "OneDrive.exe", Some(r"C:\Program Files\Microsoft OneDrive\OneDrive.exe"), 96, 0.0),
            p(12004, "notepad.exe", Some(r"C:\Windows\System32\notepad.exe"), 22, 0.0),
        ]
    };
    page.take(list(0), None);
    page.take(list(20_000_000), Some(std::time::Duration::from_secs(2)));
    page.sel = Some(9024);
    page.power = Power::Restart;
    page.planned = Some(Planned { power: Power::Restart, at: rcare::processes::unix_now() + 84 * 60 });
    page.render(g, r, th, 1.0);
}

/// A question in the window's theme (before SFC starts), over the Repair page.
fn question(g: &mut Gfx, r: Rect, th: &Theme) {
    repair(g, r, th);
    let mut q = rt_ui::message::Message::question(
        "Repair",
        "SFC checks every file of Windows: it takes 5 to 20 minutes, and the computer may be slower meanwhile. Start it now?\n\n\
You can keep using the computer and the other pages meanwhile: the result shows here when it ends.",
        "Yes",
        "No",
    );
    q.draw(g, r, &th.dialog_style(), 1.0);
}

#[test]
#[ignore]
fn screenshots() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/screens");
    std::fs::create_dir_all(&dir).unwrap();
    let save = |name: &str, pm: tiny_skia::Pixmap| pm.save_png(dir.join(name)).unwrap();
    save("open-light.png", shot("clean", false, false, Some(Hit::Page(index_of("custom"))), 1180, 720, &mut clean));
    save("open-dark.png", shot("clean", true, false, None, 1180, 720, &mut clean));
    save("collapsed-dark.png", shot("clean", true, true, Some(Hit::Toggle), 1180, 720, &mut clean));
    save("dups-light.png", shot("dups", false, false, None, 1180, 720, &mut dups));
    save("size-dark.png", shot("size", true, false, None, 1180, 720, &mut size));
    save("inst-light.png", shot("inst", false, false, None, 1180, 720, &mut inst));
    save("custom-light.png", shot("custom", false, false, None, 1280, 720, &mut custom));
    save("custom-dark-narrow.png", shot("custom", true, false, None, 800, 720, &mut custom));
    save("custom-smallest.png", shot("custom", false, false, None, 760, 480, &mut custom));
    save("maint-light.png", shot("maint", false, false, None, 1280, 720, &mut maint));
    save("maint-dark-smallest.png", shot("maint", true, false, None, 760, 480, &mut maint));
    save("startup-light.png", shot("startup", false, false, None, 1280, 720, &mut startup));
    save("startup-dark-smallest.png", shot("startup", true, false, None, 760, 480, &mut startup));
    save("speed-light.png", shot("speed", false, false, None, 1280, 720, &mut speed));
    save("speed-dark-smallest.png", shot("speed", true, false, None, 760, 480, &mut speed));
    save("services-light.png", shot("services", false, false, None, 1280, 720, &mut services));
    save("services-dark-smallest.png", shot("services", true, false, None, 760, 480, &mut services));
    save("services-collapsed.png", shot("services", false, true, None, 1180, 720, &mut services));
    save("extensions-light.png", shot("extensions", false, false, None, 1280, 720, &mut extensions));
    save("extensions-dark-smallest.png", shot("extensions", true, false, None, 760, 480, &mut extensions));
    save("registry-light.png", shot("registry", false, false, None, 1280, 720, &mut registry));
    save("registry-dark-smallest.png", shot("registry", true, false, None, 760, 480, &mut registry));
    save("protect-light.png", shot("protect", false, false, None, 1280, 720, &mut protect));
    save("protect-defender-dark.png", shot("protect", true, false, None, 1180, 720, &mut protect_defender));
    save("protect-smallest.png", shot("protect", false, false, None, 760, 480, &mut protect));
    save("net-light.png", shot("net", false, false, None, 1280, 720, &mut net));
    save("net-dark-smallest.png", shot("net", true, false, None, 760, 480, &mut net));
    save("privacy-light.png", shot("privacy", false, false, None, 1280, 720, &mut privacy));
    save("privacy-dark-smallest.png", shot("privacy", true, false, None, 760, 480, &mut privacy));
    save("sysinfo-light.png", shot("sysinfo", false, false, None, 1280, 720, &mut sysinfo));
    save("sysinfo-dark-smallest.png", shot("sysinfo", true, false, None, 760, 480, &mut sysinfo));
    save("repair-light.png", shot("repair", false, false, None, 1280, 720, &mut repair));
    save("repair-dark-smallest.png", shot("repair", true, false, None, 760, 480, &mut repair));
    save("question-dark.png", shot("repair", true, false, None, 1180, 720, &mut question));
    save("scheduler-light.png", shot("schedule", false, false, None, 1280, 720, &mut scheduler));
    save("scheduler-dark-smallest.png", shot("schedule", true, false, None, 760, 480, &mut scheduler));
    save("files-light.png", shot("files", false, false, None, 1280, 720, &mut files));
    save("files-dark-smallest.png", shot("files", true, false, None, 760, 480, &mut files));
    save("ctxmenu-light.png", shot("ctxmenu", false, false, None, 1280, 720, &mut ctxmenu));
    save("ctxmenu-dark-smallest.png", shot("ctxmenu", true, false, None, 760, 480, &mut ctxmenu));
    save("procs-light.png", shot("processes", false, false, None, 1280, 720, &mut procs));
    save("procs-dark-smallest.png", shot("processes", true, false, None, 760, 480, &mut procs));
    save("question-light.png", shot("repair", false, false, None, 1180, 720, &mut question));
}
