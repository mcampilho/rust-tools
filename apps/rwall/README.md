# rwall

A portable wallpaper changer for Windows and [Linux](#linux), written in Rust.
It puts a new picture on the desktop every day, twice or four times a day,
every week, every month or every so many minutes. The pictures come from
Bing's picture of the day, NASA's Astronomy Picture of the Day, Wallhaven,
Lorem Picsum or your own folders.

[Versão portuguesa](README.pt.md)

rwall is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`rwall.exe` is a single 2.9 MB executable: no installer, no runtime, no DLLs
beyond those that ship with Windows. It is the only tool of rust-tools that uses
the network: it downloads pictures from the services you turn on, and nothing
else. No account, no cookies, no telemetry.

## Use

```text
rwall [--tray | --auto]
```

| Command | What it does |
| --- | --- |
| `rwall` | Opens the settings: the picture on the desktop now, **Next picture**, where pictures come from, how often and how they fill the screen. Closing the window ends rwall. The picture changes only when asked: the chosen frequency needs `--tray`. |
| `rwall --tray` | Stays in the notification area and changes the picture as often as chosen. |
| `rwall --auto` | Changes the picture once with the saved settings, without a window, and ends: for the Task Scheduler, a shortcut or a script. With rwall already running, that rwall changes it. |
| `rwall --help` | Prints this in the console it was typed in, or shows it in a message box. |

Only one rwall runs at a time: launching it again shows the settings of the
one that is running.

**Resident mode** (`--tray`): a click on the icon in the notification area
opens the settings; its menu (right click) has **Next picture**, **About this
picture** (the page about it, in the browser), **Settings…**, **Show the
picture file** and **Exit**. Closing the settings only hides them (Esc too). The
first time rwall starts this way, it shows its settings and puts Bing's picture
of the day on the desktop. **Start with Windows**, in the settings, starts it
this way.

In the settings window, Esc closes it and F5 is **Next picture**.

### When the picture changes (with `--tray`)

| Choice | Changes at |
| --- | --- |
| Every day (default) | midnight |
| Twice a day | 00:00 and 12:00 |
| Four times a day | 00:00, 06:00, 12:00 and 18:00 |
| Every week | Monday at midnight |
| Every month | the 1st of the month at midnight |
| Every … min | every 5 to 480 minutes (−/+), counted from midnight |

The times are the computer's local time. rwall checks the clock every half
minute: when it was asleep, off or not running at the time of a change, the
picture changes as soon as it is back. Choosing another frequency does not
change the picture at once; it counts from then on. **Next picture** changes it
any time, and the next scheduled change still comes at its time.

When a change fails (no network, a service down), the status line in the
settings says why, and rwall tries again ten minutes later.

### Where pictures come from

Tick any number of sources. They take turns: Bing, then NASA, then your
folders, and so on. When one fails, the next one is tried at once. A picture
shown lately (the last 200) is not picked again while there are others.

| Source | What it gives | Settings |
| --- | --- | --- |
| Bing – picture of the day | The picture of bing.com in UHD, with its place and credit; one of the last eight days when today's was shown | **Region**: the Bing market, such as `pt-PT` or `en-GB` (empty: rwall's language) |
| NASA – Astronomy Picture of the Day | Today's picture, or a day of the archive at random (days with a video are passed over) | **API key**: empty uses `DEMO_KEY`, which allows a few requests an hour; a free key comes from [api.nasa.gov](https://api.nasa.gov) |
| Wallhaven – search | A wallpaper at random, safe for work, landscape, at least the size of the screen | **Search**: words such as `mountains`; empty: anything |
| Picsum – random photo | A random photo at the exact size of the screen, with its author | – |
| My folders | A picture at random from your folders (and their subfolders, if ticked) | **Add a folder…**, × to take one off |

The NASA API is often slow or answers with server errors for a while; rwall
asks again twice, then moves on to the next source.

Pictures in folders can be JPEG, PNG, BMP, GIF, TIFF or WebP. Downloaded
pictures are checked: anything that is not a picture, or is smaller than
320×320, is refused.

### Picture position and the rest

- **Picture position**: Fill (covers the screen, cutting what sticks out), Fit
  (all of it, with bars), Stretch, Center, or Span (one picture across all
  screens). It applies at once to the picture on the desktop.
- **Downloaded pictures to keep**: the newest 30 by default (1 to 500); older
  ones are deleted after each change. Pictures from your folders are never
  touched.
- **Start with Windows** adds `"rwall.exe" --tray` to
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (current user only);
  unticking it removes it. On Linux it is **Start when I log in**, a file in
  `~/.config/autostart`.

## How it works

- **Downloads**: WinHTTP, the system's HTTPS with its proxy settings (the
  `curl` program on Linux), with a 60-second timeout and size limits (4 MB for
  an answer, 64 MB for a picture). The services' answers are read with a small
  JSON reader of rwall's own.
- **The desktop**: `SystemParametersInfoW(SPI_SETDESKWALLPAPER)` with the
  position in `HKCU\Control Panel\Desktop` (`WallpaperStyle`), as Windows'
  own settings do. The same picture goes on every screen. WebP pictures, which
  Windows may not read without an extra codec, are converted to PNG first.
- **Files**: settings in `settings.ini` and downloaded pictures in `images\`,
  both in `%APPDATA%\rwall` (Windows) or `~/.config/rwall` (Linux), or next to
  `rwall.exe` when a file named `rwall.portable` is there (portable mode).

## Linux

The same settings window and, with `--tray`, the same menu through a
StatusNotifierItem icon. Without a host for it (plain GNOME), a resident rwall
keeps its window, minimized when closed. The picture is set with `gsettings`
(GNOME, Cinnamon and their relatives), `plasma-apply-wallpaperimage` (KDE
Plasma), `xfconf-query` (XFCE) or `feh` (other window managers), and downloads
use the `curl` program, which must be installed. Built and tested under WSL2,
not yet run on a Linux desktop.

## Build and test

```powershell
./dev.ps1 build --release -p rwall          # target\release\rwall.exe
./dev.ps1 test -p rwall
./dev.ps1 clippy -p rwall --all-targets
./dev.ps1 test -p rwall --lib screenshots '--' --ignored        # apps\rwall\target\screens\*.png
./dev.ps1 test -p rwall --lib live_services '--' --ignored --nocapture   # needs the network
```

rwall uses the `release` profile (not `release-small`): a malformed picture
from the web must not close it, so panics unwind and are caught.

## License

MIT. See [LICENSE](../../LICENSE). The pictures belong to their authors: see
the credit shown for each one, and the terms of each service.
