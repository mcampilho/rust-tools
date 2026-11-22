# rcare — Windows maintenance suite (plan and hand-over)

Status (2026-11-22): **the pages of phase 0 are done** — the pages of rclean, rdup, rsize and rinst sit behind a
collapsible sidebar, after rcare's own **Custom Clean** (phase 1 done). What is left of phase 0 is the service, residency, updates and the release (see "Next steps"). Windows only, no size limit, profile `release` (not `release-small`).

`rcare` gathers what several tiny tools do in one window, so that nothing has to be opened several times with
administrator rights. Models: BleachBit, CCleaner, Glary Utilities, IObit Advanced SystemCare and Uninstaller,
Auslogics BoostSpeed, Ashampoo WinOptimizer, AVG TuneUp, Avast Cleanup, Wise Care 365, Norton Utilities, Outbyte,
Fortect, System Mechanic, Wintoys, Kudu, the Sysinternals tools. The user asked for it in Portuguese on 2026-11-22
(name chosen by them: `rcare`) and wants Rust, a collapsible sidebar like Claude's, the usual screen of each tiny
tool on the right, and the menus rethought to combine the tools'. Everything after "Windows System Information" in
the list below is deliberately left for later.

## Read this first (for whoever continues)

- Talk to the user in European Portuguese (global CLAUDE.md). `python3`, not `python`. **Never commit or push without
  explicit approval.** Verify images and documents visually. Project rules are in `CLAUDE.md` (root) and
  `apps/rcare/CLAUDE.md`: "done" = no warnings, `clippy -D warnings` clean, tests passing; `cargo fmt --all` before
  a commit with `E:\Portable\scoop\apps\rust\current\bin` first on `PATH`; compile with `CARGO_INCREMENTAL=0`; every
  visible text in `strings.txt` (10 languages, tab separated, access keys differing inside each menu in all
  languages); `README.md` and `README.pt.md` updated together; `./dev.ps1 …` loads the MSVC environment.
- The commits of this work on `master`: `dedf452` (rcare skeleton, rclean page), `a31480b` (rdup and rsize pages,
  `Dialogs`, `tr_from`), `2f8c234` (rinst page), then Custom Clean. **Since 2026-11-22 the user lets commits be made
  without asking** for this plan; pushes, and anything that leaves the plan or is their decision, still need them.
- This document carries what is needed so that the code does not have to be re-read from scratch.

## What exists

| Where | What |
| --- | --- |
| `apps/rcare` | the suite: `src/{lib,main,shell,commands,pages,sidebar,settings,storage,appicon}.rs`, `src/i18n/`, `tests/screens.rs`; Windows only (`main` says so elsewhere; the library compiles everywhere) |
| `apps/rclean/src/page.rs` | `CleanPage` (+ `Event`, `Sender`, `shortcuts_text`) |
| `apps/rdup/src/page.rs` | `DupPage` (+ `Event`, `Sender`, `Popup`, `Out`, `shortcuts_text`) |
| `apps/rsize/src/page.rs` | `SizePage` (+ `Event`, `Sender`, `Popup`, `shortcuts_text`); owns its `Settings` |
| `apps/rinst/src/page/` | `InstPage` (`mod.rs`: state, reading, commands, drawing; `input.rs`: keys, search box, mouse; `uninstalling.rs`: the queue, the report, the leftovers) (+ `Event`, `JobMsg`, `Sender`, `Popup`, `shortcuts_text`) |
| `crates/rt-ui/src/page.rs` | `Effects`, `Dialogs` (+ `SystemDialogs`), `Mods` |
| `crates/rt-ui/src/theme.rs` | `Theme`: the colours the four apps and rcare share (each app's `ui::theme` re-exports it) |
| `crates/rt-core/src/i18n.rs` | `add` (extra tables), `tr_in_from`/`tr_from`/`tf_from` (a table searched first) |
| `crates/rt-core/src/menu.rs` | `M::map` (turn the commands of a menu into another type) |
| `apps/rcare/src/maint.rs`, `disks.rs` | System Maintenance: `MaintPage`, `Check`, `View`, `disks_text`; drive health from `Get-Volume` |
| `apps/rcare/src/extensions.rs`, `extensions_page.rs` | Extensions: what is added to Chrome, Edge, Brave, Vivaldi, Opera, Chromium and Firefox, by profile; opens the browser's own extensions page. Changes nothing |
| `apps/rcare/src/services.rs`, `services_page.rs` | Program Deactivator: third-party services that start with Windows, set to Manual after asking, with a copy (`services.txt`) to give the start back |
| `apps/rcare/src/speed.rs`, `speed_page.rs` | Speed Up: Game Mode switch, memory (standby list freed as administrator), processor, programs open (closed only when ticked, with `WM_CLOSE`) |
| `apps/rcare/src/startup.rs`, `startup_page.rs` | Startup Optimizer: what starts with Windows (`Run` keys of the user and the machine, 64/32 bits, both Startup folders) and turning items off/on through `Explorer\StartupApproved` as Task Manager does |
| `apps/rcare/src/custom.rs` | Custom Clean: `CustomPage` (layout, hit, draw, accessibility), `Part`, `Status`, `View`, `large_files` |
| `docs/rcare(.pt).md` | this plan |

The small apps (rclean, rdup, rsize, rinst) were rewired to hold the same pages, so they keep working as before and a
fix is made once.

## The page pattern (the contract every page follows)

A page owns its state, its work threads, its drawing in a rectangle of the window, the pointer and the keyboard. It
does **not** own the window. Read `apps/rclean/src/page.rs` (the simplest) and then `apps/rdup/src/page.rs`
(dialogs, popup menus, text boxes) and `apps/rsize/src/page.rs` (settings, wake-up times, treemap caches).

- `XPage::new(…, send: Sender)`; `Sender = Arc<dyn Fn(Event) + Send + Sync>`: the threads send their `Event`s there
  and the window turns them into its own `UserEvent` (rcare: `UserEvent::Clean/Dups/Size/Inst`). `page.event(e)` handles one.
- `page.fx: rt_ui::page::Effects` — what the page asks of the window: `redraw`, `menu_dirty` (menus' enabled/ticked
  state changed), `info` (a message to show), `save` (settings changed). The window calls `after_page()` after every
  call into the page: `let fx = page.fx.take(); …`. Pages call `self.fx.redraw()`, `.menus()`, `.info(t, x)`, `.save()`.
- Asking the user (synchronous): `rt_ui::page::Dialogs` (`confirm`, `pick_folder`, `save_file(name, (kind, pattern),
  dir)`, `recycle`), passed to `page.command(cmd, &mut dialogs)`; the window builds `SystemDialogs { hwnd }`. All four
  pages take it.
- `page.command(cmd, …) -> bool`: does the commands that are the page's and returns true; false for the window's
  (Exit, theme, language, help, Explorer menu, autostart, restart as administrator).
- `page.enabled(cmd)`, `page.checked(cmd)` for menus and buttons.
- `page.render(gfx, area, theme, scale, menu_bar)` draws only inside `area` (`view::draw` fills `l.full`, it no
  longer clears the whole canvas).
- Input: `key(…) -> Option<Command>` (the command of a chord, or the page used the key), `pointer(x, y, blocked)`,
  `pointer_left()`, `press(…) -> Option<Command | Out>` (a button asked for a command, or a popup menu),
  `release()`, `drag…`, `wheel(delta, at)`, `right_click(…)`. The mouse position and modifiers come as arguments
  (`Mods { ctrl, shift, alt }`); the page remembers the last scale from `render`.
- Menus that pop up (`Popup`): the page only says which; the window builds and shows it
  (`rt_sys::win::menu::popup_at_cursor`) and runs the command chosen.
- Wake-ups: `animate()` — rclean and rinst `bool` (an arc turns), rdup `Option<Duration>`, rsize `Option<Instant>`; the
  window sets `ControlFlow::WaitUntil`.
- Screen readers: `accessibility(&mut nodes, &mut children) -> Option<focus>` (rows are `NodeId(100 + index)`; 1, 3
  are the list and the status line) and `accessibility_click(i)`. The window adds its own nodes (dialog 4–6, rcare's
  sidebar 10–99).
- Settings: rclean/rdup/rinst keep them in the window's `Settings` and the page copies in/out (`store_into`); rsize's
  page **owns** its `Settings` (theme, language and window size are in it too). rcare keeps rdup's, rsize's and
  rinst's in their own files, `rdup.ini`, `rsize.ini` and `rinst.ini`, next to its `settings.ini` (rclean's ticks are in
  rcare's own settings).
- Each app's `i18n::add_table()` and its `tr`/`tf` search **its own table first** (`rt_core::i18n::tr_from`). This is
  needed because the apps translate the same English key with different access keys (each tuned to its own menus);
  rcare's `Cmd::label` delegates to the page's `label()` for the same reason. Any new page must do the same.

## System Maintenance (phase 2, rcare's own page, first in the sidebar)

The checks that exist, in one go ("Scan", F5), one row each with what it found and a button: **Junk Files**, **Broken
Shortcuts** and **Invalid Registry Entries** (the cleaner's analysis, split by `Group`: junk counts the ticked targets,
shortcuts and registry all that were found), **Software Health** (programs of every list of the uninstaller with a
`Problem`: uninstaller gone, install failed, manifest removed), **Drive Health** (Windows' own `Get-Volume` in a hidden
PowerShell, no administrator needed: `HealthStatus` and `OperationalStatus`; Details shows each drive and how to repair
one with `chkdsk X: /f` or the drive's Properties › Tools › Check — on this PC the S: drive says "Warning / Full Repair
Needed"). "Review" opens the cleaner or the uninstaller; nothing is changed on this page. More checks join with the
later phases (privacy sweep in phase 5 — deleting history or cookies is the user's decision, since rclean never does —,
startup, Windows integrity in phase 6).

## Extensions (phase 3, rcare's own page, last in the sidebar)

Read from each browser's own files, with `rt_core::json`: for the Chromium browsers (Chrome, Edge, Brave, Vivaldi,
Chromium under `%LOCALAPPDATA%`, Opera under `%APPDATA%\Opera Software`) `extensions.settings` of `Secure
Preferences` and `Preferences` of each profile (System and Guest left out; names from `Local State`), for Firefox
`extensions.json` of each profile. Left out: the browsers' own parts (Chromium `location` 5 and 10; Firefox
`app-builtin`, `app-system-*`), themes, hosted apps, dictionaries. Off = `disable_reasons` not empty (or the old
`state` 0) / Firefox `active` false. "Installed by": store, another program (`location` 2, 3, 6), policy (7, 9),
unpacked (4, 8). A name `__MSG_…__` is read from the extension's `_locales`. "Open in Browser" starts the browser
(`App Paths`, then the usual folders) with `--profile-directory=<profile>` and `chrome://extensions/?id=<id>` (`edge://`,
`brave://`, `vivaldi://`, `opera://`; Firefox `about:addons`). Nothing is changed by rcare (the user's choice).
Verified: the real list here (Chrome 19, Brave 2, the same states Chrome shows), screenshots, the page opening in
the real `.exe`. **Not verified**: opening the browser's page (it would open the user's browser); Edge, Vivaldi,
Opera and Firefox (not installed here: tested with sample files only).

## Program Deactivator (phase 3, rcare's own page)

The services (`SERVICE_WIN32`, not drivers) that start with Windows (Automatic or Automatic (delayed)) and are not
Microsoft's: the company is read from the program's version information (`CompanyName`, any of its languages);
with no company, a program in the Windows folder counts as Windows' own, and a service whose program cannot be
told is left out. Read with the Service Control Manager (`EnumServicesStatusExW`, `QueryServiceConfigW`,
`QueryServiceConfig2W` for the delay), which answers without administrator rights. Turning one off asks first
(security programs and drivers may need to start with Windows), writes how it started to `services.txt` in rcare's
data folder (name, tab, `auto`/`delayed`; the first start is kept), and only then sets it to Manual
(`ChangeServiceConfigW`); if the copy cannot be written nothing is changed. Turning it on gives back that start
(delay included) and takes it out of the copy. The list also shows the services rcare set to Manual, so they can
be given back. Changing needs administrator rights (without them the switches are dimmed and say why).
Verified: the real list here (Bitdefender, IObit, Intel, NVIDIA, Everything, the rust-tools service), screenshots,
the page opening in the real `.exe`. **Not verified**: changing a service for real (needs an elevated rcare).

## Speed Up (phase 3, rcare's own page)

Three cards and a list, read again every two seconds while the page is shown (paused when another page is chosen or the
window hides): **Game Mode** (Windows' own switch, `HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled`, absent = on),
**Memory** (`GlobalMemoryStatusEx`; the standby list from `NtQuerySystemInformation(SystemMemoryListInformation)`, which
answered even without administrator rights here; "Free Standby Memory" = `NtSetSystemInformation` with
`MemoryPurgeStandbyList` after enabling `SeProfileSingleProcessPrivilege`, administrator only), **Processor** (two
`GetSystemTimes` samples), and **Programs open** (top-level visible windows without owner or tool style, grouped by
process, Windows' shell and rcare left out; memory = working set). "Close the Ticked Programs…" asks first, then posts
`WM_CLOSE` to their windows: a program can still ask to save (an unsaved Notepad was in the list here). The programs'
memory is never emptied (the user's choice). Not done yet: a history graph; "Auto RAM Clean" stays out on purpose.

## Startup Optimizer (phase 3, rcare's own page, last in the sidebar)

The list Task Manager's Startup tab shows: `HKCU\…\Run`, `HKLM\…\Run` (64 and 32 bits), the user's and every user's
Startup folders (shortcuts read with `rt_sys::lnk`). One row each with a switch, where it starts from and what it runs
(in orange when the program is not there; "Needs administrator rights" for items of every user when rcare is not
elevated — they cannot be switched then, and saying so is the page's answer). Turning off writes the 12 bytes Task
Manager writes under `Explorer\StartupApproved\{Run,Run32,StartupFolder}` (03 + FILETIME; on: 02), so nothing is deleted
and Windows' own tab agrees; checked for real with an item of its own (`startup::tests::toggles_an_item_of_its_own`,
ignored: it writes the user's `Run`, then removes it). Buttons and Edit menu: Turn Off, Turn On, Open File Location,
Refresh (F5); Space turns the selected item on or off.

## Custom Clean (rcare's own page, second in the sidebar)

An overview, not a cleaner: three cards (Junk Files, Duplicate Files, Large Files) with the space and the count, and
"Review", which opens the page that shows the part in full, where it is cleaned. Nothing is deleted on this page.
"Analyze" (F5, File › Analyze) starts the work **in the other pages**: the cleaner analyses (system-wide), rdup searches
the folder (`DupPage::open_paths`, so the duplicates page's chosen roots become that folder, and its own filters
apply), rsize reads it (`SizePage::start_scan`). The folder is the user's (`USERPROFILE`) unless "Change Folder…"
chose another (`custom_folder=` in `settings.ini`). Large files are files of 100 MB or more (`custom::LARGE`), counted
from rsize's tree once per tree (`large_cache`). The shell reads the pages' state into `View` before drawing
(`custom_status`); what the pages report redraws it. Cards side by side when wide, one under the other when narrow and
tall, narrow columns with the button along the bottom on a small window (pictures `custom-*.png`).

## How rcare works

- `pages.rs`: `PAGES` (id saved in settings, `Kind`, label key, sidebar glyph). Adding a page = a `Kind`, an entry,
  a glyph in `sidebar::draw_glyph`, a field and an arm in `shell.rs` for each operation.
- `commands.rs`: `Cmd::{Clean(c), Dups(c), Size(c), Inst(c), CustomAnalyze, CustomFolder, Sidebar, RestartAdmin, Exit, Theme, UiLanguage,
  Shortcuts, CheckUpdates, About}`; `from_clean/from_dups/from_size/from_inst` turn a page command that is the
  window's into the window's.
  **Menus are built from each small app's own `menu_tree()`** (`own_menus`): what is the window's (Exit, restart,
  Explorer menu, autostart, theme, language, help) is dropped, the app's File items go first in rcare's File (then
  Restart as administrator, Exit), its View items first in View (then Collapse sidebar, Theme, Language), its other
  menus (Edit, Mark, Program…) sit between them, Help is the window's. A new page therefore gets menus for free; if two
  labels clash in some language the test `access_keys_differ_within_each_menu` says which (fix the string in
  rcare's table or the app's). With four pages the View menu is crowded: "Collapse Side&bar" needed a letter free in
  all of them (German says "Seitenleiste: nur S&ymbole", Spanish "Contra&er barra lateral").
- `sidebar.rs`: layout/hit/draw without a window (testable); 224 px open, 52 px collapsed (icons do not move); state
  in `settings.ini` (`sidebar=`, `page=`).
- `shell.rs` (Windows only): the `page!(self, p => …)` macro runs an expression on the page shown; operations whose
  signatures differ per page (`key`, `press`, `command`, `enabled`, `render`, `wheel`…) are explicit three-arm
  matches. `take_fx` merges the effects of all pages (a page not shown can still report: a search ends).
  `ensure_started(i)` does a page's first work the first time it is shown (cleaner: first analysis; uninstaller: read
  the programs; the others: read the drives). Four fields and four-arm matches now: with a fifth page, replace them
  by a small trait or an enum of pages.
- Data files: rcare's folder (`%APPDATA%\rcare`, or portable) — `settings.ini`, `rdup.ini`, `rsize.ini`, `rinst.ini`,
  `hashes.bin` (duplicates' hash cache: separate from rdup's own), `backups\registry …` (the cleaner's `.reg` copies),
  `backups\leftovers ….reg` (the uninstaller's).

## Verified, and not

Verified (2026-11-22, Windows 10): `cargo fmt --check`, `clippy --workspace --all-targets -D warnings`,
`test --workspace` (the two `rt-svc` pipe tests failed once under load and pass alone: a pre-existing race, not
touched); the pictures of the four pages, light/dark, open/collapsed sidebar (`tests/screens.rs`, read by eye); a
start-up test of the real `rcare.exe` (debug build, portable mode in `target\debug`): the window opened, stayed up 8 s,
closed by its close message with code 0 and wrote its four settings files (removed afterwards).

**Not verified**: using the real window (mouse, keyboard, native menus, popups, dialogs, switching pages, uninstalling),
running the small apps on Linux after the rewiring (their build, clippy and tests on Linux passed in CI on
2026-11-22, run 45064520969, after the push of `bf7e572`; their drawn menus and dialogs were not run), the
connection to the privileged service `rtsvc` from rcare (the pages use it as the small apps do), screen readers.

## Next steps, in order

1. ~~rinst page~~ done (2026-11-22): "Uninstall Apps" in the sidebar.
2. ~~Custom Clean~~ done (2026-11-22): see its section above.
3. ~~Unify the `Theme` copies and rclean's `Confirm`~~ done (2026-11-22): `rt_ui::theme::Theme`, `Dialogs` everywhere.
   The user should still try the real window (it was only started and closed).
4. ~~System Maintenance (phase 2)~~ done (2026-11-22): see its section.
5. ~~Residency and integration~~ done (2026-11-22): `--tray` (hidden start, the icon or another launch shows it, closing
   hides it; tested with the real `.exe`), one instance (`hand_off`, `--new-window`), File › Windows Integration ›
   Explorer Context Menu (**one** verb on folders and drives, "Analyze with rcare": the folder opens in Custom Clean,
   analysed, for that time only — `custom_once`; `--register`/`--unregister`) and Start with Windows; `rcare [folder]`
   does the same as the verb. `rcare-v*` is in `release.yml`. The service: the pages already use it as the small apps
   do (snapshots of `rtsvc` for the cleaner and the uninstaller, `scan_with` for the drives); its jobs (`Clean`,
   `UninstallMsi`) only take elevated clients, so an rcare that is not elevated has nothing more to ask of it. Left:
   "Check for
   updates" works once there is a release; the first release (tag `rcare-v0.1.0`, a push: **ask the user**) and then
   the Scoop bucket manifest (`bucket/rcare.json`, then `checkver.ps1 -Dir bucket -Update`).
6. ~~Startup Optimizer~~, ~~Speed Up~~, ~~Program Deactivator~~ and ~~Extensions~~ done (2026-11-22): phase 3 is
   complete (Real-Time TuneUp, a performance history, stays for later). See their sections.
7. Phases 4–6 (registry, network and protection, system).

## Decisions for the rest of phase 3 (the user chose the recommended option of each, 2026-11-22)

- **App/Extension Cleaner**: removing browser extensions means editing each browser's profile (Chrome/Edge
  `Preferences`, Firefox `extensions.json`) while the browser is closed — or only listing them and opening the
  browser's own extensions page. Recommended: list them (name, browser, profile, enabled) and open the browser's page. **Chosen.**
- **Game Mode / "programs stopped, RAM released"**: stopping programs ends the user's processes (unsaved work can be
  lost). Recommended: Windows' own Game Mode switch (`HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled`) plus a
  list of what runs, closing only what the user ticks, politely (`WM_CLOSE`), never forcing. **Chosen.**
- **Smart RAM**: recommended as written in "Honest limits": show the memory, free only the standby list on request
  (needs administrator rights: `NtSetSystemInformation` with `MemoryPurgeStandbyList`). **Chosen.**
- **Program Deactivator**: services set to start automatically, besides the startup items? Changing services needs
  administrator rights and can break Windows: recommended only third-party services, with a copy of the setting to
  undo. **Chosen.**

## Decisions

1. **Merging is not copying.** The engines already live in crates (`rt-clean`, `rt-inst`, `rt-dups`, `rt-scan`). The
   screens moved out of each app's `main.rs` into pages that both the small app and `rcare` use. rclean, rinst, rdup
   and rsize stay as they are (portable, small); a fix is made once.
2. **Native Windows tools first** (Defender, Windows Firewall, DISM, `sfc /scannow`, `netsh`, `powercfg`, `pnputil`,
   Windows Update): no engine of our own for antivirus or firewall. A Repair page offers `sfc /scannow`, `DISM
   /Online /Cleanup-Image /RestoreHealth` and `chkdsk` (the user asked for these).
3. **Administrator rights** through the existing `rtsvc` service (user level by default, elevated for the actions that
   need it), or restarting rcare elevated (what the small apps do).
4. The user approved: name `rcare`; small apps stay; Defender/Firewall integration instead of an own engine.

## Honest limits

- Antivirus, spyware, malware, firewall: show the state of Defender and the Firewall, start scans and updates of
  definitions, fix what is off. No own scanner.
- Registry defrag: Windows cannot compact hives in use. Analysis only for now.
- RAM "clean": emptying working sets usually hurts. Show memory as it is and free only what is safe.
- TCP "optimize": Windows tunes itself. Show the current values, safe profiles and "restore defaults".
- Update Drivers and Software Updater use the network: they go through Windows' own APIs and need an approved
  exception to the "no network" rule (ask the user).
- Repair Cortana: dropped (no longer exists).

## The user's list (up to System Information), as asked

Windows System Maintenance: Privacy Sweep, System Optimization, Antivirus Protection, Spyware Removal, Junk File
Clean, Internet Boost, Firewall, Security Reinforce, Shortcut Fix, Registry Defrag, Hardware Health, Vulnerability
Fix, Registry Clean, Disk Optimization, Software Health, Disk Check. **Custom Clean**: Junk Files, Duplicate Files,
Large Files. **Speed Up System**: Game Mode (programs stopped, RAM released), Startup Optimizer, App/Extension
Cleaner, Real-Time TuneUp (AutoCare, Auto RAM Clean, Performance Monitor). **Protect System**: System Protection
(malware, firewall, anti-spyware), Privacy Protection (safe folder, access protection, fingerprint, anti-tracking),
Browser Protection (email, ads removal, surfing, homepage advisor), Security & Repair (DNS protect, undelete).
**Smart RAM**: Smart Clean, Deep Clean. **Internet Booster**: TCP Optimize (max duplicate ACKs, optimum route, TCP
1323 options, MTU discovery, black hole router detection, TTL 64), Browser Optimize. **Registry Cleaner**: ActiveX
and Component, Registry Integrity, Application Paths, Fonts, Help Files, History Files, Shared Files, Shortcut,
Software, Sounds, Start Menu, Startup Items, MUI Cache, Virtual Device, File Extensions, System Extensions, Windows
Services, Firewall Settings, Deep Scan. **Registry Defrag**: reduce fragments, repair structural errors, recover
unused memory. **Other options**: Fast Startup, Repair Start Menu, Update Drivers, Set Default Program, Uninstall
Apps. **Program Deactivator**: enabled programs that can be deactivated. **System Information**.

Later (user's decision): Auto Shutdown, Fix problems (general, advanced, browser), Context Menu Manager, Disk
Doctor, System Control, DNS Protector, Disk Cleaner (= rclean: temporary, backup, cache, log, other junk), File
Shredder, Empty Folder Scanner, Shortcut Fixer, Duplicate File Finder (= rdup), Large File Finder (= rsize), Process
Manager, Software Updater, Default Program.

## Phases

**0. Foundations** — pages of rclean ✔, rdup ✔, rsize ✔, rinst ✔; rcare with sidebar and menus ✔;
`rtsvc`, residency, updates, release (see "Next steps").

**1. Existing pages in the sidebar** — Disk Cleaner (rclean) ✔, Duplicate Files (rdup) ✔, Large Files (rsize) ✔,
Uninstall Apps (rinst) ✔, and **Custom Clean** ✔ (junk, duplicates and large files in one analysis).

**2. One-click maintenance** ✔ (System Maintenance, with the checks that exist so far) — runs the available modules in sequence (privacy sweep, junk, registry, shortcuts, disk
check, software health) and reports.

**3. Speed** — Startup Optimizer ✔; Speed Up ✔ (Game Mode, memory and Smart RAM, processor, programs open); Program Deactivator ✔ (third-party services, with a copy); Extensions ✔ (list and open the browser's page); Game Mode and Real-Time TuneUp
(performance monitor); Smart RAM.

**4. Registry** — Registry Cleaner (the categories above, Deep Scan, `.reg` copy before each cleaning, restore);
Registry Defrag (analysis).

**5. Network and protection** — Internet Booster (TCP, DNS Protector, browser optimize); Protect System (Defender,
Firewall, anti-tracking, browser protection, safe folder, undelete to be assessed).

**6. System** — Fast Startup, repair Start Menu, Update Drivers, Set Default Program, System Information, hardware
and software health, and a Repair page (SFC, DISM, chkdsk).

## Rules

- Every destructive action shows what it will do, asks, and keeps a copy to undo (registry, services, startup).
- Modules are tested with fake data; real actions only on an elevated machine (the service was never run as a real
  service yet: `install`/`uninstall` only on an elevated machine).
- Visible texts in `strings.txt`; README.md and README.pt.md updated together.
