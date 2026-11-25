# A privileged service for rust-tools (design)

Status: built, installed and answering as SYSTEM on a real PC (`install`, `uninstall`, `status`, the volume reading and the snapshots). Not yet run for real: the jobs. The tools do not use jobs yet; jobs are not kept on disk, so they do not survive a restart of the service. [Versão portuguesa](service.pt.md)

## 1. Goal

Some things need administrator rights: reading the NTFS table of a drive, listing the programs of every
user, measuring the system's temporary files, removing what is left of a program. Today the user has to
start the whole application as administrator for that. This design adds one small Windows service, like the
one Everything has, that the tools can ask when it is running:

- **Everyone sees the system.** A user without privileges gets the system-wide view (all drives, all
  installed programs, the size of what can be cleaned) without starting anything as administrator.
- **Only an elevated client changes things.** Changing, removing or uninstalling still needs the tool to be
  started as administrator, exactly as before. The service then does the work in the background, so the
  window stays free.
- **Work done ahead.** While the computer is idle, the service collects what `rclean` and `rinst` take long
  to find, so that the tool opens with most of it ready.

Not goals: updates (each tool keeps its own, see `CLAUDE.md`), network access of any kind, Linux (there is
no equivalent planned), and anything that lets a user do what Windows would not let them do.

## 2. Principles

1. **No new power for anyone.** What a standard user may do is the same with or without the service, except
   *reading* system-wide information. Writing needs an elevated, administrator client, checked by the
   service on every request. It replaces nothing of what UAC protects.
2. **Fixed verbs, no commands.** The protocol has a list of requests, each with its own checks. There is no
   "run this" and no "delete this path": the cleaner names an item of a catalogue the service knows.
3. **The tools work without it.** Every tool tries the service first and falls back to what it does today
   (own elevation, or the limited view). The service is optional and nothing depends on it.
4. **Small and quiet.** Few dependencies, no network, low priority, only runs heavy work when the computer
   is idle.

## 3. Pieces

| Piece | What it is |
| --- | --- |
| `crates/rt-svc` | The protocol (requests, answers, frame codec) and the client every tool links: `connect()`, the verbs as functions, and the checks of §5. |
| `apps/rtsvc` | The service executable. `rtsvc install`, `uninstall`, `status`, `run` (as the service) and `console` (foreground, for development). Windows only. |

The tools that gain from it: rsize, rdup (and rsize-cli, rdup-cli), rinst and rclean. The code they run
for NTFS and for the listings stays in their crates (`rt-scan`, `rt-dups`, …); the service calls the same
functions, so there is one implementation.

The order a reader of a drive follows becomes: **Everything** (if it runs) → **the service** → own NTFS
reading (if the tool is elevated) → folder by folder. Everything stays in the client: a service runs in
session 0 and cannot reach a program in the user's session through the window messages Everything uses.
A client that is already elevated skips the service and reads the NTFS table itself.

## 4. Transport and protocol

- **Named pipe** `\\.\pipe\rust-tools-svc`, message mode, `PIPE_REJECT_REMOTE_CLIENTS`, created with
  `FILE_FLAG_FIRST_PIPE_INSTANCE` (no one can create the name before the service).
- **DACL of the pipe:** SYSTEM, Administrators and the owner (the account the service runs as) full
  control; Authenticated Users only read and write data, read attributes, read the owner and synchronize
  (`0x120083`). Not `FILE_APPEND_DATA`: it is the right to create an instance of the pipe, which would let a
  user answer in the service's place. Network logons are denied; anonymous is not an authenticated user.
- **Frames:** `u32` length (little endian), `u8` protocol version, `u16` request, then the payload. A frame
  is at most 1 MiB, except the batches of entries of a scan (at most 8 MiB). A client may have 8 requests
  open; the service serves at most 32 clients at once.
- **Payloads:** one small hand-written binary codec (little-endian numbers, strings with a length), strict
  on decoding: every read is checked, counts are checked against the bytes left, and bytes left over are an
  error. An unknown request is an error answer, never a crash.
- **Handshake:** the client sends `Hello {protocol, app, app version}`; the service answers with its
  `protocol`, version and capabilities. It speaks protocol N and N-1. A client newer than the service uses
  what is in the capabilities and falls back for the rest.
- **Timeouts:** the client connects with ~200 ms and gives up (falls back); every request has a deadline.
  The service drops a client that is silent for 60 s.
- **Cancellation:** a request can be cancelled by its number; closing the pipe cancels all of the client's.

## 5. Who is calling, and who is answering

**The service checks the client** on every connection:

1. `GetNamedPipeClientProcessId`, then `ImpersonateNamedPipeClient` and read the token (`OpenThreadToken`):
   the user's SID, the session, and `TokenElevation`. If the token has a linked token (UAC), the service
   checks whether the *linked* one is an administrator, but only treats the client as elevated when the
   token it received is itself elevated and in Administrators.
2. Three levels, from the token and nothing else:
   - **User**: any authenticated user. May read system-level information.
   - **Elevated**: a member of Administrators with an elevated token. May also change things (§7), and
     see other users' data (§6).
   - **Denied**: anonymous, guest, or a token that cannot be read.
3. The path or name of the client's program is **not** used for security (it can be forged). It is only
   written to the log.

**The client checks the service** too, before sending anything: `GetNamedPipeServerProcessId`, open the
process, and require that its token belongs to SYSTEM and that its image is the `rtsvc.exe` the Service Control Manager has registered (§9). A fake server could otherwise collect what a tool sends.

## 6. What is visible to whom

| Data | User | Elevated |
| --- | --- | --- |
| Drives and their NTFS table (names, sizes, dates) | yes | yes |
| Programs installed for everyone, Windows features, services | yes | yes |
| Programs, folders and caches **of the user** | yes | yes |
| Programs, folders and caches **of other users** (profiles, browsers' caches) | no | yes |
| System temporary files, logs, update caches (sizes) | yes | yes |
| Contents of files | no (the service never reads or returns contents) | no |

The NTFS table shows names and sizes of folders a user may not open. Everything makes the same trade-off,
and rsize/rdup already work this way with it. The service is therefore **opt-in**: `rtsvc install` says
so and asks for confirmation, and a setting (`rtsvc config visibility = user | elevated`) can limit the
table to elevated clients. **Open question:** the default of the other users' data row (proposed above).

rdup never asks the service for contents: hashing is done by the tool, with the user's own rights, on
files it can open.

## 7. Requests

**Read (User)**

| Request | Answer |
| --- | --- |
| `Ping`, `Status` | version, uptime, what is cached and how old it is, what is running |
| `ScanVolume {root}` | the entries of a drive, streamed in batches (the `rt_scan::model::Entry`), same result as the NTFS scan the tools do |
| `Programs {since}` | the installed programs, every source (registry, MSI, Store…), with the age of the snapshot |
| `Features` | Windows features (DISM) and other lists the tool reads at start |
| `CleanScan {since}` | the items of the cleaner's catalogue with their sizes and ages |
| `Jobs` | the list and state of jobs (§8), the caller's own, or all for Elevated |

**Write (Elevated only)**

| Request | What it does |
| --- | --- |
| `Clean {items}` | removes catalogue items by id (never by path, §8) |
| `Uninstall {programs}` | runs the uninstall of programs from `HKLM` (by MSI product code, or the registered command, quiet) |
| `RemoveLeftovers {ids}` | removes leftovers the service itself found for a program |
| `CancelJob {id}` | stops a running job |

Every write request becomes a **job** (§8). Requests that need the user's own context (programs installed
for one user only) are not done by the service: the tool does them as it does today.

## 8. Background work and jobs

**Scheduler.** The service keeps snapshots (programs, features, clean scan) and refreshes them:

- **When idle:** no input for a few minutes, CPU below a threshold, not on battery, no full-screen app.
  Work runs with `PROCESS_MODE_BACKGROUND_BEGIN` (low CPU and low I/O priority) and stops at once when
  the computer is used again.
- **By events** where Windows gives them: a change notification on the uninstall keys
  (`RegNotifyChangeKeyValue`) marks the programs snapshot dirty, so only what changed is read again;
  the USN journal of a volume does the same for the cleaner's folders.
- **At most:** the cleaner's catalogue every few hours and after a change; the programs after a change or
  daily.
- **Never** while a client holds a request that needs the same data: the client's request wins.

**Cache.** `%ProgramData%\rust-tools\` with an ACL of SYSTEM and Administrators write, Users read. Files
are written to a temporary name and renamed, versioned, and limited in size. Every snapshot has the time
it was made; a tool shows its age and can ask for a refresh. Entries about other users are kept in a part
that Users cannot read (§6).

**Jobs.** A write request returns a number at once. A job:

- survives the tool that asked (closing the window does not stop it), is persisted, and has a result
  log (what was removed, what failed and why) the tool reads later;
- can be cancelled, reports progress, and is run one at a time per kind;
- is limited by the **catalogue**: the cleaner's items are ids with a fixed meaning (folder or rule),
  resolved by the service. An id is never a path from the client.

**Safe deleting** (plain files and folders of the cleaning jobs). Files are opened by handle with `FILE_FLAG_OPEN_REPARSE_POINT`, the final path is
checked against the catalogue (`GetFinalPathNameByHandle`) after opening, and deleted through that handle
(`FILE_DISPOSITION_INFO_EX`). Junctions and symbolic links are removed as links, never followed. This closes
the time-of-check/time-of-use gap where a user swaps a folder for a junction.

**Uninstalling.** A service is in session 0 and cannot show a window. Quiet uninstalls from `HKLM` and
`msiexec /x` by product code run in the service. An uninstaller that shows windows is started in the
client's session with a primary token made from the client's *elevated* token (`DuplicateTokenEx`,
session id of the client), so it behaves as if the user had started it elevated. Anything installed per
user (`HKCU`) is run as that user by the tool, not by the service.

## 9. Installing

- `rtsvc install` (from an elevated prompt; Scoop cannot elevate): copies `rtsvc.exe` to
  `%ProgramFiles%\rust-tools\` by default, registers the service `RustToolsSvc` as automatic (delayed
  start) and starts it. `rtsvc install --dir <folder>` uses another folder (for example `C:\Programs`,
  as people do with Everything), **if the folder is secure** (below). The service **never runs from a
  folder a user can write to**: otherwise replacing the exe would be a privilege escalation.
- **A folder is secure when**, checked by `install` and again by the service at every start (it refuses to
  start otherwise, and says why in the log):
  - it is on a local fixed volume (not a network share, not removable, not a subst or mapped drive);
  - neither the folder nor any folder above it up to the root is a junction or symbolic link (the path is
    the same once resolved);
  - in the ACL of the folder itself, and of every file and folder in it (four levels), only SYSTEM,
    Administrators and TrustedInstaller have any right that changes things (write, append, attributes,
    delete, change permissions, take ownership, generic write or all); `Users`, `Authenticated Users`,
    `Everyone` and any other account may only read and execute. In every folder **above** it, no one else
    may delete it or them, or change their permissions or owner (adding things next to it is harmless).
    Entries only for what is created inside (inherit only) do not count; deny entries are not read, which
    can only make the check stricter; a missing ACL (everyone full control) fails;
  - its owner is SYSTEM, Administrators or TrustedInstaller;
  - the files in it (`rtsvc.exe` and what it loads) satisfy the same ACL rule.
  A folder with the permissions of a stock `C:\Program Files` passes. The root of the drive counts too: a PC
  whose `C:\` gives `Everyone` full control of the root itself (as `icacls C:\` shows on the author's PC) lets
  anyone rename what is below it, so every folder on that drive, `Program Files` included, is refused until
  that entry is removed. `install` names the entry that fails
  and has no option to override the check.
- **The data folder** (`%ProgramData%\rust-tools`) is checked too: users may create folders in `%ProgramData%`, so
  `install` and every start refuse one that holds a link or a junction, or anything not owned by SYSTEM,
  Administrators or TrustedInstaller, and then give it to Administrators with the service's own permissions.
- The client finds where the service is by asking the Service Control Manager for its registered image
  path (`QueryServiceConfig`), not by assuming Program Files, and compares it with the image of the
  process on the pipe (§5).
- The service runs as LocalSystem, with recovery actions to restart it after a failure. Restricting its
  privileges (`SERVICE_CONFIG_REQUIRED_PRIVILEGES_INFO`) is left for the hardening step, to be done with
  measurements on a real installation: removing one it needs would stop the volume reading.
- `install` prints what the service lets users read (and to whom) and asks for confirmation (`--yes` skips it).
- `rtsvc uninstall` stops and removes the service and the files, and offers to delete the cache.
- To update the service, install again from an elevated prompt (stop, replace, start). Tools tell the
  user when the service is older than what they need.
- In the Scoop bucket the service is a package of its own (`rtsvc`) with a message pointing to
  `rtsvc install`; nothing is installed as a service by Scoop itself.

## 10. Threats and what answers them

The attackers considered: a standard local user who wants more rights, a process of an administrator's
session that is *not* elevated, and malformed data.

| Threat | Answer |
| --- | --- |
| A user replaces the service exe | installed in Program Files or in another folder that passes the check of §9 (checked at install and at every start, so a later change of permissions stops the service); the client checks the server's image path against the one the Service Control Manager registered (§5) |
| A process creates the pipe name first | the service creates it with `FIRST_PIPE_INSTANCE` at boot, before any user process, and never lets the name disappear afterwards (a new instance before a broken one is closed); the client checks the server's token (§5) |
| A user creates the data folder before the service is installed | not used: no link and no other owner is accepted in it (§9) |
| A non-elevated process of an admin asks for a write | the token received through the pipe is not elevated, so the request is denied (§5); UAC is not bypassed |
| Path traversal, junction or symbolic link swaps | requests carry catalogue ids, handle-based opening, final path check (§8) |
| Reading what a user may not (contents) | no request returns file contents; names and sizes only, and by the visibility setting (§6) |
| Information about other users | held in a part of the cache Users cannot read; answered only to Elevated (§6) |
| Denial of service (many clients, huge requests, endless scans) | limits of §4, low priority, a job per kind at a time, deadlines |
| Malformed or hostile frames | length limits (a request is never long), strict decoding, no `unsafe` in the codec, fuzz tests of the decoder |
| Stale or forged cache | cache files are only writable by SYSTEM and Administrators; each carries a version and a time |
| Secrets in logs | logs name the user, the verb and the counts, never contents; rotated, in `%ProgramData%\rust-tools\logs` |

`unsafe` stays inside small Win32 modules with a `SAFETY` note, like the rest of the workspace.

## 11. What the tools do with it

| Tool | With the service | Without it |
| --- | --- | --- |
| rsize, rsize-cli | Everything → `ScanVolume` → own NTFS (if elevated) → folder by folder | as today |
| rdup, rdup-cli | the same for the listing; hashing stays in the tool | as today |
| rinst | `Programs` and `Features` at once; uninstall and leftovers as jobs when elevated | reads everything at start, as today |
| rclean | `CleanScan` at once; `Clean` as a job when elevated | scans at start, as today |

Each of these tools shows when it used the service ("via service, 3 minutes ago") and has a way to refresh
or to ignore it.

## 12. Plan

1. **This document**, reviewed.
2. **Skeleton:** `rt-svc` (frames, codec, client with the checks of §5), `rtsvc` with `install`,
   `uninstall`, `status`, `console`, `Ping` and `Status`, tests of the codec and of the access levels.
3. **`ScanVolume`** for rsize/rdup (and the `-cli` ones), from the code in `rt-scan`.
4. **Snapshots:** programs and features for rinst, the catalogue and sizes for rclean, the scheduler and
   the cache.
5. **Jobs for elevated clients:** cleaning (catalogue, safe deleting), uninstalling and leftovers.
6. **Hardening:** fuzzing the decoder, the review of §10, documentation in each tool's README.

Each step ends with tests, clippy, and the tools working with and without the service.

## 13. Open questions

- The default for other users' data (§6): visible only to Elevated (proposed) or to everyone.
- Whether the check of §9 should also accept a folder whose only extra writers are named by the user
  (proposed: no, to keep the rule simple).
- The name of the service and the executable (`RustToolsSvc` and `rtsvc` are proposed).
- How long the cache is kept, and the idle thresholds, to settle with measurements.
- Whether a `-cli` tool should offer `--no-service` (proposed: yes, with `--service` to require it).
