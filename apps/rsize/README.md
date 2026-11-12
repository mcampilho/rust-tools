# rsize

A portable disk space analyzer for Windows and [Linux](#linux), written in
Rust. It reads a drive, a folder or a network share and shows what takes up the
space, laid out as in WizTree: the drive's space on top, the folders and files
at the left, the space by file type at the right, and a chart below: a
**treemap** where each file is a rectangle as large as the file and each folder
a box with its name, a **pie** of the folder's contents, or **rings** with the
folders inside folders. Every column sorts. It also exports to CSV, and moves
what you no longer want to the Recycle Bin.

[Versão portuguesa](README.pt.md)

rsize is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`rsize.exe` is a single 1.9 MB executable: no installer, no runtime, no DLLs
beyond those that ship with Windows. It makes no network connections of its own
(it reads network shares you choose) and has no telemetry. It only reads, with
one exception you ask for: **Delete**, which moves an item to the Recycle Bin
after asking.

## Use

```text
rsize [--tray] [--new-window] [folder]
rsize --register | --unregister
```

Without a folder, rsize shows the drives, with their file system and how full
they are; click one to read it. **Choose Folder** (Ctrl+O) reads any folder,
including one on the network: in the dialog, or on the command line as
`\\server\share\folder`. A folder dropped on the window is read too.

`rsize --help` prints the options in the console it was typed in; started
without one (from Run, for example), rsize opens and shows them in its window.

**Resident mode:** `rsize --tray` starts hidden in the notification area.
Launching rsize again (with or without a folder) or clicking the tray icon
shows the window instantly and reads the folder there; closing the window
only hides it, and a reading goes on. Quit from the tray icon's menu or
File › Exit. `--new-window` opens a separate window even when rsize is
resident. To start it with Windows, put a shortcut to `rsize.exe --tray` in
`shell:startup`.

**Explorer integration** (Windows): File › Windows Integration › Explorer
Context Menu, or `--register`, adds "Analyze with rsize" to the right-click
menu of folders and drives, and of the empty space inside a folder; it reads
that folder (in the resident rsize, if there is one). It is written for the
current user only, pointing to the `rsize.exe` it was turned on from;
unchecking it, or `--unregister`, removes it.

While it reads, the window shows how many files and folders it has found and
their size. **Stop** (Esc) ends the reading and shows what was read so far; the
status bar then says the result is incomplete.

The result has four parts, in one of two layouts (**View**, F3 and F4, or the
**Folders** and **Map** buttons at the right of the block on top):

- **Folders** (as WizTree): the list at the left, the file types at the right,
  the treemap below.
- **Map**: the chart at the left, the file types at the right, the list below.
  The chart gets the larger area, and only here can it be the pie or the rings.

The bars between the parts can be dragged; each layout remembers its own.

- **On top**, what was read and the space of its volume: the folder (with the
  volume's name), how long the reading took and how (from the Everything index,
  from the NTFS table, folder by folder), how many files and folders it holds;
  then the volume's total space, used space (with its share), reserved space
  and free space, with a line that turns red past 90 %. **Reserved** is free
  space this user cannot fill: disk quotas on Windows, the blocks ext4 keeps
  for root on Linux.
- **The list**: each folder with its share of the folder above it
  (a bar and a percentage), its size, its space on the disk (**Allocated**), how
  many **items** (files and folders), **files** and **folders** it holds, and its
  newest change. Folders open with the arrow, a double click, the Right arrow
  or Space. Folders that could not be read, links and hard links are marked
  after the name. When the window is narrow, the least needed columns hide
  first (Folders, Items, Allocated).
- **The file types**, at the right: the files of the folder the chart shows,
  by extension: the colour they have in the treemap, what Windows calls them
  ("JSON File", "Application", in the system's language; "EXT File" on Linux),
  their share of the folder, their size, their space on the disk and how many
  files there are. A click on a type makes its files stand out in the treemap
  (everything else darkens), in the folder shown and the ones zoomed into
  later; another click on it ends that.
- **The chart**. In the treemap, one rectangle per file, inside the
  rectangles of its folders, shaded as cushions. Files are coloured by
  extension: the twelve extensions that take the most space get a colour each,
  the others are grey. Folders with room for it get a **header** with their
  name and size (as in SpaceSniffer and TreeSize), so the nesting reads as
  "Portable — 240 GB › scoop › apps". Pointing at a rectangle shows its path
  and size in the status bar; clicking a file, or a folder's header, selects it
  in the list, which opens the folders above it. Selecting in the list outlines
  the item in the treemap.

**Sorting.** A click on a column's header sorts by it; another click turns the
order round. The arrow shows the column and the direction. Sizes and counts
start largest first, names from A to Z. In the list, each folder's contents
are sorted within it. "Share of folder" and "Percent" follow the size chosen
in **View** (file size or size on disk).

In the map layout, the tabs above the chart (or **View**, or F6 to F8) choose
between three charts (F7 and F8 switch to the map layout):

- **Treemap**, as in WinDirStat or WizTree.
- **Pie**, as in TreeSize or JDiskReport: one slice for each item of the
  folder, the twelve largest, with the rest in one "Other" slice, and a legend
  with their sizes and shares.
- **Rings**, as in Baobab or Scanner: the folder in the middle, its contents in
  a ring around it, theirs in the next ring, up to six levels, each item inside
  the angle of its folder. The colour follows the angle and gets lighter
  further out; files are paler than folders. Items too thin to see are left
  out.

In the pie and the rings, the slice under the pointer moves out from the
centre and brightens, with everything inside it, and its path and size show in
the status bar. A click selects it in the list.

The chart shows one folder at a time, and the file types follow it. In the
treemap, a double click on a folder's header shows that folder, and on a file
goes one folder down toward it; in the pie and the rings, a double click
on a folder shows that folder, and a click in the middle of the rings goes one
folder up. **Enter** shows the folder selected in the list, **Backspace** or a
**right click** on the chart goes one folder up and **Ctrl+Backspace** back to
the top.

**View > File Size / Size on Disk** chooses which size is shown and sorted by:
the length of the files, or the space they take on the disk (clusters,
compression, sparse files, and on Linux the folders themselves).

The context menu (a right click on the list, or Shift+right click on the
chart) opens the item, shows it in the file manager, copies its path, zooms
the chart, or deletes it.

**Edit > Delete** (Delete) moves the selected file or folder to the Recycle
Bin, or to the desktop's trash on Linux, after showing what it is and how much
it holds. Where there is no Recycle Bin (a network share) or the item is too
large for it, Windows asks before deleting it for good. The list, the totals
and the charts then lose the item at once, without reading the disk again.

**File > Export to CSV** (Ctrl+S) writes the folder shown in the chart, with
everything below it, to a CSV file: one row per folder and file, with its
path, kind, size, size on disk, files and folders inside, and last change. The
file opens in spreadsheets as it is (UTF-8, commas).

### Keyboard

| Keys | Action |
| --- | --- |
| Ctrl+O | Choose a folder to analyze |
| Ctrl+D | Show the drives |
| F5 | Read the same folder again |
| Esc | Stop reading |
| ↑ ↓ PgUp PgDn Home End | Move in the list |
| ← → Space | Close or open a folder |
| Enter | Show the selected folder in the treemap |
| Backspace or right click / Ctrl+Backspace | Chart: one folder up / the whole scan |
| Shift+right click | The item's menu, on the chart |
| F3 / F4 | Folders layout / map layout |
| F6 / F7 / F8 | Treemap / pie / rings |
| Ctrl+Enter | Open the selected file or folder |
| Ctrl+E | Show it in the file manager |
| Ctrl+C | Copy its path |
| Ctrl+1 / Ctrl+2 | File sizes / sizes on disk |
| Delete | Move it to the Recycle Bin, after asking |
| Ctrl+S | Export the folder shown to CSV |
| F1 | Keyboard shortcuts |

## How it reads

rsize tries three ways, the fastest first, and the block on top says which one
gave the result.

**From the index of [Everything](https://www.voidtools.com/)** (Windows), when
Everything is running: one question to Everything returns every file and
folder below the folder, with sizes and dates, without touching the disk. The
answer is used only when it looks complete: Everything must have file sizes
in its index (Tools > Options > Indexes, "Index file size") and must list what
the folder holds at its top level. Otherwise rsize reads the folder one of the
other ways. rsize talks to Everything's window directly, as Everything's own
SDK does, so nothing else is needed.

**Directly from NTFS** (Windows, with administrator rights). When the folder
is on a local NTFS volume, rsize opens the volume and reads its Master File
Table: one record per file and folder, in large sequential chunks, the way
WizTree and Everything do. The tree is then put together from the records, so
a whole drive takes seconds instead of minutes. It also counts:

- each file with several names (hard links, common in `C:\Windows\WinSxS`)
  once;
- what compressed and sparse files really take, and named streams;
- the space a folder's own index takes;
- the system files at the root of the volume (`$MFT`, `$LogFile`...).

Without administrator rights Windows does not let a program open a volume, so
rsize reads folder by folder (below) and the status bar says it would be
faster as administrator. **File > Restart as Administrator** starts rsize again
with those rights (Windows asks first) and reads the same folder. The status
bar always says which way the result was read.

The table is read without the system cache, by one thread while another
parses it. Long stretches of free records (2 MB or more, as marked in the
table's own bitmap) are skipped. `rsize --timings <folder>`, run as administrator, reads it twice
(without and through the cache) and reports how long the disk and the parsing
took, to find out why a drive is slow.

**Folder by folder**, through the system: for other file systems (FAT,
exFAT, ReFS), network shares, Linux, and when there are no administrator
rights. Several threads read folders at the same time (up to eight), which matters
most on SSDs and network shares. On Windows each folder is read with
`GetFileInformationByHandleEx(FileIdBothDirectoryInfo)`, which returns its
entries with their sizes, space on the disk and dates in batches of 64 KB, with
nothing opened per file. Paths longer than 260 characters work. Shares that do
not support that query are read with `FindFirstFile`, and then the space on the
disk is taken to be the length. On Linux folders are read with `readdir` and
`lstat`.

Symbolic links, junctions and mount points are listed but not followed, so
nothing is counted twice and the scan stays on the volume. On Linux, folders on
another file system are not entered (like `du -x`), and a file with several
names (hard links) is counted once.

Times on this PC:

| | Folder by folder | NTFS directly | Everything |
| --- | --- | --- | --- |
| C: (SSD, about 650,000 files and folders) | 16 s | 3 s | 2 to 3 s |
| D: (SSD, 1.9 million) | 23 s | 5 s | |
| E:\Projects (hard disk, 1.6 million) | | | 6 s |

Folder by folder, the second reading of a folder is much faster, as the system
keeps what it read. Everything needs a moment after it starts to load its
index; until then rsize reads the other ways.

## Linux

The Linux package is a `tar.gz` with the `rsize` binary, 5.5 MB. It runs on X11
and Wayland, and needs glibc 2.35 or newer.

- **Libraries.** As the other tools: `libwayland-client` and `libxkbcommon` on
  Wayland, `libX11`, `libxcursor`, `libxkbcommon` and `libxkbcommon-x11` on X11.
- **Drives.** The start page lists the mounted file systems that hold files
  (ext4, btrfs, xfs, FAT, exFAT, NTFS, NFS, SMB and others), not `/proc`,
  `tmpfs` and the like. A device mounted more than once is listed once.
- **Dialogs.** The folder and save dialogs and the question before deleting
  use `zenity` or `kdialog`. Without them, a folder can still be given on the
  command line, but nothing can be deleted or exported.
- **Trash.** Delete uses `gio trash` when it is there; otherwise it moves the
  item into the user's trash (`~/.local/share/Trash`), which only takes items
  on the same file system as the home folder.
- **Menus.** They are drawn in the window, with the same items and shortcuts.
  Alt+letter or F10 opens them.
- **Names.** File names that are not valid UTF-8 are shown with replacement
  characters; such files are counted, but opening them from rsize may fail.

Checked under WSL2 on X11: the tests, a scan of `/usr` (58,000 files and
folders in 0.9 s) and the window starting with a folder. The folder dialog and
Wayland were not tried.

## Build

Requirements: Rust 1.88+ with the MSVC target and the MSVC Build Tools.

```powershell
cargo build --profile release-small -p rsize
# or, with portable Build Tools:
./dev.ps1 build --profile release-small -p rsize
./dev.ps1 test -p rsize -p rt-scan
./dev.ps1 clippy -p rsize --all-targets
```

The output is `target\release-small\rsize.exe`. The icon, version info and
manifest (per-monitor DPI v2, long paths) are embedded by `build.rs`.

## Tests and verification

`./dev.ps1 test -p rsize -p rt-scan` runs 56 tests on Windows (a few of them
only there), on:

- the tree: totals, sorting by either size, paths, extensions;
- the space of the volume a folder is on;
- reading: a folder tree with deep and accented names, a missing root, a
  stopped scan, junctions on Windows, symbolic and hard links and unreadable
  folders on Linux;
- the NTFS reader, on an 8 MB volume made by `mkntfs`: the boot sector, run
  lists, update sequences, resident and non-resident files, a named stream, a
  name with accents, a folder below the root, `$BadClus`, stopping;
- the answers of Everything: reading them, building the tree from the paths
  (without case), and refusing an index that misses the folder or its sizes;
  with Everything running, the same folder read both ways;
- the treemap: the squarified layout fills the area in proportion, every pixel
  belongs to a file or a folder's frame, clicks find the right file, folders
  with room get a header that selects them and holds their contents below it,
  colours follow the extensions, a picked type stands out;
- the pie and the rings: slices cover the circle in proportion, small items
  go into "Other", rings nest inside their folders, clicks find the right
  segment;
- the list: opening folders, selection, the keyboard, zoom, sorting by every
  column both ways; the two layouts keep their own bars; the table of file types: sorting by every column, the
  names of the types, following the folder the chart shows;
- deleting: the item leaves its folder, the totals and the extension totals
  shrink, and the chart moves up when it showed what was deleted; the CSV
  export (order, quotes, columns); on Linux, the trash folder;
- sizes, counts, percentages and dates in several languages, settings;
- translations: every string in the 10 languages, no two items of a menu with
  the same access key, and all the pages and charts drawn in German with no
  untranslated text.

Three more tests run on request: `screenshots` draws every page to
`apps/rsize/target/screens/*.png`, `bench` times the reading of the folder in
`RSIZE_BENCH_DIR` (the fastest way available), and `probe` shows what a
running Everything answers for it:

```powershell
./dev.ps1 test -p rsize --release --lib screenshots '--' --ignored
$env:RSIZE_BENCH_DIR = 'C:\'; ./dev.ps1 test -p rsize --release --lib bench '--' --ignored --nocapture
./dev.ps1 test -p rsize --lib probe '--' --ignored --nocapture
```

## Limitations

- Read folder by folder on Windows, a file with several names (hard links)
  is counted once per name, so a system drive can show more than it holds,
  and a folder's own space on the disk is not counted. Reading NTFS directly
  has neither problem.
- Reading NTFS directly reads the whole volume's table even for one folder of
  it; for a small folder on a large drive, folder by folder may be quicker.
- From the index of Everything, the space on the disk is estimated (sizes
  rounded up to whole clusters), a file with several names counts once per
  name, and folders Everything was told to leave out are missing. Everything
  may also be a few seconds behind the disk.
- **Rescan** reads the whole folder again; one folder of the result cannot be
  read again on its own yet.
- The folders' headers in the treemap take some of the area, so areas are no
  longer exactly in proportion across folders: the contents of a folder share
  what is left below its header. Folders too small for a header have none.
- With hundreds of thousands of files, drawing the treemap takes a fraction of
  a second; while the window is being resized it is stretched, and drawn again
  when the resizing stops.

## License

MIT. See [LICENSE](../../LICENSE).
