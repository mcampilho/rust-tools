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
    let l = sidebar::layout(side, PAGES.len(), s);
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
    let mut page = ExtensionsPage {
        list: vec![
            e("Bitdefender Anti-tracker", Browser::Chrome, "Miguel", true, Source::Program),
            e("Bitwarden Password Manager", Browser::Chrome, "Miguel", true, Source::Store),
            e("Dark Reader", Browser::Chrome, "Miguel", false, Source::Store),
            e("Google Docs Offline", Browser::Chrome, "Miguel", true, Source::Store),
            e("Tampermonkey", Browser::Chrome, "Work", false, Source::Store),
            e("Bitwarden Password Manager", Browser::Brave, "Personal", true, Source::Store),
            e("uBlock Origin", Browser::Firefox, "default-release", true, Source::Store),
        ],
        read: true,
        ..ExtensionsPage::default()
    };
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
}
