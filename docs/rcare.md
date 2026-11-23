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
| `apps/rcare/src/repair.rs`, `repair_page.rs` | Repair: SFC, DISM, chkdsk /scan per NTFS drive (and /spotfix), the Start menu; the tool's output as it comes, with its progress |
| `apps/rcare/src/sysinfo.rs`, `sysinfo_page.rs` | System Information: Windows, the computer, graphics, drives' health, devices with a problem, battery; Fast Startup, Default Apps, Driver Updates (Windows' pages); Copy |
| `apps/rcare/src/privacy.rs`, `privacy_page.rs` | Privacy Sweep: Windows' traces (recent documents, Run, Explorer, open/save dialogs, jump lists, clipboard); registry ones to a `.reg` first |
| `apps/rcare/src/net.rs`, `net_page.rs` | Internet Booster: Windows' TCP values (put back to Windows' own) and each connection's DNS (Automatic, Cloudflare, Quad9, Google), with a `.cmd` copy before each change |
| `apps/rcare/src/protect.rs`, `protect_page.rs`, `ps.rs` | Protect System: antivirus and firewall products, Defender, Controlled folder access, the Windows Firewall's profiles; Windows' own actions. `ps.rs` runs PowerShell and tools without a window |
| `apps/rcare/src/registry_page.rs` | Registry Cleaner: the cleaner's registry categories (ticked, `.reg` copy, cleaned), COM classes and services only reported, the registry's file sizes |
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

## The window keeps answering; questions in the theme (2026-11-23, the user's tests)

The user saw the window freeze (and had to end it) while SFC or DISM ran and other pages were opened, and white
confirmation boxes in dark mode. Now:

- **Nothing slow on the window's thread.** `bg::Job` runs a function in a thread and the page takes the result in its
  `animate` (the window calls the shown page's `animate` while it has work). Reading: Startup Optimizer, Program
  Deactivator (the Service Control Manager can wait on TrustedInstaller while SFC/DISM run), Extensions, the Privacy
  Sweep's count (also for System Maintenance), Speed Up's two-second reading. Changing: a service's start, the
  Registry Cleaner's and the Privacy Sweep's cleaning (`reg.exe` once an entry), freeing the standby list. Measured with
  `SendMessageTimeout` on every page while it loads: after the window's first ~400 ms the window answers in 1 ms or
  less. Not reproduced with SFC/DISM running (they need administrator rights).
- **Questions drawn in the window** (`rt_gfx::dialog::draw_panel` with its own buttons, `rt_ui::message::Message::
  question`): a page's `Dialogs::confirm` goes through `ShellDialogs`, which notes the question and answers no; the
  window shows it with Yes and No in its theme (Enter Yes, Esc No, screen readers get both buttons); Yes runs the
  same command again, and the same question is then answered yes once. The small apps' pages in rcare get it too;
  folder and save dialogs stay Windows' own.
- **Long operations say so first**: SFC (5–20 minutes), DISM (20 minutes or more, network), chkdsk (minutes to over an
  hour) and Defender's quick scan ask with how long they take and that the computer and the other pages can be used
  meanwhile.

## Repair (phase 6, rcare's own page, last in the sidebar)

Windows' own tools, one at a time (the user's choices): `sfc /scannow`; `DISM /Online /Cleanup-Image /RestoreHealth`;
`chkdsk X: /scan` for each NTFS drive (from `disks::read`; the system drive until they are known), and when it ends
with 3 (errors that need fixing) Fix Errors runs `chkdsk X: /spotfix` with `Y` typed in (on the system drive Windows
does it at the next start; another drive goes offline for a moment — both asked first); the Start menu (Start's and
Search's processes stopped, Windows starts them again, and `Add-AppxPackage -Register` of
`StartMenuExperienceHost`/`ShellExperienceHost`; asked first). `repair::run_command` runs the tool without a window and
reads its output in threads: `Decoder` tells UTF-16 (what `sfc` writes) from the console's code page
(`MultiByteToWideChar(CP_OEMCP)`: `chkdsk`, `DISM`), `Output` keeps the lines (`\r` rewrites the line: progress) and
`percent` finds the progress for the bar. Each row says how it ended last (chkdsk by its exit code: 0 no errors, 1–2
fixed, 3 needs fixing). Also Copy the Output and Open the Log (`CBS.log`, `dism.log`). Administrator rights for every
tool. Verified: the decoding, the lines and the progress (tests), the runner with `cmd` (its output and exit code),
screenshots, the page in the real `.exe`. **Not verified**: running SFC, DISM, chkdsk or the Start menu repair for real
(administrator; SFC and DISM take a long time).

## System Information (phase 6, rcare's own page)

One hidden PowerShell (`sysinfo::SCRIPT`): `Win32_OperatingSystem` (and `DisplayVersion`/`UBR` from the registry),
`Win32_ComputerSystem`, `Win32_BaseBoard`, `Win32_BIOS`, `Win32_Processor`, `Win32_VideoController`,
`Get-PhysicalDisk` (health), `Win32_Battery`, `Get-PnpDevice` with `Status` Error (`Problem`: `CM_PROB_…`, said in
words by `problem_text`), `HiberbootEnabled` (absent = on). Six cards (Windows, Computer — the board when the maker
left "System Product Name" —, Graphics, Drives, Devices with a problem, Battery); in a small window each line goes on
one row. Actions: Refresh, Copy (all as text), Fast Startup (a switch, `HiberbootEnabled`, administrator), Default
Apps (`ms-settings:defaultapps`: Windows does not let programs set them) and Driver Updates
(`ms-settings:windowsupdate-optionalupdates`). Here: Windows 10 Pro N 22H2 19045.6466, i7-6700K, 32 GB, GTX 960, three
healthy drives, the Genesys card reader that could not start, Fast Startup on. Verified: that reading, screenshots,
the page in the real `.exe`. **Not verified**: turning Fast Startup off and on for real (administrator).

## Starting as administrator (2026-11-23, the user's request)

rcare starts as administrator by default (`start.rs`): after `hand_off` (a resident rcare shows itself first), if it
is not elevated and none of `--user`, `--tray`, `--help`, `--register`, `--unregister` was given, it starts itself
again with `runas` (`rt_sys::win::restart_elevated`, the same arguments, quoted by `relaunch_args`) and ends; if
Windows' prompt is refused it goes on without the rights. `--user` starts it without asking. `--tray` never asks (it
is what Start with Windows runs: a prompt at every sign-in would be wrong); File › Restart as Administrator stays.
As administrator Windows blocks drag and drop from Explorer (UIPI): the folder dialogs work. Verified: the decision
and the quoting (tests), `--user` starting without a prompt. **Not verified**: accepting the prompt (it is the
user's screen); the Explorer verb now prompts too.

## Privacy Sweep (phase 5, rcare's own page, last in the sidebar)

Only Windows' traces (the user's choice): recent documents (the files in `%APPDATA%\Microsoft\Windows\Recent`, not
its subfolders, and `Explorer\RecentDocs`), the Run box (`RunMRU`), Explorer's address bar and searches
(`TypedPaths`, `WordWheelQuery`), the open and save dialogs (`ComDlg32\OpenSavePidlMRU`, `LastVisitedPidlMRU`), jump
lists (`Recent\AutomaticDestinations` and `CustomDestinations`; not ticked by default: pinned items go too) and the
clipboard (`Clipboard.Clear` and `Windows.ApplicationModel.DataTransfer.Clipboard.ClearHistory` through PowerShell; not
ticked by default). Counted: files plus the keys' values (their `MRUList`/`MRUListEx` bookkeeping left out; an empty
key is nothing). Sweeping asks with the list; each trace's keys go to `backups\privacy <date>\<id>.reg` and are
deleted whole (Windows makes them again), files are deleted (one already gone is not a failure). Never the
browsers' history, cookies or passwords. Here: 114 recent documents, 2 in Run, 21 in the dialogs, 32 jump list files.
Verified: that count, sweeping files in a folder of its own (test), screenshots, the page in the real `.exe` (nothing
swept on its own). **Not verified**: sweeping this PC's traces for real, and the clipboard. Also counted by System
Maintenance's one-click check (its second row).

## Internet Booster (phase 5, rcare's own page)

One hidden PowerShell (`net::SCRIPT`): `Get-NetTCPSetting -SettingName Internet` and `Get-NetOffloadGlobalSetting`
(PowerShell's own words, the same in every language: `netsh` prints in the system's language), and for each adapter
that is up its IPv4 and IPv6 servers (`Get-DnsClientServerAddress`) and whether they were set by hand (`NameServer`
under `Tcpip`/`Tcpip6\Parameters\Interfaces\{guid}`; the `fec0:` placeholders left out). TCP card: the values, those
that differ from Windows' own marked (only auto-tuning Normal, RSS and RSC Enabled are compared: ECN, timestamps and
the initial timeout change with Windows' version, so they are only shown); "Restore Windows' TCP Values…"
(`netsh int tcp set global autotuninglevel=normal rss=enabled rsc=enabled ecncapability=default timestamps=default`).
DNS card: the connections (servers; Automatic, by hand or the known service) and four choices for the one selected:
Automatic (`Set-DnsClientServerAddress -ResetServerAddresses`), Cloudflare, Quad9, Google (IPv4 and IPv6 together).
Each change asks, needs administrator rights, and first writes `backups\network <date>\tcp.cmd` or `dns-<index>.cmd`
with the `netsh` lines that put back what was there (if the copy cannot be written nothing changes); then it reads
again. Here: Windows' own TCP values, Ethernet on the router's DNS (IPv4 and IPv6). Verified: that reading,
screenshots, the page in the real `.exe`. **Not verified**: the changes for real and running a `.cmd` copy (they need
administrator rights).

## Protect System (phase 5, rcare's own page)

One hidden PowerShell (`protect::SCRIPT`, a fact a line, read by `parse`): `root/SecurityCenter2` `AntiVirusProduct` and
`FirewallProduct` (`productState`: the middle byte's 0x10 = on, the last byte 0 = definitions up to date; Defender's
own line is left out, its `Get-MpComputerStatus` describes it: `AMRunningMode` Normal = the antivirus, Passive/EDR,
or not running), `Get-MpPreference` `EnableControlledFolderAccess`, `Get-NetFirewallProfile`. Three cards (antivirus,
firewall, Microsoft Defender) with a green or orange dot. Actions, all Windows' own and in a thread: Quick Scan and
Update Definitions (`MpCmdRun -Scan -ScanType 1`, `-SignatureUpdate`; only when Defender is the antivirus), Turn On
What Is Off… (asks with the list; `netsh advfirewall set <profile> state on`, `Set-MpPreference
-DisableRealtimeMonitoring $false`; administrator), Open Windows Security (`windowsdefender:`), Controlled Folder
Access (its page in Windows Security: turning it on can block programs, so the user does it there). Here: Bitdefender
and IObit on, Defender not running, `Get-MpPreference` fails (so Controlled folder access shows Unknown), every
Windows Firewall profile on. Verified: that reading, screenshots (this PC, and Defender as the antivirus; on a narrow
page the buttons go on two rows), the page in the real `.exe`. **Not verified**: the scan, the definitions and turning
on for real (Defender is not the antivirus here; turning on needs administrator rights).

## Registry Cleaner (phase 4, rcare's own page)

The user's choices (2026-11-22): only the safe categories are cleaned; COM classes and services are only shown;
Registry Defrag only shows sizes. The engine is the cleaner's (`rt_clean::registry`), so rclean gained the new
categories too: help files (`Help`, `HTML Help`), sounds (the default value of `AppEvents\…\.Current`: the event
stays, silent) and Windows Firewall rules (`FirewallRules` values whose `App=` is gone; the Firewall reads the
registry again when it restarts). `registry::REPORTS`/`report`: COM classes (`CLSID\…\InprocServer32` as a path,
`LocalServer32` as a command) and services (`Type` 16/32) whose file is gone — 19 COM classes here, all Windows'
own leftovers, which is why they are never cleaned. In the 32-bit view `Program Files` is also looked for in
`Program Files (x86)` (it was counted gone before). The page: categories on the left (ticked; the system's only as
administrator, the reports with an "i"), the selected one's entries on the right, scan in a thread when the page is
first shown and with F5; "Clean the Ticked Entries…" asks with the list and saves each category to
`backups\registry <date>\<id>.reg` before deleting (the cleaner's folder and format), then scans again;
"Open the Copies Folder"; "Registry Size…": the hives' sizes (`System32\config` needs administrator rights even
to list: without them only `NTUSER.DAT` and `UsrClass.dat`, and it says so). Verified: the scan of this PC (nothing
to clean without administrator; 19 COM classes reported), screenshots, the page in the real `.exe`. **Not
verified**: cleaning for real from this page (the engine's own test `cleans_a_startup_entry` does it), and the
system's categories as administrator.

## Extensions (phase 3, rcare's own page)

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
7. ~~Phase 4~~ done (2026-11-22): Registry Cleaner. ~~Phase 5~~ done (2026-11-23): Protect System, Internet
   Booster, Privacy Sweep. ~~Phase 6~~ done (2026-11-23): System Information (with Fast Startup, Default Apps, Driver
   Updates) and Repair (SFC, DISM, chkdsk, Start menu). See their sections. The user's list up to System Information is
   covered.
8. ~~The Privacy Sweep in System Maintenance's one-click check~~ done (2026-11-23): a sixth row, counting the traces
   ticked in the Privacy Sweep (the scan runs it too), its button opens that page. Next: what was left out on purpose or for later (see
   "Later" below: Auto Shutdown, Context Menu Manager, File Shredder, Empty Folder Scanner, Process Manager…); the
   first release `rcare-v0.1.0` (a push and a tag: ask the user) and the Scoop bucket.

## Decisions for phase 6 (the user chose the recommended option of each, 2026-11-23)

- **Update Drivers**: no network in rcare: list the devices with a problem and open Windows Update's optional updates,
  where Windows itself finds and installs drivers.
- **chkdsk** (Repair page): `chkdsk /scan` (online, no restart); when it finds errors, offer `/spotfix` (on the system
  drive it is scheduled for the next start). Together with SFC and DISM.
- **Repair Start Menu**: restart Start's and Search's processes and register Windows' own Start menu package again
  (`Add-AppxPackage -Register`); nothing is deleted.

## Decisions for phase 5 (the user chose the recommended option of each, 2026-11-22)

With these, rcare itself still uses no network: what needs it (Defender's definitions) is done by Windows.

- **Privacy Sweep**: only Windows' traces — recent documents, the Run and Explorer address bar history, jump lists,
  the clipboard — with a `.reg` copy of what is registry. Never the browsers' history, cookies or passwords (rclean's
  rule stays).
- **Internet Booster (TCP Optimize)**: show Windows' current TCP values (autotuning, ECN, RSS…) and offer "Restore
  Windows' values", with a copy of what was there. No "boost" tweaks (TTL 64, MTU, duplicate ACKs): Windows tunes
  itself and those old tweaks can make it worse.
- **DNS Protector**: show each connection's DNS and let the user choose Automatic (the router's) or a known one
  (Cloudflare, Quad9, Google), as administrator, with a copy of what was there to undo.
- **Protect System**: Defender and the Firewall through Windows' own tools — their state, a quick scan, updating the
  definitions, turning on what is off; the "safe folder" is Defender's Controlled folder access. Left out: ads
  removal, email, homepage advisor, fingerprint; undelete stays for later (a tool of its own).

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

**4. Registry** ✔ — Registry Cleaner (the safe categories, COM and services reported, `.reg` copy before each
cleaning, restore by opening it); Registry Defrag (sizes only).

**5. Network and protection** ✔ (Protect System, Internet Booster, Privacy Sweep, as decided) — Internet Booster (TCP, DNS Protector, browser optimize); Protect System (Defender,
Firewall, anti-tracking, browser protection, safe folder, undelete to be assessed).

**6. System** ✔ (System Information, Repair) — Fast Startup, repair Start Menu, Update Drivers, Set Default Program, System Information, hardware
and software health, and a Repair page (SFC, DISM, chkdsk).

## Rules

- Every destructive action shows what it will do, asks, and keeps a copy to undo (registry, services, startup).
- Modules are tested with fake data; real actions only on an elevated machine (the service was never run as a real
  service yet: `install`/`uninstall` only on an elevated machine).
- Visible texts in `strings.txt`; README.md and README.pt.md updated together.
