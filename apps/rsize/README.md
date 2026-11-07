# rsize

A portable disk space analyzer for Windows and [Linux](#linux), written in
Rust. It reads a drive, a folder or a network share and shows what takes up the
space: the folders and files by size, largest first, and a chart: a
**treemap** where each file is a rectangle as large as the file, a **pie** of
the folder's contents, or **rings** with the folders inside folders.

[Versão portuguesa](README.pt.md)

rsize is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`rsize.exe` is a single 1.8 MB executable: no installer, no runtime, no DLLs
beyond those that ship with Windows. It makes no network connections of its own
(it reads network shares you choose) and has no telemetry. It only reads: it
never changes, moves or deletes anything.

This is version 0.1, the first stages of the program. Still to come:
using the index of [Everything](https://www.voidtools.com/) when it is running,
statistics by file type, exporting to CSV and deleting to the Recycle Bin.

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

The tabs above the chart (or **View**, or F6, F7 and F8) choose between three
charts:

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

The chart shows one folder at a time. In the treemap, a double click goes one
folder down toward the file clicked; in the pie and the rings, a double click
on a folder shows that folder, and a click in the middle of the rings goes one
folder up. **Enter** shows the folder selected in the list, **Backspace** goes
one folder up and **Ctrl+Backspace** back to the top.

**View > File Size / Size on Disk** chooses which size is shown and sorted by:
the length of the files, or the space they take on the disk (clusters,
compression, sparse files, and on Linux the folders themselves).

The context menu of the list and the treemap (right click) opens the item,
shows it in the file manager, copies its path, or zooms the treemap.

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
| F6 / F7 / F8 | Treemap / pie / rings |
| Ctrl+Enter | Open the selected file or folder |
| Ctrl+E | Show it in the file manager |
| Ctrl+C | Copy its path |
| Ctrl+1 / Ctrl+2 | File sizes / sizes on disk |
| F1 | Keyboard shortcuts |

## How it reads

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

Times on this PC, folder by folder: C: (NTFS, 608,000 files and folders) in
16 s, D: (NTFS, 1.9 million) in 23 s. The second reading of a folder is much
faster, as the system keeps what it read.

The NTFS reader is tested on a real NTFS image made by `mkntfs`
(`tests/fixtures`), but it has not yet been timed on a whole drive.

## Linux

The Linux package is a `tar.gz` with the `rsize` binary, 5.5 MB. It runs on X11
and Wayland, and needs glibc 2.35 or newer.

- **Libraries.** As the other tools: `libwayland-client` and `libxkbcommon` on
  Wayland, `libX11`, `libxcursor`, `libxkbcommon` and `libxkbcommon-x11` on X11.
- **Drives.** The start page lists the mounted file systems that hold files
  (ext4, btrfs, xfs, FAT, exFAT, NTFS, NFS, SMB and others), not `/proc`,
  `tmpfs` and the like. A device mounted more than once is listed once.
- **Folder dialog.** It uses `zenity` or `kdialog`. Without them, a folder can
  still be given on the command line.
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

`./dev.ps1 test -p rsize` runs 41 tests (40 on Linux), on:

- the tree: totals, sorting by either size, paths, extensions;
- reading: a folder tree with deep and accented names, a missing root, a
  stopped scan, junctions on Windows, symbolic and hard links and unreadable
  folders on Linux;
- the NTFS reader, on an 8 MB volume made by `mkntfs`: the boot sector, run
  lists, update sequences, resident and non-resident files, a named stream, a
  name with accents, a folder below the root, `$BadClus`, stopping;
- the treemap: the squarified layout fills the area in proportion, every pixel
  belongs to a file, clicks find the right file, colours follow the extensions;
- the pie and the rings: slices cover the circle in proportion, small items
  go into "Other", rings nest inside their folders, clicks find the right
  segment;
- the list: opening folders, selection, the keyboard, zoom;
- sizes, counts, percentages and dates in several languages, settings;
- translations: every string in the 10 languages, no two items of a menu with
  the same access key, and all the pages and charts drawn in German with no
  untranslated text.

Two more tests run on request: `screenshots` draws every page to
`apps/rsize/target/screens/*.png`, and `bench` times the reading of the folder
in `RSIZE_BENCH_DIR`:

```powershell
./dev.ps1 test -p rsize --release --lib screenshots '--' --ignored
$env:RSIZE_BENCH_DIR = 'C:\'; ./dev.ps1 test -p rsize --release --lib bench '--' --ignored --nocapture
```

## Limitations

- Read folder by folder on Windows, a file with several names (hard links)
  is counted once per name, so a system drive can show more than it holds,
  and a folder's own space on the disk is not counted. Reading NTFS directly
  has neither problem.
- Reading NTFS directly reads the whole volume's table even for one folder of
  it; for a small folder on a large drive, folder by folder may be quicker.
- **Rescan** reads the whole folder again; one folder of the result cannot be
  read again on its own yet.
- With hundreds of thousands of files, drawing the treemap takes a fraction of
  a second; while the window is being resized it is stretched, and drawn again
  when the resizing stops.

## License

MIT. See [LICENSE](../../LICENSE).
