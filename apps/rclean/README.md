# rclean

A portable cleaner for Windows and Linux, written in Rust. It shows how much
the temporary files of the system and the caches of the browsers take, and
deletes what you tick, after asking.

[Versão portuguesa](README.pt.md)

rclean is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`rclean.exe` is a single 1.9 MB executable: no installer, no runtime, no DLLs
beyond those that ship with Windows. It makes no network connections and has
no telemetry. It cleans temporary files, browser caches, the caches of package
managers, broken shortcuts and invalid registry entries (see [Phases](#phases)).

## Use

```text
rclean
```

When it starts, rclean looks at once at what each item holds (**Analyze**,
F5, does it again) and shows, for each, how many files and how much space; a
title for each group (Windows or Linux, then each browser found) adds up its
items. Tick what you want gone (a click on the box, Space for the row
selected, the group's box for all of it) and press **Clean** (Ctrl+L): rclean
says what will be deleted and how much, and asks before it starts. Then it
looks again. **Stop** (Esc) stops after the item now.

The ticks are remembered. **Tick the Usual Ones** goes back to those ticked at
first; the others (thumbnails, shader caches, the Recycle Bin) are left for
you to choose.

**Resident mode:** `rclean --tray` starts hidden in the notification area,
looking at nothing until shown. Launching rclean again or clicking the tray
icon shows the window and analyzes again; closing the window only hides it,
and a cleaning goes on. Quit from the tray icon's menu or File › Exit.
`--new-window` opens a separate window even when rclean is resident. **File ›
Start with Windows** (Start When I Log In on Linux) starts it this way with
the session, for the current user only (the Run key, or
`~/.config/autostart/rclean.desktop`); choosing it again turns it off.

### What is cleaned

| Item | Where | Ticked at first |
| --- | --- | --- |
| Temporary files | `%TEMP%`: files not changed for a day | yes |
| Temporary files of Windows | `C:\Windows\Temp`: files not changed for a day (administrator) | yes |
| Windows Update downloads | `C:\Windows\SoftwareDistribution\Download` (administrator) | yes |
| Error reports | `%LOCALAPPDATA%\Microsoft\Windows\WER`, `%LOCALAPPDATA%\CrashDumps` | yes |
| Error reports of the system | `C:\ProgramData\Microsoft\Windows\WER\ReportArchive` and `ReportQueue` (administrator) | yes |
| Memory dumps | `C:\Windows\Minidump`, `C:\Windows\MEMORY.DMP` (administrator) | yes |
| Thumbnail cache | `thumbcache_*` in `%LOCALAPPDATA%\Microsoft\Windows\Explorer` | no |
| Graphics shader caches | DirectX (`D3DSCache`), NVIDIA and AMD | no |
| Recycle Bin | every drive's, through Windows | no |
| A browser's cache | each profile's `Cache`, `Code Cache`, `GPUCache`, Service Worker caches, and the shader caches: Chrome, Edge, Brave, Vivaldi, Opera, Chromium; Firefox's `cache2` and `startupCache`, also LibreWolf, Waterfox and Floorp | yes |

Only what is there to be thrown away: history, cookies, passwords, sessions
and settings of the browsers are never touched. A browser is listed when it
has a cache.

### Package managers

Each package manager found (on the `PATH`, or by its folder) has a group of
its own. Its cache is measured from its folders and cleaned by its own
command, so it keeps its records right; Cargo and vcpkg have no such command
and lose the folders.

| Manager | Item | Cleaned with | Ticked at first |
| --- | --- | --- | --- |
| Scoop | Downloads (`<scoop>\cache`) | `scoop cache rm *` | yes |
| Scoop | Old versions (versions kept after an update) | `scoop cleanup *` | no |
| Chocolatey | Cache | `choco cache remove` (administrator) | yes |
| pip | Cache | `python -m pip cache purge` | yes |
| npm | Cache (where `npm config get cache` says) | `npm cache clean --force` | yes |
| Yarn | Cache | `yarn cache clean` | yes |
| pnpm | Store: packages no project uses | `pnpm store prune` | no |
| Go | Build cache | `go clean -cache` | yes |
| NuGet | HTTP cache (the packages your projects use stay) | `dotnet nuget locals http-cache --clear` | yes |
| Cargo | `registry/cache`, `registry/src`, `git/checkouts` | deleted | yes |
| vcpkg | `downloads`, `buildtrees`, `packages` | deleted | yes |

On Linux also apt (`apt-get clean`), dnf (`dnf clean all`), zypper and pacman,
through `pkexec` (it asks for the password); Flatpak's unused runtimes
(`flatpak uninstall --unused`, size known only after) and Snap's old
revisions (`snap remove <name> --revision=<n>`), both unticked at first. When
a command does not work, the end says what it printed.

### Broken shortcuts

Shortcuts that open something gone, by where they are: the Start menu (yours,
and everyone's with administrator rights), the desktop (yours, and everyone's),
the taskbar and Quick Launch, Send To, and the recent items. On Linux, your
menu entries (`.desktop` files in `~/.local/share/applications`), the desktop,
autostart, and links to nothing in `~/.local/bin`. The details list each one
and what it opened.

A shortcut to the network, or to a drive that is not there now (a USB disk
unplugged), is not called broken: nobody can tell. Broken shortcuts go to the
Recycle Bin (the trash on Linux), so they can be put back.

### Invalid registry entries (Windows)

Entries that name a file or folder that is gone:

| Item | Where | Administrator |
| --- | --- | --- |
| Uninstall entries | `Uninstall` entries whose uninstaller and folder are gone (not Windows Installer's) | yours: no; the system's: yes |
| Startup entries | `Run` and `RunOnce` values whose program is gone | yours: no; the system's: yes |
| Program paths | `App Paths` whose program is gone | yes |
| Names of programs | `MuiCache`: names Explorer kept of programs gone | no |
| Open with | `Classes\Applications` whose programs are all gone | no |
| Shared files | `SharedDLLs` counts of files gone | yes |
| Installer folders | Windows Installer's list of folders, those gone | yes |
| Fonts | fonts whose file is gone | yes |

Only full paths on a drive that is there are checked: never relative names
(`rundll32.exe`), the network, or a drive unplugged; a 32-bit entry's
`System32` is looked for in `SysWOW64` too. Every entry is saved first to a
`.reg` file, in `backups\registry <date>` beside the settings (one file per
item); the end of the cleaning says where. Double-click a file to put its
entries back. The details list each entry and what it named.

Items marked "administrator" need administrator rights: without them they
are shown, dimmed, with **Restart as Administrator** above the list.

### The optional service

With the [rtsvc](../rtsvc/README.md) service installed (Windows, once, from an elevated prompt), the
items marked "administrator" show how many files and how much space they hold even when rclean was not
started as administrator (the status bar says "sizes of what needs rights: from the service"). They still
need administrator rights to be cleaned.

### How it deletes

- Temporary files and caches are deleted for good, not to the Recycle Bin:
  they are made to be thrown away, and would only fill it.
- Temporary folders: only files not changed for a day, so nothing a running
  program is using goes.
- A file in use cannot be deleted: it stays, and is counted ("in use stayed").
  When a browser is open, its row says so; close it first to clean all of it.
- Links and junctions inside are neither followed nor deleted. The folders
  themselves stay (only what is in them goes); folders inside that end up
  empty are removed.
- Read-only files are deleted too.

## Keyboard

| Key | Action |
| --- | --- |
| F5 | Analyze again |
| Ctrl+L | Clean what is ticked |
| Esc | Stop |
| ↑ ↓ PgUp PgDn Home End | Move in the list |
| Space | Tick or untick the row selected |
| Ctrl+A | Tick everything |
| Ctrl+E | Open the folder in Explorer |
| Ctrl+Shift+C | Copy the details |
| F1 | Keyboard shortcuts |

## Settings

The theme, the language, the window's size and the ticks are kept in
`settings.ini`, in `%APPDATA%\rclean` (Windows) or `~/.config/rclean` (Linux),
or next to the program when a file named `rclean.portable` is there.

## Build

```powershell
./dev.ps1 build --profile release-small -p rclean
./dev.ps1 test -p rclean
```

The output is `target\release-small\rclean.exe`.

## Linux

One file of 5.6 MB. The items are the user's: temporary files in `/tmp` and
`/var/tmp` (only yours, not changed for a day), thumbnails (`~/.cache/thumbnails`),
Mesa and NVIDIA shader caches, the trash (`~/.local/share/Trash`), and the
browsers' caches (in `~/.config` and `~/.cache`, Firefox also from Snap and
Flatpak), and the package managers above.

## Phases

1. Done: temporary files of Windows and Linux, error reports, memory dumps,
   thumbnails, shader caches, the Recycle Bin and the browsers' caches;
   analyze, tick and clean.
2. Done: the caches of package managers, through their own commands: Scoop,
   Chocolatey, pip, npm, Yarn, pnpm, Go, NuGet, Cargo and vcpkg; apt, dnf,
   zypper, pacman, Flatpak and Snap on Linux.
3. Done: broken shortcuts (Start menu, desktop, taskbar, Send To, recent
   items; Linux `.desktop` files and links), to the Recycle Bin.
4. Done: invalid registry entries (uninstall, startup, App Paths, MuiCache,
   Open with, SharedDLLs, Installer folders, fonts), saved to a `.reg` file
   before they are removed.
