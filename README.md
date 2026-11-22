# rust-tools

Small, portable desktop tools for Windows and Linux, written in Rust. Each one
is a single executable: no installer, no runtime, and no telemetry. Only rwall
uses the network, to download the pictures it puts on the desktop.

[Versão portuguesa](README.pt.md)

| Tool | What it is | Windows | Linux |
| --- | --- | --- | --- |
| [rcalc](apps/rcalc/README.md) | Calculator: standard, scientific, programmer, dates and unit converters | 1.9 MB | 6.6 MB |
| [rpad](apps/rpad/README.md) | Text and code editor: tabs, encodings, regex search, multiple carets, syntax highlighting | 4.2 MB | 9.2 MB |
| [rview](apps/rview/README.md) | Document viewer for PDF, Markdown, HTML, text and CSV, with selection and copy | 4.4 MB | 16.6 MB |
| [rsize](apps/rsize/README.md) | Disk space analyzer laid out as WizTree: the volume's space, folders and file types sortable by every column, a treemap with folder headers, pie or rings, CSV export; reads NTFS directly or the Everything index | 1.9 MB | 5.5 MB |
| [rdup](apps/rdup/README.md) | Duplicate file finder: files with the same contents on several drives and folders at once, by size and BLAKE3 hashes; reads the drives like rsize | 1.9 MB | 5.6 MB |
| [rwall](apps/rwall/README.md) | Wallpaper changer in the notification area: every day, twice or four times a day, weekly, monthly or every few minutes, with pictures from Bing, NASA, Wallhaven, Picsum or your own folders; on Windows also a dynamic wallpaper behind the icons (video, animated GIF or a drawn scene) and desktop widgets (clocks, calendar, notes, weather, fences, photos), and a see-through taskbar | 3.7 MB | 6.8 MB |
| [rpack](apps/rpack/README.md) | Archiver with a file browser: makes 7z, zip and tar archives; opens, tests and extracts zip, 7z, tar and files compressed with gzip, bzip2, xz, lzma, lzip, zstd, lz4 or brotli, found by their contents; hashes from CRC-32 to BLAKE3; passwords, rar, iso and more come next | 4.8 MB | 9.3 MB |
| [rinst](apps/rinst/README.md) | Uninstaller: the programs installed for everyone, for you and (as administrator) for every other user, from the registry, Windows Installer, the Store and winget in one list, Scoop and Chocolatey in tabs, with shortcuts, icons and newer versions; uninstalls one or several in a row (normal or quiet, waiting for what each uninstaller starts); then finds and removes the leftovers (folders, shortcuts, startup entries, registry keys, services, scheduled tasks; `.reg` copy and Recycle Bin), also of programs removed before; tabs for Windows features (DISM) and npm, pip, Cargo and vcpkg; on Linux, dpkg, rpm, Flatpak and Snap packages | 2.1 MB | 5.9 MB |
| [rclean](apps/rclean/README.md) | Cleaner: shows how much the temporary files of Windows or Linux, error reports, thumbnails, shader caches, the Recycle Bin and the browsers' caches (Chrome, Edge, Brave, Vivaldi, Opera, Firefox…) and the caches of package managers (Scoop, pip, npm, Cargo, vcpkg, apt… through their own commands) take, and deletes for good what you tick, after asking; broken shortcuts (to the Recycle Bin) and invalid registry entries (saved to a `.reg` file first); history, cookies and passwords are never touched | 1.9 MB | 5.6 MB |
| [rcare](apps/rcare/README.md) | Windows maintenance in one window (Windows only, in progress, see [the plan](docs/rcare.md)): a collapsible sidebar chooses the page, and the pages are the ones of the small tools. So far System Maintenance (the checks in one go), Custom Clean (junk, duplicates and large files in one look), Speed Up, the Startup Optimizer and the pages of rclean (cleaner), rdup (duplicates), rsize (disk space) and rinst (uninstaller); then speed, registry, network and protection tools | – | – |
| [rscoop-cli](apps/rscoop-cli/README.md) | Command-line checker for a [Scoop](https://scoop.sh) installation: shims, `current` links, persisted data, failed installs, Start menu shortcuts, registry, variables and config files that are broken or still point to an old Scoop root (after moving Scoop to another drive); `--fix` repairs them with a backup; a SQLite cache of the files already read makes the next runs about 15 times faster | 2.4 MB | – |
| [rsize-cli](apps/rsize-cli/README.md) | Command-line disk usage, like `du`: how much space the files and folders under a path take (on the disk or by length), as bytes, KiB, MiB or `1.5K`, with depth, totals, the N largest, JSON and CSV; reads NTFS directly or the Everything index like rsize | 0.5 MB | – |
| [rdup-cli](apps/rdup-cli/README.md) | Command-line duplicate finder, like `fdupes`: files with the same contents in the folders and drives given together, listed in groups (the biggest waste first) or as a summary, JSON or CSV, with filters by size, date, extension and name and the cache of hashes of rdup; it never deletes anything | 0.8 MB | – |

They all draw their own interface on the CPU (winit, softbuffer, tiny-skia and
ab_glyph), with no GPU and no UI toolkit. They come in 10 languages and follow
the system's light or dark theme. They can also stay resident in the
notification area with `--tray`, and each can start that way with Windows (or
when you log in, on Linux) from its own menu or settings. rscoop-cli, rsize-cli and rdup-cli are the
exceptions: command-line tools (the `-cli` suffix) in English, Portuguese and Spanish.

## Build

Requirements: Windows, Rust 1.88+ with the `x86_64-pc-windows-msvc` target, and
the MSVC Build Tools (linker and `rc.exe`).

```powershell
./dev.ps1 dist              # release builds of all the tools, in dist\
./dev.ps1 test --workspace
./dev.ps1 clippy --workspace --all-targets
```

`dev.ps1` runs cargo after loading a portable Build Tools environment, if there
is one: from `$env:RUST_TOOLS_MSVC_ENV`, or from `\Portable\Tools\dev\msvc` on
`E:` or `D:`. With the Build Tools already on `PATH`, plain cargo works too:

| Tool | Command | Output |
| --- | --- | --- |
| rcalc | `cargo build --profile release-small -p rcalc` | `target\release-small\rcalc.exe` |
| rpad | `cargo build --profile release-small -p rpad` | `target\release-small\rpad.exe` |
| rview | `cargo build --release -p rview` | `target\release\rview.exe` |
| rsize | `cargo build --profile release-small -p rsize` | `target\release-small\rsize.exe` |
| rdup | `cargo build --profile release-small -p rdup` | `target\release-small\rdup.exe` |
| rwall | `cargo build --release -p rwall` | `target\release\rwall.exe` |
| rpack | `cargo build --release -p rpack` | `target\release\rpack.exe` |
| rinst | `cargo build --profile release-small -p rinst` | `target\release-small\rinst.exe` |
| rclean | `cargo build --profile release-small -p rclean` | `target\release-small\rclean.exe` |
| rcare | `cargo build --release -p rcare` | `target\release\rcare.exe` |
| rscoop-cli | `cargo build --profile release-small -p rscoop-cli` | `target\release-small\rscoop-cli.exe` |
| rsize-cli | `cargo build --profile release-small -p rsize-cli` | `target\release-small\rsize-cli.exe` |
| rdup-cli | `cargo build --profile release-small -p rdup-cli` | `target\release-small\rdup-cli.exe` |

The two profiles exist because `panic` cannot be set per package in a workspace.
`release-small` aborts on panic, which makes rcalc, rpad, rsize, rdup, rinst, rclean, rscoop-cli, rsize-cli and rdup-cli about 20% smaller.
rview needs `release`: it catches a panic from the PDF library, so that a
malformed PDF cannot close the program. rwall uses it too, for the same reason
with pictures from the web, and rpack with damaged archives. `./dev.ps1 dist` picks the right profile
for each tool.

## Download

Each tool has its own [releases](https://github.com/mcampilho/rust-tools/releases),
as a zip with the `.exe` for 64-bit Windows 10/11, and a `tar.gz` for 64-bit
Linux. Extract and run: there is nothing to install.

To publish a version, set it in `apps/<tool>/Cargo.toml` and push a tag such as
`rview-v1.1.0`. The [release workflow](.github/workflows/release.yml) then tests
and builds that tool, and publishes it with its SHA-256 sum and build provenance.

### With Scoop

The repository is also a [Scoop](https://scoop.sh) bucket (the `bucket` folder):

```powershell
scoop bucket add rust-tools https://github.com/mcampilho/rust-tools
scoop install rpad
```

The manifests know where each new version is and take its sum from the
release's `SHA256SUMS.txt`. After a release, `checkver.ps1 -Dir bucket -Update`
(from Scoop's own `bin` folder) brings them up to date. Every tool is in the
bucket; the command-line ones (`rscoop-cli`, `rsize-cli`, `rdup-cli`) are put on
the `PATH` and have no Start menu shortcut.

### Updates

Each tool can look for a newer version of itself, only when you ask: **Help ›
Check for Updates…** (rcalc: Settings › About; rwall: the notification-area
icon's menu). It asks GitHub for the tool's releases. If there is a newer one, it
says so and asks before downloading. The download is checked against the
published SHA-256 sum, then put in place of the program, and you choose when to
start the new version. A tool installed with Scoop offers `scoop update <tool>`
instead, so that Scoop keeps track of the version. Nothing else goes to the
network.

## Service (optional, Windows)

[rtsvc](apps/rtsvc/README.md) is a Windows service that, once installed from an elevated prompt (`rtsvc install`), lets rsize, rdup, rinst and rclean show what needs administrator rights (the NTFS table, the programs of every user, the size of Windows' own files) without being started as administrator, and keeps part of it ready in the background. Changing things still needs the tool started as administrator. The tools work as before without it. [Design](docs/service.md) (not yet run as an installed service).

## Linux

The Linux packages need glibc 2.35 or newer (2.34 for rcalc and rpad) and run on
X11 and Wayland. What
differs from Windows is in each README: [rcalc](apps/rcalc/README.md#linux),
[rpad](apps/rpad/README.md#linux), [rview](apps/rview/README.md#linux),
[rsize](apps/rsize/README.md#linux), [rdup](apps/rdup/README.md#linux),
[rwall](apps/rwall/README.md#linux), [rpack](apps/rpack/README.md#linux), [rinst](apps/rinst/README.md#linux) and [rclean](apps/rclean/README.md#linux).
They were checked under WSL2, not on a real Linux desktop.

Under WSLg, the title bar these tools draw on Wayland can crash its compositor,
so they may close with a "Broken pipe" error. Running them with
`WAYLAND_DISPLAY=` uses X11 instead.

## Layout

```text
rust-tools/
├─ .github/        release workflow
├─ Cargo.toml      workspace members and build profiles
├─ dev.ps1         cargo with the Build Tools environment; dist
├─ apps/
│  ├─ rcalc/
│  ├─ rcare/
│  ├─ rclean/
│  ├─ rdup/
│  ├─ rdup-cli/
│  ├─ rpack/
│  ├─ rpad/
│  ├─ rsize/
│  ├─ rsize-cli/
│  ├─ rinst/
│  ├─ rscoop-cli/
│  ├─ rtsvc/
│  ├─ rview/
│  └─ rwall/
└─ crates/         code shared by the apps
   ├─ rt-cli/      what the command-line apps share: texts in en/pt/es, sizes as `du` shows them
   ├─ rt-core/     translations, settings, menu tree
   ├─ rt-clean/    what the cleaner cleans: the targets, measuring and removing them
   ├─ rt-dups/     finding duplicate files and folders: the search, the hash cache, the filters
   ├─ rt-inst/     the programs installed: the registry, Windows Installer, the Store, dpkg and rpm
   ├─ rt-svc/      the privileged service: protocol, pipe, client, the check of its folder
   ├─ rt-gfx/      drawing, text layout, drawn menus, message panel
   ├─ rt-scan/     reading drives fast: Everything, the NTFS table, folder by folder
   ├─ rt-sys/      Windows and Linux integration
   ├─ rt-ui/       window shell: window, theme, drawn menus, messages, controls
   ├─ rt-syntax/   syntax highlighting
   └─ rt-build/    icon and version in the Windows executable
```

Each app keeps its own sources, `build.rs`, README and translation table. The
[shared crates](crates/README.md) explain what they offer and how to start a new
tool from them.

## License

MIT. See [LICENSE](LICENSE).
