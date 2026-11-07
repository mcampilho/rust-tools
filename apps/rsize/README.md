# rsize

A portable disk space analyzer for Windows and [Linux](#linux), written in
Rust. It reads a drive, a folder or a network share and shows what takes up the
space: the folders and files by size, largest first, and a chart: a
**treemap** where each file is a rectangle as large as the file, a **pie** of
the folder's contents, or **rings** with the folders inside folders. It also
totals the space by file type, exports to CSV, and moves what you no longer
want to the Recycle Bin.

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
rsize [folder]
```

Without a folder, rsize shows the drives, with their file system and how full
they are; click one to read it. **Choose Folder** (Ctrl+O) reads any folder,
including one on the network: in the dialog, or on the command line as
`\\server\share\folder`. A folder dropped on the window is read too.

While it reads, the window shows how many files and folders it has found and
their size. **Stop** (Esc) ends the reading and shows what was read so far; the
status bar then says the result is incomplete.

The result has two parts, with a bar between them that can be dragged:

- **The list**: each folder with its share of the folder above it (a bar and a
  percentage), its size, how many files it holds and its newest change. Folders
  open with the arrow, a double click, the Right arrow or Space. Folders that
  could not be read, links and hard links are marked after the name.
- **The treemap**: one rectangle per file, inside the rectangles of its
  folders, shaded as cushions so the folders stand out without borders. Files
  are coloured by extension: the twelve extensions that take the most space
  get a colour each, the others are grey. Pointing at a rectangle shows its
  path and size in the status bar; clicking selects it in the list.

The tabs above the chart (or **View**, or F6 to F9) choose between three
charts and a table:

- **Treemap**, as in WinDirStat or WizTree.
- **Pie**, as in TreeSize or JDiskReport: one slice for each item of the
  folder, the twelve largest, with the rest in one "Other" slice, and a legend
  with their sizes and shares.
- **Rings**, as in Baobab or Scanner: the folder in the middle, its contents in
  a ring around it, theirs in the next ring, up to six levels, each item inside
  the angle of its folder. The colour follows the angle and gets lighter
  further out; files are paler than folders. Items too thin to see are left
  out.
- **Types**: the files of the folder shown, by extension, largest first: the
  colour they have in the treemap, their total size and share, and how many
  files there are.

In the pie and the rings, the slice under the pointer moves out from the
centre and brightens, with everything inside it, and its path and size show in
the status bar. A click selects it in the list.

The chart shows one folder at a time. In the treemap, a double click goes one
folder down toward the file clicked; in the pie and the rings, a double click
on a folder shows that folder, and a click in the middle of the rings goes one
folder up. **Enter** shows the folder selected in the list, **Backspace** goes
one folder up and **Ctrl+Backspace** back to the top.

**View > File Size / Size on Disk** chooses which size is shown and sorted by:
the length of the files, or the space they take on the disk (clusters,
compression, sparse files, and on Linux the folders themselves).

The context menu of the list and the charts (right click) opens the item,
shows it in the file manager, copies its path, zooms the chart, or deletes it.

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
| Backspace / Ctrl+Backspace | Chart: one folder up / the whole scan |
| F6 / F7 / F8 / F9 | Treemap / pie / rings / file types |
| Ctrl+Enter | Open the selected file or folder |
| Ctrl+E | Show it in the file manager |
| Ctrl+C | Copy its path |
| Ctrl+1 / Ctrl+2 | File sizes / sizes on disk |
| Delete | Move it to the Recycle Bin, after asking |
| Ctrl+S | Export the folder shown to CSV |
| F1 | Keyboard shortcuts |

## How it reads

rsize tries three ways, the fastest first, and the status bar says which one
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
./dev.ps1 test -p rsize
./dev.ps1 clippy -p rsize --all-targets
```

The output is `target\release-small\rsize.exe`. The icon, version info and
manifest (per-monitor DPI v2, long paths) are embedded by `build.rs`.

## Tests and verification

`./dev.ps1 test -p rsize` runs 47 tests (45 on Linux), on:

- the tree: totals, sorting by either size, paths, extensions;
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
  belongs to a file, clicks find the right file, colours follow the extensions;
- the pie and the rings: slices cover the circle in proportion, small items
  go into "Other", rings nest inside their folders, clicks find the right
  segment;
- the list: opening folders, selection, the keyboard, zoom;
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
- With hundreds of thousands of files, drawing the treemap takes a fraction of
  a second; while the window is being resized it is stretched, and drawn again
  when the resizing stops.

## License

MIT. See [LICENSE](../../LICENSE).
