# rscoop

*Read this in [Portuguese](README.pt.md).*

Validates a [Scoop](https://scoop.sh) installation on Windows, wherever it lives, and
reports what is broken or still points to an old Scoop root — for example after moving
Scoop from `D:\Portable\scoop` to `E:\Portable\scoop`. By default it **changes nothing**:
it only reads files and the registry. With `--fix` it repairs what it can (see
[Repairing](#repairing)).

A Rust port of the Python `scoop-check`, with the same checks, options and report, plus
two uses of SQLite to make it faster (see [Speed](#speed)). One 2.4 MB executable; texts in
English, Portuguese and Spanish.

## Running

```bat
rscoop
rscoop --lang pt --skip configs
rscoop --json > report.json
```

The exit code is 0 when nothing was found, 1 when there are errors or warnings (or, with
`--fix`, when a repair failed), and 2 for wrong options.

### Options

| Option | What it does |
|---|---|
| `--root PATH` | Scoop root to check. By default: the `SCOOP` variable (process, user, machine), then `root_path` in `~\.config\scoop\config.json`, then `~\scoop`. |
| `--old-root PATH` | A previous root to look for (repeatable). Without it, other roots are still found from paths shaped like `X:\…\scoop\apps\…` (or `shims`, `persist`, `buckets`, `cache`, `modules`). |
| `--json` | JSON report instead of text. |
| `--checks LIST` / `--skip LIST` | Run only, or skip, some checks (comma-separated ids below). |
| `--max N` | Problems shown per check in the text report (default 50, `0` = all). JSON always has all. |
| `--config-depth N` | Folder depth searched for config files under `apps\*\current` (default 3). |
| `--persist-depth N` | Folder depth searched for config files under `persist` (default 5). |
| `--lang en\|pt\|es` | Report language (default: `RSCOOP_LANG`, then the Windows display language; English for others). |
| `--fix [LIST]` | Repair what can be repaired; optionally only some checks (`current`, `persist`, `shims`, `regfiles`, `configs`, `shortcuts`, `registry`, `env`). |
| `--dry-run` | Show the repairs `--fix` would make, without changing anything. |
| `--backup-dir PATH` | Where `--fix` keeps its backups (default `%LOCALAPPDATA%\rscoop\backup\<date-time>`). |
| `--no-cache` | Read every file again and leave the cache alone. |
| `-q`, `--quiet` | No progress lines on stderr. |

## Checks

| Id | What is checked |
|---|---|
| `roots` | Where the root comes from; `SCOOP` and `SCOOP_GLOBAL` (process, user, machine) and `root_path`/`global_path` in `config.json` point to existing folders; `SCOOP` and `root_path` agree. |
| `shims` | Every `<root>\shims\*.shim`, `*.cmd`, `*.ps1` and extension-less bash shim: the target exists and is not under another root; `.exe` and `.shim` come in pairs. Alternative shims (`x.shim.app`) are warnings. |
| `current` | `apps\<app>\current` is a junction to an existing version folder. An **empty `current` folder** (what a plain copy of the tree leaves instead of the junction) is an error, with the version folder it should point to. |
| `persist` | For each `persist` entry of the installed manifest: the folder is a junction (or the file a hard link) to `<root>\persist\<app>\…`. |
| `install` | Apps without `scoop-install.json`/`scoop-manifest.json` (or the older `install.json`/`manifest.json`) — the "Install failed" of `scoop list`; manifests no longer in their bucket; buckets that are missing or not git repositories. |
| `shortcuts` | `.lnk` files in the `Scoop Apps` Start menu folders (user and all users): target, working folder and icon. |
| `regfiles` | `.reg` files in `apps\*\current` (such as `install-context.reg`), which hold absolute paths written at install time. |
| `registry` | HKCU `Software\Classes`, `Run`, `RunOnce`, `App Paths` and `Uninstall`: values pointing inside another Scoop root, or to missing files (in `Classes` and `Uninstall` only missing files under the Scoop root count). |
| `env` | User and machine variables (from the registry): every `PATH` entry and every variable that looks like a path exists and is not under another root; the shims folder is in `PATH`. |
| `configs` | Text config files (`.ini`, `.json`, `.cfg`, `.conf`, `.xml`, `.toml`, `.yaml`, `gitconfig`, …, up to 512 KB) in `apps\*\current` and `persist` with a path inside another root or a missing path under the current one. |

The global root (`SCOOP_GLOBAL`, `global_path` or `%ProgramData%\scoop`) counts as a
current root; its apps, shims and persist folder are checked too when it exists.

## Speed

On a first run almost all the time goes into opening some 30 000 small files — and the
antivirus reads each one on its first open. rscoop keeps a cache in
`%LOCALAPPDATA%\rscoop\cache.db` (SQLite): for each shim, shortcut, install/manifest JSON,
`.reg` and config file, its size, its modification time and what was read from it (for
config files only the paths found in them). A file whose size and time did not change is
not opened again; whether the paths exist is always checked anew. Rows unused for 30 days
are dropped. `--no-cache` turns it off.

When Scoop's `use_sqlite_cache` is on, the `install` check also reads Scoop's own
`scoop.db` (the manifests of every bucket) with one query; an app missing there is still
looked up in its bucket folder, since that database can lag behind.

On the installation it was written for (1 500 apps, 28 000 config files, a hard disk):

| | First run | Next runs |
|---|---|---|
| Python `scoop-check` | 264 s | 264 s |
| rscoop | 261 s | 17 s |

The config files are read on several threads. The first run is bound by the disk and the
antivirus, like the Python version.

## Repairing

```bat
rscoop --fix --dry-run
rscoop --fix
```

Run `--dry-run` first to see the list. `--fix` then repairs, in this order:

| Check | Repair |
|---|---|
| `current` | An empty `current` folder or a broken junction becomes a junction to the newest version folder. |
| `persist` | A missing or wrongly linked persisted item becomes a junction (folder) or hard link (file) to `<root>\persist\<app>\…`, when that exists. Real files or folders in the way are left alone. |
| `shims`, `regfiles`, `configs` | The old root is replaced by the current one in the text, in all its spellings (`\`, `/` and the escaped `\\` of JSON and `.reg` files); the encoding (UTF-8, UTF-16, BOM, Windows-1252) and a read-only attribute are kept. |
| `shortcuts` | The shortcut is re-created through `WScript.Shell` with the old root replaced in target, arguments, working folder and icon. (Changing only the target leaves the old path inside the file's LinkInfo, which is what `scoop reset` does.) Shortcuts WScript cannot read are left alone. |
| `registry`, `env` | HKCU values and user/machine variables: the old root is replaced, keeping the value type. Machine variables need an administrator prompt. Running programs are told that the environment changed. |

Every changed file is copied to the backup folder first (under a folder named after its
drive), and changed registry values are written to `registry.json` there with their old
and new values. Missing files that are not in another root cannot be repaired
automatically; they stay in the report. Run rscoop again afterwards to see what is left.

## Tests

```powershell
./dev.ps1 test -p rscoop
```

The tests build a fake Scoop tree (junctions, hard links, shims, shortcuts, `.reg` and
config files) under `target\rscoop-tests` and use an in-memory registry; the real
registry is never read or written.

## Languages

The texts live in `src/strings.txt`: one row per key with the English, European
Portuguese and Spanish texts, separated by tabs.

## Linux

Scoop is Windows only: on Linux rscoop only says so.
