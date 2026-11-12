# rwall

A portable wallpaper changer for Windows and [Linux](#linux), written in Rust.
It puts a new picture on the desktop every day, twice or four times a day,
every week, every month or every so many minutes. The pictures come from
Bing's picture of the day, NASA's Astronomy Picture of the Day, Wallhaven,
Lorem Picsum or your own folders. On Windows it can also play a **dynamic
wallpaper** behind the desktop icons (a video, an animated GIF or WebP, or a
scene drawn by rwall) and put **widgets** on the desktop: clocks, a calendar,
sticky notes, annotations, the weather, fences and photos.

[Versão portuguesa](README.pt.md)

rwall is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`rwall.exe` is a single 3.7 MB executable: no installer, no runtime, no DLLs
beyond those that ship with Windows. It is the only tool of rust-tools that uses
the network: it downloads pictures from the services you turn on, and the
weather for a weather widget, and nothing else. No account, no cookies, no
telemetry.

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
of the day on the desktop. **Start with Windows**, at the top right of the
settings beside the tabs, starts it this way.

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
| NASA – Astronomy Picture of the Day | The latest day not shown yet, or a day of the archive at random (days with a video are passed over), with its page on science.nasa.gov | – (no key needed) |
| Wallhaven – search | A wallpaper at random, safe for work, landscape, at least the size of the screen | **Search**: words such as `mountains`; empty: anything |
| Picsum – random photo | A random photo at the exact size of the screen, with its author | – |
| My folders | A picture at random from your folders (and their subfolders, if ticked) | **Add a folder…**, × to take one off |

NASA's pictures come from the APOD list of science.nasa.gov
(`wp-json/wp/v2/apod-basic`), where APOD moved from `api.nasa.gov`. After a
server error, rwall asks again twice, then moves on to the next source.

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
- **Start with Windows** (beside the tabs, for all of rwall) adds `"rwall.exe" --tray` to
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (current user only);
  unticking it removes it. On Linux it is **Start when I log in**, a file in
  `~/.config/autostart`.

## Dynamic wallpaper (Windows)

The **Dynamic wallpaper** tab of the settings plays something behind the
desktop icons, in place of the still picture:

| Choice | What plays |
| --- | --- |
| A video | One video file (MP4, MOV, WMV, AVI, MKV, WebM…: what Windows can play), in a loop, without sound |
| Videos from a folder | A video at random from a folder (and its subfolders, if ticked); with `--tray`, another one at each change chosen on the Static wallpaper tab. **Next video** changes it any time |
| An animated picture | A GIF or an animated WebP, at its own speed |
| A scene drawn by rwall | **Northern lights**, **Starry sky** (it turns slowly, with a shooting star now and then), **Sunset sea** or **Color flow**: no file needed |

Icons, files, notifications, widgets and the taskbar stay over it, as always:
only the picture behind them moves. **Turn on** and **Turn off** start and
stop it; when it stops, the still picture comes back. While it plays, the
still picture does not change on its own.

- **Position on each screen**: Fill (covers each screen, cutting what sticks
  out) or Fit (all of it, with black bars). With several screens, each shows
  the whole video.
- **Pause when** (all ticked by default): a program or a game is in full
  screen; maximized windows cover every screen; the computer is locked or the
  screen is off; the computer runs on battery. Paused, the last frame stays.
  The menu of the icon also has **Pause the dynamic wallpaper**.
- It plays while rwall runs: with `--tray` all the time (and from the start
  with **Start with Windows**), without it while the settings window is open.
  `--auto` does not touch it.

It costs little: a video is decoded by the graphics card (about 1–2 % of one
processor core for a 1080p video), and the frames of a scene or an animation
are made small and stretched by the graphics card (under 10 % of one core).

## Widgets (Windows)

The **Widgets** tab of the settings adds widgets to the desktop and lists them.
They sit over the wallpaper and the icons and under every window, as if they
were part of the desktop. Each one can instead be **Always on top**, over
every window.

| Widget | What it shows |
| --- | --- |
| Analog clock | A round clock face, with the day; the seconds hand can be turned off |
| Digital clock | The time, large (24-hour or AM/PM, with or without seconds), and the date |
| Calendar | The month, with today marked and the national holidays of the country of Windows' regional settings in red (the next one, or the one under the pointer, named under the month); ‹ › for the months before and after, a click on the title goes back to today |
| Clock and calendar | The two side by side |
| Sticky note | A coloured note to write in (six colours); its text is kept as typed |
| Annotation | Text straight on the wallpaper, without a box, in three sizes and four colours: labels and reminders |
| Weather | The weather now and for the next three days, for a city typed in the tab (`Porto` or `Porto, Portugal`; at first, the city of the computer's time zone), in °C or °F |
| Fence | A box showing the files and folders of a folder, with Explorer's icons and thumbnails |
| Photos | The photos of a folder (and its subfolders), one after another, every 1 to 240 minutes |

- **Move** a widget by dragging it (a note by its top bar); it snaps to the
  edges of the screen. **Resize** it by dragging its bottom right corner.
  **Lock position** (in its menu) keeps it from moving by mistake.
- **Right-click** a widget for its options: colours and sizes, seconds and
  24-hour time, Refresh and °F for the weather, Next photo, Always on top, Lock
  position, Widget settings… and Remove the widget.
- **Notes and annotations**: click a note to write in it (double-click an
  annotation); arrows, Home/End, Ctrl+A, Ctrl+C/X/V, Ctrl+Backspace work as
  usual; Esc or a click elsewhere ends writing. The text is saved as you type.
- **Fences**: a double-click opens a file or folder; files can be dragged out
  of a fence (to the desktop, a folder, a program) and dropped into one. A file
  dropped from the same drive is moved there, from another drive copied (Ctrl
  copies, Shift moves), with Explorer's own progress, questions and undo.
  Hidden and system files stay hidden.
- **Weather**: data by [Open-Meteo.com](https://open-meteo.com/) (free, no key,
  no account), asked again every half hour. The city is looked for once; the
  most populous place of that name is taken, unless a region or country is
  typed after a comma.
- **Regional settings**: a new clock is 24-hour or AM/PM, and a new weather
  widget in °C or °F, as Windows' regional format says; calendars start the
  week on its first day (Saturday, Sunday or Monday). The weather starts with
  the city of the time zone (`Lisbon, PT` for Europe/Lisbon), which can be
  changed.
- **Holidays**: national public holidays of Portugal, Brazil, Spain, Mexico,
  the United States, the United Kingdom (England and Wales), France, Germany
  and Italy, worked out for any year (regional and municipal ones are left
  out). **Holidays** in the tab turns them off for a calendar.
- **How many**: one clock, calendar and weather of each kind (their Add
  button is greyed out while there is one), any number of notes,
  annotations, fences and photos, up to 16 widgets in all. A new widget goes
  to the first free place from the top right corner of the main screen.
- The widgets show while rwall runs: with `--tray` all the time, without it
  while the settings window is open. **Show the widgets on the desktop** hides
  or shows them all.

They cost little: nine widgets, three of them with seconds, use under 1 % of
one processor core and about 36 MB.

## How it works

- **Downloads**: WinHTTP, the system's HTTPS with its proxy settings (the
  `curl` program on Linux), with a 60-second timeout and size limits (4 MB for
  an answer, 64 MB for a picture). The services' answers are read with a small
  JSON reader of rwall's own.
- **The desktop**: `SystemParametersInfoW(SPI_SETDESKWALLPAPER)` with the
  position in `HKCU\Control Panel\Desktop` (`WallpaperStyle`), as Windows'
  own settings do. The same picture goes on every screen. WebP pictures, which
  Windows may not read without an extra codec, are converted to PNG first.
- **Behind the icons**: Windows has no public way to draw there. Like every
  live wallpaper program, rwall sends the desktop window (`Progman`) the
  undocumented message `0x052C`, which makes Explorer put a `WorkerW` window
  behind the icon layer (`SHELLDLL_DefView`); rwall's windows (one per screen)
  become its children. Since Windows 11 24H2 the icon layer stays inside
  `Progman`: there rwall's windows go just below it. When Explorer restarts
  or the screens change, rwall puts them back. If rwall is ended by force,
  the last frame stays until the wallpaper is painted again (by rwall or by
  Windows).
- **Playing**: Media Foundation's media engine plays the video into a
  Direct3D 11 swap chain per screen. Scenes and animations are drawn on the
  processor at 960×540 (or smaller) and stretched by the swap chain.
- **Widgets**: each one is a layered window (`WS_EX_LAYERED`, per-pixel
  transparency through `UpdateLayeredWindow`), drawn by rwall itself. A widget
  pinned to the desktop is *owned* by the window that holds the icons, so
  Windows keeps it just above them; rwall holds its place among the windows,
  even while you write in a note. Fences use OLE drag and drop and Explorer's
  file operations (`SHFileOperationW`). When Explorer restarts or the screens
  change, the widgets come back where they were.
- **Files**: settings in `settings.ini` and downloaded pictures in `images\`,
  both in `%APPDATA%\rwall` (Windows) or `~/.config/rwall` (Linux), or next to
  `rwall.exe` when a file named `rwall.portable` is there (portable mode).

## Linux

The same settings window (without the dynamic wallpaper and the widgets,
Windows only for now) and, with `--tray`, the same menu through a
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
./dev.ps1 test -p rwall --lib scene_pictures '--' --ignored      # a frame of each scene, flow.gif
./dev.ps1 test -p rwall --release --lib scene_speed '--' --ignored --nocapture
./dev.ps1 test -p rwall --lib widget_pictures '--' --ignored     # every widget, light, dark and at 150 %
./dev.ps1 test -p rwall --lib weather_live '--' --ignored --nocapture   # Open-Meteo, needs the network
./dev.ps1 test -p rwall --lib pinned_window_order '--' --ignored --nocapture
```

rwall uses the `release` profile (not `release-small`): a malformed picture
from the web must not close it, so panics unwind and are caught.

## License

MIT. See [LICENSE](../../LICENSE). The pictures belong to their authors: see
the credit shown for each one, and the terms of each service.
