# runinst

A portable uninstaller for Windows, written in Rust. It lists the programs
installed on the computer for every user (from the registry, Windows Installer,
the Store and winget, in one list; Scoop and Chocolatey in tabs of their own),
with their size, version, publisher, uninstall command, registry entry, newer
version and the shortcuts that belong to them on the Start menu and the desktop;
and uninstalls them, one or several in a row, then finds what they left behind.

[Versão portuguesa](README.pt.md)

runinst is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`runinst.exe` is a single 2.1 MB executable: no installer, no runtime, no DLLs
beyond those that ship with Windows. It makes no network connections and has
no telemetry. It is being built in phases: it lists, uninstalls and removes
the leftovers, also of programs removed before; the optional features of
Windows and the packages of development tools come next (see [Next](#next)).

## Use

```text
runinst
```

### What is listed

runinst joins in one list, without repeats, what Windows and its tools know
of the programs installed; the **Source** column says where each was found:

- **Registry**: the `Uninstall` entries ("Apps and features"), the same as
  `REG QUERY …\Uninstall`: the system's, in the 64-bit and the 32-bit views;
  your own (`HKEY_CURRENT_USER`, and its `WOW6432Node`); and other users'
  (below).
- **Installer**: the products of Windows Installer, from its own records
  (`MsiEnumProductsEx`): what `Get-WmiObject Win32_Product` and
  `wmic product` list, without their side effect (that WMI class has Windows
  Installer check, and at times repair, every product). A product with an
  `Uninstall` entry is the same program; one without is listed as a system
  component.
- **Store**: the Store and MSIX packages, as `Get-AppxPackage` lists them,
  from the package manager of Windows: ChatGPT, Windows Terminal, Outlook, the
  apps of Windows. They have no `Uninstall` entry. Frameworks (VCLibs, Windows
  App Runtime…), the packages signed as part of Windows, and those without an
  app to start are system components.
- **winget**: `winget list`, which takes seconds, so it is asked after the
  rest is shown (the status bar says so meanwhile). Its rows are joined by the
  registry key or the package they name, or else by name; they bring winget's
  id and the **newer version** its sources have (in blue in the Version column,
  `9.11 → 9.12`, and counted in the status bar).

For whom:

- **For everyone**: the system's entries and products; any user can read them.
- **For you**: your own entries, products and packages (Chrome, VS Code,
  ChatGPT and the like often are).
- **For other users**: their entries under `HKEY_USERS` when they have a
  session open, and otherwise from their profile's `NTUSER.DAT`, loaded on its
  own and only to be read; their products and packages. This needs
  administrator rights: without them, a line above the list names the users
  who could not be read, with **Restart as Administrator**.

When Windows asks for an administrator's password (another account), runinst
runs as that account but still knows you as the user of the desktop: your
programs, shortcuts and Scoop stay yours, and are not listed twice.

**Scoop** and **Chocolatey**, when they are installed, have a tab each: the
apps in Scoop's folders (`SCOOP`, or `~\scoop`, and the global one) with their
version, bucket, description and home page; the packages in Chocolatey's
`lib` folder with their version and authors. They are read from the folders,
without running `scoop` or `choco`.

Shortcuts are read from the Start menu and the desktop of everyone
(`C:\ProgramData\…\Start Menu`, `C:\Users\Public\Desktop`), of you, and, with
administrator rights, of the other users. runinst reads each `.lnk` file
itself, and ties each shortcut to its program by the folder of what it opens
(the program's install folder, or the folder of its icon or of its
uninstaller), or else by its name. A shortcut whose target is gone is shown in
orange.

Not listed: the optional features of Windows (`dism /online /get-features`,
which needs administrator rights) and the packages of development tools (npm,
pip, Cargo, vcpkg): later.

### Uninstalling

Select one program, or several (Ctrl+click, Shift+click, Ctrl+A), or tick
their boxes (a click on the box, or Space for the rows selected; the box in the
header ticks all shown or unticks all). Ticks stay while the search changes,
so you can find programs one by one and tick each; while any are ticked, the
buttons act on them. Then choose
**Uninstall** (Delete) or **Uninstall Quietly** (Shift+Delete), from the
buttons above the list, the **Program** menu or the right-click menu. runinst
says which will be uninstalled and which cannot, and asks first. Then it runs
one uninstaller at a time and waits until it ends, together with the processes
it starts (many uninstallers copy themselves to the temporary folder and start
that copy); the line above the list says which is running, with a bar of how
far the queue is and **Stop after this one**, and each program in the queue is
marked in the list (Waiting, Uninstalling…, Done, Did not work). At the end it reads the programs again and says what was
uninstalled, what is still installed (an uninstaller that was cancelled), what
did not work and what was not started; and whether Windows must restart.

What it runs:

- the program's own uninstaller (`UninstallString`); quietly, its
  `QuietUninstallString`, or `/VERYSILENT` for Inno Setup's; without either,
  the normal one;
- `msiexec /x {code}` for Windows Installer (`/qn` quietly);
- for Store apps, the package manager of Windows;
- `scoop uninstall` and `choco uninstall` in their tabs;
- `winget uninstall --id` for what only winget knows.

An uninstaller that needs administrator rights gets Windows' question.
Programs installed for another user are uninstalled from their own account;
so are your Store apps when runinst runs as another account.
**Modify or Repair** runs the program's change command (`ModifyPath`, or
Windows Installer's maintenance); for a Scoop app, `scoop install` again when its
install failed, else `scoop reset` (its shims, shortcuts and current version
again). Few programs have one (Microsoft 365,
Edge, Visual C++ and Windows Installer packages do); for the others, as for
Chocolatey and Store apps, and when the entry says `NoModify` and `NoRepair`,
it says why it cannot.

Programs with a problem have a tag in the list, and the status bar counts
them: **Install failed** (a Scoop app whose install did not finish: no
`install.json`), **Manifest removed** (no bucket has a Scoop app's manifest
any more), **Uninstaller missing** (an entry whose uninstaller and folder are
gone: left by a program removed some other way).

### Leftovers

After the programs are uninstalled, runinst looks for what they left behind
and shows it in a **Leftovers** tab (it stays on the tab you were on; closing
the list goes back to it):

- **Program folder**: the install folder, if it is still there; folders named
  after the program in Program Files, Common Files, ProgramData and
  `AppData\Local\Programs`; its publisher's folder when it holds nothing else
  and no other program of the publisher is installed. For a Scoop app, its
  folder.
- **Shortcut**: Start menu folders named after it, and shortcuts that open
  something it had.
- **Starts with Windows**: `Run` entries that start something it had.
- **Registry**: its keys under `HKEY_LOCAL_MACHINE\SOFTWARE` (and
  `WOW6432Node`), and its `Uninstall` entry when its uninstaller is gone.
- **Service** and **Scheduled task**: those that run something in its folder,
  or something gone with its name. Tasks are read from
  `Windows\System32\Tasks`, which needs administrator rights for most.
- **User settings**: its folders in AppData (Local, Roaming, LocalLow), its key
  under the user's `Software`, and a Scoop app's `persist` folder. These are not
  ticked at first: settings are often worth keeping.
- **Found by Everything**: with [Everything](https://www.voidtools.com) running,
  folders of its name elsewhere on the system's drive or the program's. Only
  names of more than one word, or inside the publisher's folder, count; never
  in other users' profiles, package managers, projects, caches or backups. Not
  ticked at first.

Nothing a program still installed uses is offered: not its folder, nothing in
it, nothing holding it; never Windows' folders, a profile or a drive. What needs
administrator rights runinst does not have is shown greyed, and cannot be
ticked.

Tick what should go (a click on the box, or Space) and choose **Remove the
Ticked Ones** (Delete). runinst asks first. Registry keys and values, and the
keys of services, are saved to a `.reg` file in the settings folder
(`backups\leftovers <date>.reg`): double-click it to put them back. A service
is then stopped and deleted (`sc delete`); a task's file is copied to
`backups\leftovers <date>.tasks\` (`schtasks /Create /XML` puts it back) and the
task deleted. Files and folders go to the Recycle Bin.

**Find Leftovers** (the File menu, or the button) looks for what programs
removed before left behind, without uninstalling anything:

- entries whose uninstaller is gone, with all of the above for each, as if
  they had just been uninstalled;
- shortcuts, startup entries, services and scheduled tasks that run something
  gone (from a drive that is there; Windows' own tasks are left alone), and the
  folder in Program Files, ProgramData or AppData they pointed into (not
  ticked at first: a hint only);
- empty folders right below Program Files, ProgramData, AppData and the Start
  menus (not those Windows keeps empty).

Services found only because what they run is gone are not ticked at first
either.

### The window

- **Typing** anywhere finds programs by name, publisher or folder (several
  words: all of them); Esc clears the search. Ctrl+V pastes into it from
  anywhere. In the box, a click puts the caret, a drag or Shift selects, a
  double click selects a word; the arrows, Home and End move (Ctrl: by words),
  Delete and Backspace delete, Ctrl+A, Ctrl+C and Ctrl+X select, copy and cut;
  its right-click menu has Cut, Copy, Paste, Delete and Select All.
- **All Programs / For Everyone / For Me Only / For Other Users** chooses whose
  programs are shown.
- **System Components** and **Updates** show what Windows hides from its own
  list (`SystemComponent`, updates and KBs), greyed.
- Columns: name with the program's icon, publisher, version, size, installed
  date, for whom, and source; a click on a title sorts by it (versions by their
  numbers: 9.11 before 10.2).
- **Details**, on the right: for whom, 64 or 32 bits, Windows Installer (with
  its product code), size, date, install folder, uninstall command (and the
  quiet and modify ones), the registry key, its shortcuts, and every value of
  the entry.
- The buttons above the list, the **Program** menu and the right-click menu:
  Uninstall, Uninstall Quietly, Modify or Repair, Open the Install Folder
  (Ctrl+E), Copy the Name (Ctrl+C), the Uninstall Command, the Registry Key,
  All the Details (Ctrl+Shift+C), Select All (Ctrl+A).
- With several programs selected, the details show how many, their size and
  each one; Copy All the Details copies them all.
- **Export to CSV** (Ctrl+S) saves the list shown. F5 reads everything again.

The status bar counts the programs shown, their size and the shortcuts.

## Keyboard

| Key | Does |
| --- | --- |
| Letters | Find a program by its name, publisher or folder |
| Esc | Clear the search |
| ↑ ↓ PgUp PgDn Home End | Move in the list (with Shift: select) |
| Ctrl+click, Shift+click | Select several programs |
| Space | Tick the programs selected (when the search box is not being typed in) |
| Ctrl+V | Paste into the search |
| ← → Home End, Delete, Ctrl+A/C/X | Edit the search, while it is in use |
| Ctrl+A | Select every program shown |
| Delete | Uninstall the programs selected (on the Leftovers tab: remove those ticked) |
| Shift+Delete | Uninstall them quietly |
| F5 | Read the programs again |
| Ctrl+E | Open the install folder |
| Ctrl+C | Copy the name |
| Ctrl+Shift+C | Copy all the details |
| Ctrl+S | Save the list as CSV |
| F1 | Keyboard shortcuts |

## Settings

Settings (theme, language, window size, whose programs are shown, system
components and updates) are kept in `%APPDATA%\runinst\settings.ini`. With a
file named `runinst.portable` beside `runinst.exe`, they are kept there instead
(portable mode).

## Build

```powershell
./dev.ps1 build --profile release-small -p runinst
./dev.ps1 test -p runinst
./dev.ps1 clippy -p runinst --all-targets
./dev.ps1 test -p runinst --lib report '--' --ignored --nocapture   # what this computer has
```

The output is `target\release-small\runinst.exe`.

## Linux

It builds and runs (5.7 MB), but lists nothing yet: the packages of Linux
(dpkg, rpm, flatpak, snap) come in phase 5.

## Next

1. Done: the programs of every user from the registry, Windows Installer, the
   Store and winget, joined; Scoop and Chocolatey tabs; shortcuts, icons,
   details, search and CSV.
2. Done: uninstalling, normal or quiet, one or several in a row, waiting for
   the processes each uninstaller starts; modify and repair.
3. Done: leftovers after uninstalling (folders, shortcuts, startup entries,
   registry keys, services, scheduled tasks, the user's settings, Everything),
   files to the Recycle Bin and keys saved to a `.reg` file; Find Leftovers for
   programs removed before and entries whose uninstaller is gone.
4. The optional features of Windows (DISM); the packages of development tools
   (npm, pip, Cargo, vcpkg).
5. Linux: dpkg, rpm, flatpak and snap packages, removed with `pkexec`.
