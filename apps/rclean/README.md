# rclean

A portable cleaner for Windows and Linux, written in Rust. It shows how much
the temporary files of the system and the caches of the browsers take, and
deletes what you tick, after asking.

[Versão portuguesa](README.pt.md)

rclean is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`rclean.exe` is a single 1.6 MB executable: no installer, no runtime, no DLLs
beyond those that ship with Windows. It makes no network connections and has
no telemetry. It is being built in phases: this first one cleans temporary
files and browser caches; the caches of package managers, broken shortcuts and
invalid registry entries come next (see [Phases](#phases)).

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

Items marked "administrator" need administrator rights: without them they
are shown, dimmed, with **Restart as Administrator** above the list.

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
| Ctrl+E | Open the folder |
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

One file of 5.5 MB. The items are the user's: temporary files in `/tmp` and
`/var/tmp` (only yours, not changed for a day), thumbnails (`~/.cache/thumbnails`),
Mesa and NVIDIA shader caches, the trash (`~/.local/share/Trash`), and the
browsers' caches (in `~/.config` and `~/.cache`, Firefox also from Snap and
Flatpak). The system's own (`apt`, the journal) come with the package
managers, in phase 2.

## Phases

1. Done: temporary files of Windows and Linux, error reports, memory dumps,
   thumbnails, shader caches, the Recycle Bin and the browsers' caches;
   analyze, tick and clean.
2. The caches of package managers, through their own commands: Scoop
   (`scoop cache rm`, `scoop cleanup`), Chocolatey, pip, npm, Cargo, vcpkg,
   winget; apt, dnf, Flatpak and Snap on Linux.
3. Broken shortcuts (Start menu, desktop, Linux `.desktop` files).
4. Invalid registry entries, saved to a `.reg` file before they are removed.
