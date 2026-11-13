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
| [rwall](apps/rwall/README.md) | Wallpaper changer in the notification area: every day, twice or four times a day, weekly, monthly or every few minutes, with pictures from Bing, NASA, Wallhaven, Picsum or your own folders; on Windows also a dynamic wallpaper behind the icons (video, animated GIF or a drawn scene) and desktop widgets (clocks, calendar, notes, weather, fences, photos) | 3.7 MB | 6.8 MB |
| [rpack](apps/rpack/README.md) | Archiver with a file browser: makes 7z, zip and tar archives; opens, tests and extracts zip, 7z, tar and files compressed with gzip, bzip2, xz, lzma, lzip, zstd, lz4 or brotli, found by their contents; hashes from CRC-32 to BLAKE3; passwords, rar, iso and more come next | 4.8 MB | 9.3 MB |
| [runinst](apps/runinst/README.md) | Uninstaller: the programs installed for everyone, for you and (as administrator) for every other user, from the registry, Windows Installer, the Store and winget in one list, Scoop and Chocolatey in tabs, with shortcuts, icons and newer versions; uninstalls one or several in a row (normal or quiet, waiting for what each uninstaller starts); then finds and removes the leftovers (folders, shortcuts, startup entries, registry keys, services, scheduled tasks; `.reg` copy and Recycle Bin), also of programs removed before; tabs for Windows features (DISM) and npm, pip, Cargo and vcpkg; on Linux, dpkg, rpm, Flatpak and Snap packages | 2.1 MB | 5.9 MB |

They all draw their own interface on the CPU (winit, softbuffer, tiny-skia and
ab_glyph), with no GPU and no UI toolkit. They come in 10 languages and follow
the system's light or dark theme. They can also stay resident in the
notification area with `--tray`.

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
| runinst | `cargo build --profile release-small -p runinst` | `target\release-small\runinst.exe` |

The two profiles exist because `panic` cannot be set per package in a workspace.
`release-small` aborts on panic, which makes rcalc, rpad, rsize, rdup and runinst about 20% smaller.
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

## Linux

The Linux packages need glibc 2.35 or newer (2.34 for rcalc and rpad) and run on
X11 and Wayland. What
differs from Windows is in each README: [rcalc](apps/rcalc/README.md#linux),
[rpad](apps/rpad/README.md#linux), [rview](apps/rview/README.md#linux),
[rsize](apps/rsize/README.md#linux), [rdup](apps/rdup/README.md#linux),
[rwall](apps/rwall/README.md#linux), [rpack](apps/rpack/README.md#linux) and [runinst](apps/runinst/README.md#linux).
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
│  ├─ rdup/
│  ├─ rpack/
│  ├─ rpad/
│  ├─ rsize/
│  ├─ runinst/
│  ├─ rview/
│  └─ rwall/
└─ crates/         code shared by the apps
   ├─ rt-core/     translations, settings, menu tree
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
