# rdup

A portable duplicate file finder for Windows and [Linux](#linux), written in
Rust. It searches drives, folders and network shares, all together, for files
with the same contents, and lists them in groups, the ones that waste the most
space first; and for folders that are copies of each other, whole or in part.

[Versão portuguesa](README.pt.md)

rdup is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`rdup.exe` is a single 2.2 MB executable: no installer, no runtime, no DLLs beyond
those that ship with Windows. It makes no network connections of its own (it
reads network shares you choose) and has no telemetry. Nothing is moved or
deleted unless you mark it and confirm, and then only to the Recycle Bin or to
a folder you choose; one copy of each group is always kept.

## Use

```text
rdup [folder...]
```

Without folders, rdup shows the drives and the folders added before, each with
a box to tick: the folders stay on the list, ticked or not, until their ×
takes them off, so favourite folders can be searched only now and then. **Add
Folder** (Ctrl+O) adds any folder, ticked, including one on the network
(`\\server\share\folder`); a folder dropped on the window is added too. **Look for** ticks duplicate files, duplicate folders or both (neither leaves
nothing to look for: the search cannot start). **Find Duplicates** (F5) searches everything ticked at once, so a file on
one drive and its copy on another are found. A folder inside another one
chosen is read only once. The choice is kept for the next time.

While it searches, the window shows the step it is in, what it has done and
the file it is reading; **Stop** (Esc) ends the search.

The result lists the groups of copies, largest waste first. Each group has a
title row (the name, how many copies, their size and the space that would be
freed by keeping only one) and a row per copy, with its folder, size and date.
**Open** (Ctrl+Enter), **Show in File Manager** (Ctrl+E) and **Copy Path**
(Ctrl+C) act on the selected copy, also from its context menu. The status bar
sums it all up: groups, copies, space that could be freed, files compared, time,
and how the drives were read. **New Search** (Ctrl+N) goes back to the drives.

### Marking and moving copies

Each copy has a box at the start of its row: a click on it, or Space, marks it
or unmarks it. **Mark Copies...** under the list (also the **Mark** menu)
marks all copies of every group but one: **Keep the Newest**, **Keep the
Oldest**, **Keep the Shortest Path**, or **Keep Those in a Folder...** (the
copies in that folder are kept and the others marked; a group with no copy
there is left alone). **Unmark All** clears the marks.

**Move to Recycle Bin** (Delete) and **Move To...** (Ctrl+M) move the marked
copies away, after asking, with their number and size; they then leave the
list, and so do the groups left with one copy. Moved to a folder, they keep
their names (with a number when the name is taken); on another drive they are
copied first, and the original is removed only when the copy is complete.
The last copy of a group can never be marked, whatever the rule.
**Export to CSV...** (Ctrl+S) saves the list shown, with the marks.

In the folders, only identical folders can be marked: a similar folder holds
files the other has not. Moving a folder away also takes its copies out of
the files, and the other way round.

### Duplicate folders

**Folders** (Ctrl+2) above the list shows the folders that are copies of each
other, and **Files** (Ctrl+1) the files again. Folders are compared by the
contents of all the files below them, whatever their names: they are
**identical** when every file of one has its copy in the other and the other
way round, and similar when the files they share make up a share of the larger
one (in bytes). The buttons beside them choose what is shown: **Identical**,
**90% or more** (the first time), **75% or more** or **50% or more**.

Identical folders are shown together, as many as there are, with the space
that keeping one would free; two similar folders, with how much they have in
common. A pair inside a pair with as much in common or more is left out:
when `C:\Photos` and `D:\Backup\Photos` are the same, `C:\Photos\2024` and
`D:\Backup\Photos\2024` are too. Identical folders inside two that are only
similar are shown, so each lower share shows all that a higher one does, and
more. **Open**, **Show in File Manager** and **Copy Path** act on the
selected folder.

The filters choose the files that count in a folder, but not the smallest
size: it only spares reading files. So the files under it are compared too,
only in the folders that may be copies (step 5 below), and a folder is only
identical when all its files are.

### Filters

The filters below the drives choose which files are compared; they are kept
for the next time, and **Clear Filters** empties all but the smallest size.

- **Size**: the smallest and the largest file, as `2000` (bytes), `500 KB`,
  `1.5 GB` or `2 TB`; the buttons beside them set the smallest with one click
  (any size, 4 KB, 64 KB, 1 MB, 10 MB, 100 MB) and empty the largest, and none
  of them is lit while a largest size is typed. The smallest is 1 MB unless you
  change it: below a few hundred kilobytes nearly every file shares its size
  with another, so each of them would have to be opened. On three drives with
  5.3 million files, 97 % share a size with another file; from 1 MB, 30 000
  files are left (0.6 %), holding 78 % of the space.
- **Modified**: from one day to another, as `2024-01-31` or `31/01/2024`, or a
  whole month or year (`2024-06`, `2024`). Both days count. The button in each
  box opens a calendar: the arrows (or PgUp and PgDn, or the wheel) change the
  month, a click on a day writes it in the box, and **Today** and **No Date**
  do what they say.
- **Types**: ticks for images, videos, audio, archives, office files,
  programming and system files; pointing at one shows its extensions. Ticking
  one writes its extensions in **Only these**, and unticking it takes them out;
  a type stays ticked while all its extensions are there, so taking one out by
  hand unticks it.
- **Extensions**: only these (`msi png mp3`), or all but these
  (`tmp log obj lnk`), with or without the dot. `doc*` stands for `doc`,
  `docx`, `docm`...
- **Names**: only files whose name, or the name of a folder they are in,
  matches (`Setup*  *Installer*  *video*`), or all but those
  (`tmp*  *cache*  log*`); a folder left out is not looked into. `*` stands for
  any text and `?` for one character; case does not matter.

A filter that is not understood is outlined in red, and the line beside
**Find Duplicates** says why; so is an extension or a name both to keep and to
leave out, until it is taken out of one of the boxes. Empty files are always left out, and so are, at
the root of a drive, the Recycle Bin, `System Volume Information` and the NTFS
system files.

The filter boxes edit text as usual: drag the pointer, double-click a word or
Shift+click to select; Shift with the arrows, Home and End to select from the
keyboard, and Ctrl with the arrows, Backspace and Delete to move or delete a
word at a time; Ctrl+A, Ctrl+C, Ctrl+X and Ctrl+V select all, copy, cut and
paste. Tab goes to the next box, Enter starts the search, and Esc leaves the
box.

## How it finds duplicates

Reading every file to compare it would take hours, so rdup compares in steps,
each leaving fewer files to read:

1. **Read the drives and folders**: names, sizes and dates only, the fastest way
   there is (see below).
2. **Size.** A file no other file has the size of cannot have a copy. This
   leaves out most files without opening any of them.
3. **First and last bytes.** The files left are opened, and their first and
   last 4 KB are hashed (files up to 64 KB whole). Files that only share a size
   part here.
4. **Contents.** The files still together are read whole and hashed with
   BLAKE3, a fast hash that does not give the same result for different
   contents. Files with the same hash are copies.
5. **Folders.** For two copies of a file, their folders, and those above them
   level by level, make pairs, up to where one holds the other. The pairs that
   cannot reach 50 % in common, even with all their smaller files, are left
   out; the smaller files of the others are compared as in steps 2 to 4, and
   then each pair file by file. A file with more than 32 copies does not make
   pairs: it says little about the folders it is in.

The hashes are kept in `hashes.bin`, next to the settings, with the size and
date of each file: a file that did not change since is not read again, so a
second search of the same drives only reads what changed. Entries not used for
120 days are dropped.

Two names of one file (hard links) are not copies: rdup sees that they are the
same file on the same volume and counts it once.

Each drive is read on its own, in the order of its folders. The first and last
bytes are read by four threads at once: they are small reads that mostly wait
for the file to open and the head to arrive, and a hard disk serves several in
a better order (on a folder of 60 000 small build files, five times faster
than one at a time). Whole files are read one at a time, so that the head does
not jump between them.

### Reading the drives

The same three ways as [rsize](../rsize/README.md), shared through the
`rt-scan` crate, tried in this order:

- **The [Everything](https://www.voidtools.com/) index** (Windows), when
  Everything is running and indexes the folder with file sizes: one question
  returns every file below it, without touching the disk. This is also what
  makes network shares and USB drives fast.
- **The NTFS table read directly** (Windows, as administrator): a whole drive
  in seconds. **File > Restart as Administrator** starts rdup again with those
  rights and the same choice.
- **Folder by folder**, through the system: any file system, network shares,
  Linux.

The status bar says which ways were used, and when administrator rights would
have been faster.

## Keyboard

| Key | Action |
| --- | --- |
| Ctrl+O | Add a folder to the search |
| F5 | Find the duplicates |
| Ctrl+N | Change the drives and folders |
| Esc | Stop the search |
| Tab | Go to the next filter box |
| Ctrl+1, Ctrl+2 | Show the files or the folders |
| Space | Mark or unmark the selected copy |
| Delete | Move the marked copies to the Recycle Bin |
| Ctrl+M | Move the marked copies to a folder |
| Ctrl+S | Save the list as CSV |
| ↑ ↓ PgUp PgDn Home End | Move in the list |
| Ctrl+Enter | Open the selected file |
| Ctrl+E | Show it in the file manager |
| Ctrl+C | Copy its path |
| F1 | Keyboard shortcuts |

## Languages and settings

The interface comes in ten languages (English, British English, German,
Spanish, French, Italian, Japanese, Portuguese of Brazil and of Portugal,
Chinese), following the system or chosen in **View > Display Language**, and
in a light or dark theme. Settings are kept in `%APPDATA%\rdup\settings.ini`
(`~/.config/rdup` on Linux), or next to the program when a file named
`rdup.portable` is there.

## Build

```powershell
./dev.ps1 build --profile release-small -p rdup   # target\release-small\rdup.exe
./dev.ps1 test -p rdup
```

## Linux

The same program, with the drawn menus of the other rust-tools apps. Drives are
the real mounted file systems; reading is folder by folder, without crossing
into other file systems, and hard links are counted once. It was checked in
WSL2, not on a real Linux desktop.

## Next

Faster first searches on many large drives, and filters for folders (dates,
number of files).

## License

MIT. See [LICENSE](../../LICENSE).
