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
| Disk Cleaner | [rclean](../rclean/README.md) | Temporary files, browser and package manager caches, broken shortcuts, invalid registry entries |
| Duplicate Files | [rdup](../rdup/README.md) | Files and folders with the same contents on several drives and folders at once |
| Large Files | [rsize](../rsize/README.md) | What takes the space on a drive or in a folder: list, file types, treemap, pie, rings |

Next: the uninstaller (rinst), then
speed, registry, network and protection tools (see the plan). The small tools
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

## Not yet

Resident mode (`--tray`), "Windows Integration", Check for Updates for real
(there is no release yet), the Scoop bucket and the release workflow.
