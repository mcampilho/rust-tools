# rsize-cli

*Read this in [Portuguese](README.pt.md).*

How much disk space do the files and folders under a path take? The command-line
twin of [rsize](../rsize/README.md), with the options of `du`. It reads the disk the same
way rsize does: the NTFS file table directly (as administrator), the
[Everything](https://www.voidtools.com) index when that is running, or folder by folder.
One 0.5 MB executable for Windows and Linux; messages in English, Portuguese and
Spanish; no network, no telemetry.

## Running

```bat
rsize-cli                       :: the current folder, in KiB, folders only
rsize-cli -h -d 1 C:\Users      :: the folders of C:\Users with sizes as 1.5K, 23M, 4.0G
rsize-cli -sh C:\Windows D:\    :: only the total of each path
rsize-cli -ah --top 20 D:\      :: the 20 largest files and folders of D:
rsize-cli --json -d 2 D:\ > d.json
```

The output is the one of `du`: the size, a tab, the path as you typed it, and each
folder after what is in it. Sizes are what the files take **on the disk** (clusters, compression,
sparse files); `--apparent-size` or `-b` give their length instead. A file with several names
(hard links) counts once.

### Options

| Option | What it does |
|---|---|
| `-h`, `--human-readable` | Sizes as `1.5K`, `23M`, `4.0G` (steps of 1024, rounded up). |
| `--si` | Like `-h`, with steps of 1000 (`1.5k`). |
| `-b`, `--bytes` | Sizes in bytes, and the length of the files (`--apparent-size`). |
| `-k`, `-m` | Sizes in KiB (the default) or in MiB, rounded up. |
| `-s`, `--summarize` | Only the total of each path (`-d 0`). |
| `-a`, `--all` | The files too, not only the folders. |
| `-c`, `--total` | A grand total after all the paths. |
| `-d N`, `--max-depth=N` | Folders down to N levels below each path. |
| `--apparent-size` | The length of the files, not the space they take on the disk. |
| `--sort=size\|name` | Order inside each folder: largest first (the default) or by name. |
| `--top N` | Only the N largest entries, largest first. |
| `--json`, `--csv` | A JSON list or CSV (`path`, `kind`, `size`, `allocated`, `files`, `folders`, `modified` as seconds since 1970) instead of text. |
| `--no-index` | Read folder by folder, without the Everything index or the NTFS table. |
| `--lang=en\|pt\|es` | Language of the messages (default: `RSIZE_LANG`, then the system's). |
| `--version`, `--help` | The version, and the help. |

Short options join (`-sh`, `-ahd2`), `--` ends the options, and `/?` shows the help.

The exit code is 0 when everything was read, 1 when a path or some folders could not be read
(the others are still shown, and a line on the error output says how many), and 2 for
wrong options.

## Differences from `du`

- A path that is a **file** shows its length (the space it takes is not known without reading its folder).
- Symbolic links, junctions and mount points are counted, not followed, and other file systems are not entered.
- With the Everything index or the NTFS table the folders are not read one by one: a drive takes seconds
  instead of minutes. They are used only when they can answer for the path; otherwise it reads folder by folder.
- Extras: `--top`, `--sort`, `--json`, `--csv`, `--no-index`.
- No `-x`, `-L`, `--exclude`, `--time`, `--block-size` or `--threshold` yet.

## Building and testing

```powershell
./dev.ps1 build --profile release-small -p rsize-cli    # target\release-small\rsize-cli.exe
./dev.ps1 test -p rsize-cli -p rt-cli
```

The tests build small folders under `target\rsize-cli-tests`, inside the project.
