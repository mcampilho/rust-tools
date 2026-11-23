# rcare

Windows maintenance in one window, written in Rust (Windows only, in progress).

[Versão portuguesa](README.pt.md)

rcare is part of [rust-tools](../../README.md). It gathers what several of the
small tools do, so that nothing has to be opened several times with
administrator rights. The plan, with what is left out on purpose and why, is in
[docs/rcare.md](../../docs/rcare.md). Commands below run from the repository
root.

## Use

```text
rcare
```

A sidebar on the left chooses the page; the **Sidebar** button (or Ctrl+B, or
View › Collapse Sidebar) shows only the icons. To the right of it is the page
as the small tool shows it, and the menus are the window's (File, View, Help)
plus the page's own (Edit, Mark…): each page brings the menus of its small tool, minus what the window has once
(Exit, theme, language, help).

Pages so far:

| Page | The small tool | What it does |
| --- | --- | --- |
| System Maintenance | – (rcare's own) | The checks in one go (F5): junk, broken shortcuts, invalid registry entries, programs with a problem, and what Windows says about each drive; each row opens the page that fixes it. Nothing is changed there |
| Custom Clean | – (rcare's own) | Junk, duplicate and large files (100 MB or more) of a folder in one look, with a button to review each in its page; F5 analyses. Nothing is deleted there |
| Disk Cleaner | [rclean](../rclean/README.md) | Temporary files, browser and package manager caches, broken shortcuts, invalid registry entries |
| Duplicate Files | [rdup](../rdup/README.md) | Files and folders with the same contents on several drives and folders at once |
| Large Files | [rsize](../rsize/README.md) | What takes the space on a drive or in a folder: list, file types, treemap, pie, rings |
| Speed Up | – (rcare's own) | Windows' Game Mode, the memory (free the standby list as administrator), the processor, and the programs open: those ticked are asked to close, as their close button does |
| Startup Optimizer | – (rcare's own) | What starts with Windows, with a switch each: turned off as Task Manager does it, so it can be turned on again and nothing is deleted. Items of every user need administrator rights |
| Program Deactivator | – (rcare's own) | Services of other companies that start with Windows, with a switch each: turned off, a service starts only when needed (Manual); how it started is kept in a copy to turn it on again. Needs administrator rights to change |
| Extensions | – (rcare's own) | What is added to Chrome, Edge, Brave, Vivaldi, Opera, Chromium and Firefox, by profile, on or off, and who installed it; opens the browser's own extensions page, where it is turned off or removed. Changes nothing itself |
| Registry Cleaner | – (rcare's own, with rclean's engine) | Registry entries that name what is gone, by category, saved to a `.reg` file before they are deleted; COM classes and services only shown; the size of the registry's files |
| Protect System | – (rcare's own) | The antivirus and firewall as Windows reports them (other companies' included), Microsoft Defender and the Windows Firewall's profiles; a quick scan, new definitions and turning on what is off, all through Windows' own tools |
| Internet Booster | – (rcare's own) | Windows' TCP values, with those changed by other tools put back to Windows' own, and each connection's DNS: automatic or Cloudflare, Quad9, Google. Each change keeps a `.cmd` copy that puts back what was there |
| Privacy Sweep | – (rcare's own) | The traces Windows keeps: recent documents, the Run box, Explorer's history, the open and save dialogs, jump lists, the clipboard. Registry ones are saved to a `.reg` file first; never the browsers' history, cookies or passwords |
| Uninstall Apps | [rinst](../rinst/README.md) | The programs of every user and source, uninstalled one after another, and their leftovers |

Next:
system tools (see the plan). The small tools
stay as they are: the pages are shared code, so a fix is made once.

Settings (theme, language, window, sidebar, the page shown, the boxes ticked)
are in `%APPDATA%\rcare`, or next to the program when a file named
`rcare.portable` exists there.

## Build

```powershell
./dev.ps1 build --release -p rcare
./dev.ps1 test -p rcare
./dev.ps1 test -p rcare --test screens '--' --ignored   # apps/rcare/target/screens/*.png
```

Profile `release`: it is not built for size.

Command line: `rcare [--tray] [--new-window] [folder]`. A folder opens Custom
Clean on it, analysed. `--tray` stays in the notification area (File ›
Windows Integration › Start with Windows starts it this way); File › Windows
Integration › Explorer Context Menu adds "Analyze with rcare" to folders and
drives (or `--register` / `--unregister`).

## Not yet

Check for Updates for real (there is no release yet) and the Scoop bucket.
