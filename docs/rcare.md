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

## Custom Clean (rcare's own page, first in the sidebar)

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
the Linux build of the small apps after the rewiring (`#[cfg(not(windows))]` code in their `main.rs`, `menus.rs`
and `page.rs`: there is no WSL answering and no Linux target installed — CI on Linux will be the first check), the
connection to the privileged service `rtsvc` from rcare (the pages use it as the small apps do), screen readers.

## Next steps, in order

1. ~~rinst page~~ done (2026-11-22): "Uninstall Apps" in the sidebar.
2. ~~Custom Clean~~ done (2026-11-22): see its section above.
3. ~~Unify the `Theme` copies and rclean's `Confirm`~~ done (2026-11-22): `rt_ui::theme::Theme`, `Dialogs` everywhere.
   The user should still try the real window (it was only started and closed).
4. ~~Residency and integration~~ done (2026-11-22): `--tray` (hidden start, the icon or another launch shows it, closing
   hides it; tested with the real `.exe`), one instance (`hand_off`, `--new-window`), File › Windows Integration ›
   Explorer Context Menu (**one** verb on folders and drives, "Analyze with rcare": the folder opens in Custom Clean,
   analysed, for that time only — `custom_once`; `--register`/`--unregister`) and Start with Windows; `rcare [folder]`
   does the same as the verb. `rcare-v*` is in `release.yml`. The service: the pages already use it as the small apps
   do (snapshots of `rtsvc` for the cleaner and the uninstaller, `scan_with` for the drives); its jobs (`Clean`,
   `UninstallMsi`) only take elevated clients, so an rcare that is not elevated has nothing more to ask of it. Left:
   "Check for
   updates" works once there is a release; the first release (tag `rcare-v0.1.0`, a push: **ask the user**) and then
   the Scoop bucket manifest (`bucket/rcare.json`, then `checkver.ps1 -Dir bucket -Update`).
5. Then the phases below.

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

**2. One-click maintenance** — runs the available modules in sequence (privacy sweep, junk, registry, shortcuts, disk
check, software health) and reports.

**3. Speed** — Startup Optimizer; App/Extension Cleaner and Program Deactivator; Game Mode and Real-Time TuneUp
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
