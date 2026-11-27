# rlaunch

A portable application menu for Windows and Linux, written in Rust. It gathers
the programs installed with Scoop, those of the Start menu and those of your
own folders into one menu, by category, with a search box.

[Versão portuguesa](README.pt.md)

rlaunch is part of [rust-tools](../../README.md). Commands below run from the
repository root. **It is being built in phases**: see [Phases](#phases).

`rlaunch.exe` is a single executable of about 1.9 MB: no installer, no
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
  goes back. A category can have subgroups, listed first (see
  [Categories](#categories)).
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

**Programs** page:

- **Programs from:** the shortcuts Scoop makes in the Start menu
  (`Scoop Apps`), the rest of the Start menu, and **My folders**: any folder of
  programs or shortcuts, followed three levels down (see
  [PortableApps](#portableapps)).
- **Start with Windows** and **Always start as administrator** (Windows):
  Windows asks at every start, also when rlaunch starts with the session; if you
  say no, it starts without those rights.
- **Keyboard shortcut:** Ctrl+Alt+Space, Ctrl+Shift+Space, Alt+Shift+Space,
  Win+Shift+Space or off. When another program already holds the combination,
  the menu says so: choose another.
- Language.

**Appearance** page:

- **Theme** (light, dark or as the system) and **Color** of the frame: seven
  colors or one made from the system's accent.
- **Size:** 100% (350×480), 115% or 130%. **Rows:** compact, or large with
  bigger icons.
- **Transparency** (Windows): off, or 10% to 40%.
- **Position** (on Linux, X11 only, and without "at the pointer"): right (by the notification area), left, center or at
  the pointer.

For colors of your own, put a `theme.ini` file beside `settings.ini`. It is read
each time the menu opens; each line replaces one color, and those left out stay:

```ini
frame=#202040
accent=#FF8800
list=#FFFFFF
text=#101010
```

The names are `frame`, `frame_text`, `frame_dim`, `frame_hover`, `list`, `text`,
`dim`, `border`, `hover`, `accent`, `on_accent` and `warn`.

Uninstallers, manuals and web page shortcuts are left out, and a program with
the same name in two places is shown once (Scoop's first). Where one folder has
`x.exe`, `x64.exe` and `x64a.exe` (as Sysinternals does), only the one that
suits the computer is listed: `x64.exe` on a 64 bit PC, `x64a.exe` on ARM.

**Console programs** (Windows): an `.exe` without a window of its own opens in a
console that stays open, in its folder, so its output can be read and it can be
run again with arguments. Right-click > **Keep the console open** turns this on
or off per program (for `.bat` and `.cmd` too; shortcuts open as Windows opens
them).

**Run with arguments…** (Windows; right-click an `.exe`, `.bat`, `.cmd` or
shortcut): type the arguments in the program's row and press Enter. The last
eight are kept per program: the latest is offered, and Up/Down go through the
others. Esc, or a click elsewhere, gives up. In a console the line is the
console's, so `|` and `>` work.

## Categories

There are ten built-in categories, guessed from the names of well-known programs
(the others go to **Other**), and your own. There are two levels: category and
subgroup.

**From your folders.** Each folder under **Options > My folders** has a button
that chooses where its categories come from:

- **Automatic** (the default): when shortcuts are arranged in subfolders, as
  **Subfolders**; else **By name**.
- **Subfolders:** the subfolder is the category and a folder inside it the
  subgroup (`links\Office\LibreOffice`). What is loose at the top is guessed.
- **One category:** the folder itself is the category (`SysInternals`, say), and
  its subfolders the subgroups.
- **By name:** the folders do not count.

A program Scoop also has goes in the category your folder gives it.

**From the Start menu.** A subfolder of it with four or more programs
(`Administrative Tools`, `Chrome Apps`) is a subgroup, in the category of most of
them.

**Dragging.** A category of yours dropped on the middle of another becomes its
subcategory (any subgroups it had become one, until it comes out again); a
subgroup dropped on the back arrow becomes a category. A program dropped on a
category moves there.

**Several at once.** Ctrl+click marks and unmarks, Shift+click or Shift+arrows
marks a range, Ctrl+A marks all in the list (in a search too). Then drag them,
or right-click > **Move N programs to**, **Add to favorites** or **Hide**. Esc
unmarks.

**Order.** **Options > Appearance > Order**: **Alphabetical** (Other last) or
**My order**, where categories are dragged between others (the top or bottom
edge of a row; the middle still puts one inside) or moved with **Move up/down**
in their right-click menu.

**By hand.** Right-click a program > **Category** picks another, or **New
category…** (type `Name` or `Name/Subgroup`). Right-click a category of yours or
a subgroup: **Rename** or **Remove category** (the programs of a subgroup stay in
the category; those of a category go to **Other**, or back to the guess if you
had put them there). Your choice wins over the folder, and the folder over the
guess. Nothing changes in the folders: it is kept in rlaunch's settings.

## PortableApps

A folder with apps in the [PortableApps.com](https://portableapps.com) format is
read as such, wherever it is among your folders: each app gives its own name
(without "Portable"), category and programs, and nothing else inside it is
listed. Adding the folder (or the drive's folder that holds `PortableApps`) also
brings the favorites, and the renamed, moved and hidden programs, of the
PortableApps menu; what you already chose in rlaunch stays. The suite's files are
only read.

Other suites are not read in their own format: add their folder of programs.

### On a pen drive

Create an empty file named `rlaunch.portable` beside `rlaunch.exe`: settings
are then kept there, folders on the same drive are found again when the drive
gets another letter, **Explore** opens the drive and the bar shows its space.
Turn off Scoop and the Start menu to show only what is on the pen. The first
time, a PortableApps folder beside rlaunch's, or at the usual places of the
drive, is found and added.

Without that file, settings are in `%APPDATA%\rlaunch`.

## Linux

The menu reads the installed applications (`.desktop` files: their names in the
menu's language, their categories and icons, PNG or SVG, from the desktop's icon
theme; those hidden from your desktop are left out) and your folders, where it
lists `.desktop` files, executable programs, scripts and AppImages. A program of
a folder starts in that folder. Documents, Downloads and the others follow the
names in `user-dirs.dirs`.

What differs from Windows:

- no keyboard shortcut of its own: bind one in the desktop's settings to
  `rlaunch` (starting it again opens the menu);
- **Position** works on X11; on Wayland the desktop places the window;
- dragging files onto the menu works on X11, not on Wayland;
- no transparency and no administrator options;
- the menu stays in the notification area where there is one (StatusNotifierItem;
  on GNOME, with an extension). Without one, closing the menu quits.

## Phases

1. **Done:** the menu in the notification area with the keyboard shortcut,
   Scoop, Start menu and own folders, categories, search, opening programs.
2. **Done:** favourites, hiding, renaming, changing category and icon, adding a
   single program, running as administrator.
3. **Done:** themes and looks.
4. **Done for PortableApps:** its apps and the choices of its menu. Other suites
   are not read in their own format yet.
5. **Done:** Linux in full (icons, programs of folders).
6. **Done:** categories from folders, the Start menu and your own, on two
   levels; dragging, an order of your own, marking several, consoles.

## Build

```powershell
./dev.ps1 build --profile release-small -p rlaunch
./dev.ps1 test -p rlaunch
```
