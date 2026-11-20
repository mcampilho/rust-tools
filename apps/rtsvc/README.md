# rtsvc

The rust-tools service for Windows. Installed once, it lets the tools show what needs administrator rights
(the NTFS table of the drives, the programs of every user, how much the system's own files take) without being
started as administrator, and it keeps that ready in the background. Changing things is still only for tools
started as administrator. Design, protocol and threat model: [docs/service.md](../../docs/service.md).

[Versão portuguesa](README.pt.md)

## Commands

| Command | What it does |
| --- | --- |
| `rtsvc install [--dir FOLDER] [--visibility user\|elevated] [--yes]` | Copies itself to `%ProgramFiles%\rust-tools` (or FOLDER), registers the service `RustToolsSvc` (automatic, delayed start, restarted after a failure) and starts it. Needs an elevated prompt. Says what the service lets users read, and asks first (`--yes` skips it). |
| `rtsvc uninstall [--purge]` | Stops and removes the service and its program; `--purge` also deletes its data (`%ProgramData%\rust-tools`). Elevated. |
| `rtsvc status` | Whether it is installed and running, and what it answers: version, your level, how long it has been up, clients, scans and the age of each snapshot. |
| `rtsvc console` | Runs in this window, as you, on a pipe of its own (`rust-tools-svc-dev`), for development. It does not touch the installed service, and the tools do not talk to it. |

`--lang en|pt|es` (or `RTSVC_LANG`) chooses the language of the messages.

## What it does

- **Reads the NTFS table** of a local drive for rsize, rdup, rsize-cli and rdup-cli, which ask it after Everything and
  before reading the table themselves. `--visibility elevated` limits this to tools started as administrator.
- **Keeps snapshots**, made when the computer is idle (the processor busy less than a third of the time, on mains
  power), when the list of installed programs changes, or when a tool asks: the programs (rinst reads them at start;
  a user sees the machine's and their own, an administrator everyone's) and the size of what the cleaner finds that
  needs administrator rights (rclean shows it in the rows it cannot clean yet). Kept in `%ProgramData%\rust-tools\cache`,
  readable only by administrators.
- **Does jobs** for a tool started as administrator, one of each kind at a time, which go on if the tool closes:
  cleaning the cleaner's targets that need rights (by id, plain files and the Recycle Bin only), and uninstalling
  programs of the machine by their Windows Installer product code. The tools do not use jobs yet.

## Safe by design

- Everything is a request with its own checks; there is no "run this" and no "delete this path".
- Who is asking is read from the token on the pipe: anyone may read, only an elevated administrator may change things.
  Closing UAC's door is not the service's job and it does not open it.
- The service only runs from a folder only SYSTEM, Administrators and TrustedInstaller can change (checked when
  installing and at every start), and the tools check that the server of the pipe is the one registered.
- On a PC where `Everyone` has full control of the root of the drive (`icacls C:\`), every folder on it is refused.

## Limitations

- `install`, `uninstall` and the real service have been tested only in pieces (the pipe, the levels, the folder check,
  the snapshots and the jobs, with tests in one process); they have not been run on an installed service yet.
- The tools do not show that a list came from the service, nor offer to ignore it (rinst reads the programs live
  after the first reading).
- Restricting the privileges of the service, and the jobs in the tools' windows, are still to do.
- Windows only.

## Build

```powershell
./dev.ps1 build --profile release-small -p rtsvc
./dev.ps1 test -p rtsvc -p rt-svc
```
