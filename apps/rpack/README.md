# rpack

A portable archiver for Windows and [Linux](#linux), written in Rust. It browses
drives and folders like a file manager; makes 7z, zip and tar archives (alone or
compressed with gzip, xz, zstd, bzip2, lz4 or lzip); opens them, and files
compressed whole, folder by folder; tests them and extracts all or part of them;
and it calculates the hashes of files (CRC, MD5, SHA-1, SHA-2, SHA-3, BLAKE2sp,
BLAKE3...).

[Versão portuguesa](README.pt.md)

rpack is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`rpack.exe` is a single 4.8 MB executable: no installer, no runtime, no DLLs
beyond those that ship with Windows. It makes no network connections and has
no telemetry. It is being built in phases: passwords, rar, iso, cab, zpaq and
the Explorer menu come next (see [Next](#next)).

## Use

```text
rpack [archive | folder]
```

An archive on the command line opens at once; a folder is shown. `rpack --help`
prints this in the console it was typed in; started without one, rpack shows it
in its window.

rpack opens as a file browser, in the folder shown last (the first time, the
user's folder). The path above the list goes back to any folder of it, up to
**This PC** with the drives (size, free space, file system); Backspace or the
arrow at its left goes up one level, F5 reads the folder again. Folders come
first, then files, sorted by any column (a click on its title; again to turn
the order around): name, size, date and type. Archives have an icon of their
own.

Enter (or a double click) opens a folder, an archive, or any other file with its
program. An archive is recognised by its first bytes, whatever its name: a zip
renamed `.dat`, or a `.tar.gz` without an extension, still opens; so does a zip
inside a self-extracting program. Only lzma and brotli, which have no mark of
their own, need their extension (`.lzma`, `.br`). **Open Archive** (Ctrl+O)
opens one from a dialog, and an archive dropped on the window opens too.

Inside an archive the browser goes on: its folders (made from the paths when
the archive does not store them), with size, packed size, date and method; a
folder shows the size of everything below it. Up from the archive's root goes
back to the folder it is in (**Close Archive**, Ctrl+W, too). Enter on a file
opens it with its program, after extracting it to a temporary folder that is
emptied when rpack ends. An encrypted entry is marked with `*`.

Click, Ctrl+click and Shift+click select rows; Ctrl+A selects the whole folder.
The status bar shows the row pointed or selected (path, sizes, CRC), and the
totals of the folder or of the archive (format, files, how much it was
compressed).

**What can be done with the rows selected** is on the bar under the list
(**Add**, **Hashes**, **Test**, **Extract**), on the Archive menu, and on the
right-click menu, as in 7-Zip or PeaZip:

- on disk: Open; for archives, Extract (each beside itself), Extract Here,
  Extract to "*name*" (a folder named after it), Extract to Folder, Test; Add
  to Archive, Add to "*name*.7z", Add to "*name*.zip", Add Each to Its Own
  Archive; Calculate Hashes; Show in File Manager, Copy Path, Move to Recycle
  Bin (Delete);
- inside an archive: Open, Extract, Extract to Folder, Calculate Hashes, Copy
  Path.

### Adding to an archive

**Add** (Ctrl+Shift+A) opens a window for the files and folders selected:

- **Name** of the archive, typed, and **Folder** it goes to (the one shown, or
  another with Change). The name never is a file already there: `photos (2).7z`.
- **Format**: 7z, zip, tar, tar.gz, tar.xz, tar.zst, tar.bz2, tar.lz4, tar.lz,
  and, for one file, gz, xz, zst, bz2 or lz4. The name's extension follows.
- **Level**: Store, Fastest, Fast, Normal, Maximum or Ultra (for 7z and xz the
  presets 0 to 9 of 7-Zip and xz; for zstd its levels 1 to 19).
- **Method**, for zip: Deflate (every program opens it), Zstandard, XZ or BZip2.
- **Solid** (7z, on by default): the files are compressed together in blocks of
  up to 256 MB, which makes smaller archives; off, each file alone.
- **Each item into an archive of its own**, with several selected: each named
  after itself, beside it.

The choices are kept for the next time. Add to "*name*.7z" and "*name*.zip" make
the archive at once with them, and Add Each to Its Own Archive too.

A folder is put in with everything below it, named from the folder it is in
(`photos/...`), with the files' dates (and, on Linux, permissions). Links are
left out. A file that cannot be read is left out and named in the report. The
archive is written under a temporary name and takes its name only when it is
complete: stopping, or an error, leaves nothing behind. 7z and xz use all the
processor's cores.

### Extracting

**Extract** (Ctrl+E) extracts the rows selected, with everything below the
folders selected, or the whole archive when nothing is selected. Files
selected inside a folder of the archive are written without that folder's path:
extracting `img` from inside `docs` writes `img`, not `docs/img`.

They go to the folder shown beside **Extract to**: by default a folder named
after the archive (`photos.tar.gz` → `photos`) beside it, or the archive's own
folder when everything is inside one folder or the archive holds one file, so
that there is no folder inside a folder of the same name. **Change**
chooses another folder; **Extract to Folder** (Ctrl+Shift+E) chooses one and
extracts at once. The folder is made if it is not there.

**In a folder**, archives selected can be extracted or tested all at once, each
on its own: beside itself as above (Extract), into the folder it is in (Extract
Here), into a folder named after it (Extract Each to a New Folder), or into a
folder chosen (Extract to Folder); a report tells how each went.

Archive › When a File Exists chooses what happens to a file already there:
**Keep Both** (the default: the new one is written as `name (2).ext`),
**Replace It** or **Skip It**. Dates are kept, and on Linux the permissions too.
With Archive › Open the Folder After Extracting (on by default), the folder
opens in the file manager when it is done.

Nothing is written outside the folder chosen, whatever the archive says:
`..` and drive letters are dropped from paths, and names Windows cannot hold
(`con.txt`, `what?.txt`, names ending in a dot) are changed. Links (symbolic
and hard, in tar archives) are not extracted, since a link could lead later
files out of the folder; the report counts them. A file that cannot be read
(damaged, or encrypted) is reported and left out, never left half written.

### Hashes

**Hashes** (or Archive › Calculate Hashes) calculates CRC-32, CRC-64, XXH64,
MD5, SHA-1, SHA-256, SHA-384, SHA-512, SHA3-256, BLAKE2sp or BLAKE3, or all of
them at once, of the files and folders selected on disk (every file below a
folder), or of the files selected inside an archive (all of them when nothing
is selected). Each file is read once, whatever the number of hashes. The
window shows the first files; Ctrl+C copies all of them, and **Save Hashes**
writes them as `sha256sum` and its kin write them (`hash *name`, one file a
line), so that they can be checked with those tools. CRC-64 is the one of xz
and 7-Zip; CRCs and XXH64 are shown in capitals, as 7-Zip shows them.

### Testing

**Test** (Ctrl+T) reads every file to the end and checks what the format
checks: the CRC of each zip and 7z entry, the checksums of xz, gzip, bzip2,
lzip, lz4 and zstd streams, and that each file is as long as the archive says.
It then lists the errors, if there are any.

Opening, adding, extracting, testing and hashing run in the background: the
window shows how far they got, the file being read and the time so far.
**Stop** (Esc) ends them; a file being written is removed.

## Formats

| Format | Reads | Writes | Methods |
| --- | --- | --- | --- |
| 7z | yes | yes | Reads LZMA, LZMA2, BZip2, PPMd, the BCJ filters, solid blocks; writes LZMA2 (solid or not) or Store |
| zip, jar, apk, docx… | yes, zip64 too | yes | Reads Store, Deflate, Deflate64, BZip2, LZMA, XZ, Zstandard, PPMd, and the old Shrink, Reduce and Implode; writes Store, Deflate, Zstandard, XZ, BZip2 |
| tar | yes | yes | ustar, GNU (long names, sparse), pax |
| gz / tgz | yes | yes | gzip (several members as one) |
| bz2 / tbz2 | yes | yes | bzip2 |
| xz / txz | yes | yes | xz (all checks, several streams) |
| lzma / tlzma | yes | no | lzma (the old `.lzma` format) |
| lz / tlz | yes | tar.lz | lzip |
| zst / tzst | yes | yes | Zstandard, windows up to 2 GB (`--long`) |
| lz4 / tlz4 | yes | yes | LZ4 frames |
| br / tbr | yes | no | Brotli (writing it would make rpack 1 MB larger) |

A compressed file that holds a tar is opened as a tar. A compressed tar can only
be read from the start, so listing a `.tar.xz` reads all of it, and extracting
part of it reads it up to the last file wanted. A solid 7z block is the same:
extracting one file reads the files before it in the block.

Names in zip archives not marked as UTF-8 are read in the old DOS code page
(437), as the zip format says; archives made by old Windows programs in other
languages may show some accented letters wrongly. Names written by rpack are
always UTF-8.

Password-protected zip and 7z entries are listed but not extracted yet, and
archives cannot be made with a password yet (phase 2).

## Keyboard

| Key | Does |
| --- | --- |
| Enter | Open the folder, archive or file selected |
| Backspace | Back to the folder above |
| F5 | Read the folder again |
| Ctrl+O | Open an archive |
| Ctrl+W | Close the archive |
| Ctrl+A | Select everything in the folder |
| Ctrl+Shift+A | Add the selection to an archive |
| Ctrl+E | Extract the selection, or everything |
| Ctrl+Shift+E | Extract to a folder you choose |
| Ctrl+T | Test the archive |
| Delete | Move the selection to the Recycle Bin (after asking) |
| Esc | Stop |
| ↑ ↓ PgUp PgDn Home End | Move in the list (Shift selects) |
| F1 | Keyboard shortcuts |

In the Add window: typing changes the name, Enter makes the archive, Esc closes
the window.

## Settings

Settings (theme, language, window size, what to do with existing files, the
folder shown last, how archives are made) are kept in
`%APPDATA%\rpack\settings.ini` on Windows and `~/.config/rpack/settings.ini` on
Linux. With a file named `rpack.portable` beside `rpack.exe`, they are kept
there instead (portable mode).

## Build

```powershell
./dev.ps1 build --release -p rpack
./dev.ps1 test -p rpack
./dev.ps1 clippy -p rpack --all-targets
```

The output is `target\release\rpack.exe`. rpack uses the `release` profile,
which unwinds: a decoder that panics on a damaged archive ends that one task,
not the program.

## Linux

The same features, with the drawn menus and the file dialogs of zenity or
kdialog; Move to Trash instead of the Recycle Bin. Checked under WSL2, not on a
real Linux desktop.

## Next

1. Done: a file browser; reading, testing and extracting zip, 7z, tar and the
   compressed files, one archive or several; hashes.
2. Partly done: making 7z, zip and tar archives and compressed files, with the
   Add window and the right-click menu. To do: passwords (AES in zip and 7z,
   to read and to write), dictionary size and thread count, splitting into
   volumes, adding to an archive that exists.
3. Reading only: rar (RAR4 and RAR5, volumes, passwords) with the UnRAR code,
   iso images (ISO 9660, Joliet and UDF), cab.
4. zpaq (with its versions), Lizard, lzop, and Zstandard, LZ4, Brotli and Lizard
   inside 7z archives as 7-Zip ZS writes them.
5. Windows integration: Explorer menu (extract here, extract to a folder, add to
   archive, test), file types, dragging files out of the window, commands on the
   command line.
