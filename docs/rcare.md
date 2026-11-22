# rcare — Windows maintenance suite (plan)

Status: planned (2026-11-22). Windows only, no size limit, `release-small` is not used (the profile is `release`).

`rcare` gathers what several tiny tools do in one window, so that nothing has to be opened several times with
administrator rights. Maintenance suites it takes as a model: BleachBit, CCleaner, Glary Utilities, IObit Advanced
SystemCare and Uninstaller, Auslogics BoostSpeed, Ashampoo WinOptimizer, AVG TuneUp, Wise Care 365, Wintoys, the
Sysinternals tools.

## Decisions

1. **Merging is not copying.** The engines already live in crates (`rt-clean`, `rt-inst`, `rt-dups`, `rt-scan`). The
   screens move out of each app's `main.rs` into pages that both the small app and `rcare` use. rclean, rinst, rdup
   and rsize stay as they are (portable, small); a fix is made once.
2. **`Page` trait in `rt-ui`.** Each page draws itself, handles the pointer and the keyboard, runs its own work
   threads and contributes its menu entries. `rcare` only owns the frame: window, sidebar, menu bar, status bar.
3. **Collapsible sidebar** drawn with `rt-gfx` (no UI library): icons and text, or icons only. The state is saved.
4. **Menus.** File and Help are shared. Between them goes the menu of the active page, rebuilt on each change of
   page. The access-key test runs for every page.
5. **Administrator rights** through the existing `rtsvc` service (user level by default, elevated for the actions
   that need it).
6. **Native Windows tools first** (Defender, Windows Firewall, DISM, SFC, `netsh`, `powercfg`, `pnputil`, Windows
   Update); no engine of our own for antivirus or firewall.

## Honest limits

- Antivirus, spyware, malware, firewall: show the state of Defender and the Firewall, start scans and updates of
  definitions, fix what is off. No own scanner.
- Registry defrag: Windows cannot compact hives in use. Analysis only for now.
- RAM "clean": emptying working sets usually hurts. Show memory as it is and free only what is safe.
- TCP "optimize": Windows tunes itself. Show the current values, safe profiles and "restore defaults".
- Update Drivers and Software Updater use the network: they go through Windows' own APIs and need an approved
  exception to the "no network" rule.
- Repair Cortana: dropped (no longer exists).

## Phases

**0. Foundations** — extract the pages of rclean, rinst, rdup and rsize; create `apps/rcare` (window, sidebar, menu,
status bar, strings in 10 languages, README en/pt, profile, release workflow, Scoop bucket); connect to `rtsvc`.

**1. Existing pages** — Disk Cleaner (rclean), Duplicate Files (rdup), Large Files (rsize), Uninstall Apps (rinst),
Custom Clean (junk, duplicates and large files in one analysis).

**2. One-click maintenance** — runs the available modules in sequence (privacy sweep, junk, registry, shortcuts,
disk check, software health) and reports.

**3. Speed** — Startup Optimizer; App/Extension Cleaner and Program Deactivator; Game Mode and Real-Time TuneUp
(performance monitor); Smart RAM.

**4. Registry** — Registry Cleaner (20 categories, Deep Scan, `.reg` copy before each cleaning, restore); Registry
Defrag (analysis).

**5. Network and protection** — Internet Booster (TCP, DNS Protector, browser optimize); Protect System (Defender,
Firewall, anti-tracking, browser protection, safe folder, undelete to be assessed).

**6. System** — Fast Startup, repair Start Menu, Update Drivers, Set Default Program, System Information, Hardware
and Software Health, and a Repair page (SFC `/scannow`, DISM `/RestoreHealth`, `chkdsk`).

**Later** — Auto Shutdown, Fix problems, Context Menu Manager, Disk Doctor, System Control, File Shredder, Empty
Folders, Shortcut Fixer, Process Manager, Software Updater, Default Program.

## Rules

- Every destructive action shows what it will do, asks, and keeps a copy to undo (registry, services, startup).
- Modules are tested with fake data; real actions only on an elevated machine.
- Visible texts in `strings.txt`; README.md and README.pt.md updated together.
