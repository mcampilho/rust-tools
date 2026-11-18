# rdup-cli

*Read this in [Portuguese](README.pt.md).*

Which files have the same contents? The command-line twin of [rdup](../rdup/README.md), with
the options of `fdupes`. It searches all the folders and drives you give it together,
subfolders included, and lists the copies in groups, the ones that waste the most space first.
It reads the disk the way rdup does (the NTFS file table as administrator, the
[Everything](https://www.voidtools.com) index when that is running, or folder by folder) and compares
files by size, then by their first and last bytes, then by BLAKE3 hashes of the whole contents.
One 0.8 MB executable for Windows and Linux; messages in English, Portuguese and Spanish; no
network, no telemetry. **It only lists: it never deletes, moves or links anything.**

## Running

```bat
rdup-cli D:\Photos E:\Backup           :: the copies, a file per line, a blank line between groups
rdup-cli -S --min-size 1MB D:\         :: with the size of each group, files of 1 MB or more
rdup-cli -m C:\Users                   :: only how many copies there are and the space they take
rdup-cli -f D:\Photos > extras.txt     :: leaving out the first of each group: the files you could remove
rdup-cli --ext jpg,png --json D:\ > copies.json
```

Paths are shown in full. Empty files are never copies, and neither are the names of one file
(hard links).

### Options

| Option | What it does |
|---|---|
| `-r`, `-R`, `-n` | Accepted, as in fdupes, and they change nothing: subfolders are always searched and empty files are never compared. |
| `-S`, `--size` | The size of the files of each group before them. |
| `-h`, `--human-readable` | Sizes as `1.5K`, `23M`, `4.0G`. |
| `-m`, `--summarize` | Only how many copies there are, in how many groups, and the space they take. |
| `-1`, `--sameline` | A group per line, the files separated by spaces (in quotes when a path holds one). |
| `-f`, `--omitfirst` | Leaves out the first file of each group. |
| `-q`, `--quiet` | No progress. It is shown on the error output only when that is a terminal. |
| `-A`, `--nohidden` | Leaves out files and folders whose name starts with a dot. |
| `--min-size SIZE`, `--max-size SIZE` | Only files of at least or at most SIZE (`100KB`, `1 MB`, `1.5GB`). |
| `--after DATE`, `--before DATE` | Only files changed from DATE, or up to the end of DATE (`2024-01-31`, `31/01/2024`, `2024-01`, `2024`). |
| `--ext LIST`, `--skip-ext LIST` | Only files with these extensions (`jpg,png`), or leaving them out. |
| `--include NAMES`, `--exclude NAMES` | Only files or folders whose name matches, or leaving them out (`*` and `?`, no case; a folder left out is not looked into). |
| `--json`, `--csv` | The groups as a JSON list (`size`, `wasted`, `hash`, `files` with `path` and `modified` in seconds since 1970) or CSV (a row per file) instead of text. |
| `--no-index` | Read folder by folder, without the Everything index or the NTFS table. |
| `--no-cache`, `--cache FILE` | Do not use the cache of hashes, or keep it in FILE. |
| `--lang=en\|pt\|es` | Language of the messages (default: `RDUP_LANG`, then the system's). |
| `--version`, `--help` | The version, and the help. |

Short options join (`-rSm`), `--` ends the options, and `/?` shows the help.

The exit code is 0 when the search was done (with duplicates or without), 1 when a folder or some
files could not be read (the others are still searched, and a line on the error output says how many),
and 2 for wrong options.

## The cache of hashes

The first search of a big disk reads every file that may have a copy, and antivirus programs make that slow.
The hashes are kept (by path, size and date) in `hashes.bin`, in the same place as rdup's, so that
the next search, here or in rdup, only reads what changed. `--no-cache` ignores it.

## Differences from `fdupes`

- Always recursive, and all the paths together: a copy in one is found in the others.
- Hard links of one file are not copies (fdupes has `-H` to say so; there is no such option yet).
- Always the same set of steps, so there is no `-p`/`-P` (permissions), `-o` (order) or `-t` (time).
- Extras: `--min-size`, `--max-size`, `--after`, `--before`, `--ext`, `--skip-ext`, `--include`,
  `--exclude`, `--json`, `--csv`, `--no-index`, the cache of hashes.
- Nothing is deleted or linked: no `-d`, `-N`, `-L` or `-B` yet. To remove copies, use rdup or the list from `-f`.
- Not yet: duplicate folders (rdup finds them).

## Building and testing

```powershell
./dev.ps1 build --profile release-small -p rdup-cli    # target\release-small\rdup-cli.exe
./dev.ps1 test -p rdup-cli -p rt-dups
```

The tests make small folders under `target`, inside the project.
