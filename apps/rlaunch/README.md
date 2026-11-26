# rlaunch

A portable application menu for Windows and Linux, written in Rust. It gathers
the programs installed with Scoop, those of the Start menu and those of your
own folders into one menu, by category, with a search box.

[Versão portuguesa](README.pt.md)

rlaunch is part of [rust-tools](../../README.md). Commands below run from the
repository root. **It is being built in phases**: see [Phases](#phases).

`rlaunch.exe` is a single executable of about 1.7 MB: no installer, no
runtime. It makes no network connections (except **Check for updates**, when
you choose it) and has no telemetry.

## Use

```text
rlaunch [--tray]
```

rlaunch stays in the notification area. The menu opens from the icon, with the
keyboard shortcut (**Ctrl+Alt+Space**) or by starting rlaunch again, by the
notification area of the screen the pointer is on, and closes when something else gets the
focus. `--tray` starts it without opening the menu.

- **Left:** the categories; a click (or Enter) opens one, the arrow at its top
  goes back.
- **Search:** just type. Accents do not matter. Up, Down and Enter choose;
  Esc clears, goes back, and then closes.
- **Right:** your Documents, Downloads, Pictures, Music and Videos folders,
  **Explore**, Options, About and Exit.
- **Bottom:** the free space of the drive.
- **Favourites** stay at the top, above the categories.
- **Pin** (beside the search box): the menu stays open when something else gets
  the focus, so files can be dragged onto it.

### Your programs

Right-click a program (or press the Menu key, or Shift+F10): **Open**, **Run as
administrator** (also Ctrl+Shift+Enter; Windows), **Always run as
administrator** (the program gets a shield), **Open file location**, **Add
to favorites**, **Rename** (an empty name gives back the original), **Category**,
**Change icon…** (`.ico`, `.png`, a program or a shortcut) and **Hide**. Hidden
programs are under **Options > Hidden**, where the same menu shows them again.

**Options > Add a program…** adds any file by itself, a program or a document;
dragging it onto the pinned menu does the same (a dragged folder joins your
folders). Its menu has **Remove from the menu**. The shortcuts and programs are
never changed: what you choose is kept in rlaunch's settings.

**Admin mode**, in the right column (or **Restart as administrator** in the
icon's menu), starts rlaunch again with administrator rights; **Exit** and start
it again to go back.

When rlaunch itself runs as administrator, a shield takes the place of its
logo and a line says so: what it opens has those rights too.

### Options

- **Programs from:** the shortcuts Scoop makes in the Start menu
  (`Scoop Apps`), the rest of the Start menu, and **My folders**: any folder of
  programs or shortcuts, followed three levels down.
- **Size:** 100% (350×480), 115% or 130%.
- **Transparency** (Windows): off, or 10% to 40%.
- **Keyboard shortcut:** Ctrl+Alt+Space, Ctrl+Shift+Space, Alt+Shift+Space,
  Win+Shift+Space or off. When another program already holds the combination,
  the menu says so: choose another.
- **Always start as administrator** (Windows): Windows asks at every start,
  also when rlaunch starts with the session; if you say no, it starts without
  those rights.
- Start with Windows, theme, language.

Categories are guessed from the names of well-known programs; the others go to
**Other**. Uninstallers, manuals and web page shortcuts are left out, and a
program with the same name in two places is shown once (Scoop's first).

### On a pen drive

Create an empty file named `rlaunch.portable` beside `rlaunch.exe`: settings
are then kept there, folders on the same drive are found again when the drive
gets another letter, **Explore** opens the drive and the bar shows its space.
Turn off Scoop and the Start menu to show only what is on the pen.

Without that file, settings are in `%APPDATA%\rlaunch`.

## Linux

The menu reads the installed applications (`.desktop` files, with their own
categories) and your folders. There is no keyboard shortcut of its own (bind
one in the desktop's settings to `rlaunch`), and icons are not shown yet.
Without a notification area, closing the menu quits.

## Phases

1. **Done:** the menu in the notification area with the keyboard shortcut,
   Scoop, Start menu and own folders, categories, search, opening programs.
2. **Done:** favourites, hiding, renaming, changing category and icon, adding a
   single program, running as administrator.
3. Themes and looks.
4. PortableApps (names and categories of each program) and importing from
   other portable suites.
5. Linux in full (icons).

## Build

```powershell
./dev.ps1 build --profile release-small -p rlaunch
./dev.ps1 test -p rlaunch
```
