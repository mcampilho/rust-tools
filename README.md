# rust-tools

Small, portable desktop tools for Windows, written in Rust. Each one is a
single `.exe`: no installer, no runtime, no DLLs beyond those that ship with
Windows, and no telemetry.

[Versão portuguesa](README.pt.md)

| Tool | What it is | Size |
| --- | --- | --- |
| [rcalc](apps/rcalc/README.md) | Calculator: standard, scientific, programmer, dates and unit converters | 1.9 MB |
| [rpad](apps/rpad/README.md) | Text and code editor: tabs, encodings, regex search, multiple carets, syntax highlighting | 4.2 MB |
| [rview](apps/rview/README.md) | Document viewer for PDF, Markdown, HTML, text and CSV, with selection and copy | 4.4 MB |

All three draw their own interface on the CPU (winit, softbuffer, tiny-skia and
ab_glyph), with no GPU and no UI toolkit. They come in 10 languages, follow the
Windows light or dark theme, and can stay resident in the notification area
with `--tray`.

## Build

Requirements: Windows, Rust 1.88+ with the `x86_64-pc-windows-msvc` target, and
the MSVC Build Tools (linker and `rc.exe`).

```powershell
./dev.ps1 dist              # release builds of all three, in dist\
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

The two profiles exist because `panic` cannot be set per package in a workspace.
`release-small` aborts on panic, which makes rcalc and rpad about 20% smaller.
rview needs `release`: it catches a panic from the PDF library, so that a
malformed PDF cannot close the program. `./dev.ps1 dist` picks the right profile
for each tool.

## Download

Each tool has its own [releases](https://github.com/mcampilho/rust-tools/releases),
as a zip with the `.exe`, for 64-bit Windows 10/11. Unzip and run: there is
nothing to install. rcalc and rpad also have a Linux package.

To publish a version, set it in `apps/<tool>/Cargo.toml` and push a tag such as
`rview-v1.1.0`. The [release workflow](.github/workflows/release.yml) then tests
and builds that tool, and publishes it with its SHA-256 sum and build provenance.

## Linux

The three tools compile on Linux and their tests pass there, and the workflow
keeps it that way. rcalc and rpad are complete enough to have a package:

| Tool | On Linux, for now |
| --- | --- |
| rcalc | Works: see [its README](apps/rcalc/README.md#linux) |
| rpad | Works: see [its README](apps/rpad/README.md#linux) |
| rview | No package: opens Markdown, HTML, text and CSV; no PDF, images, menus or clipboard |

## Layout

```text
rust-tools/
├─ .github/        release workflow
├─ Cargo.toml      workspace members and build profiles
├─ dev.ps1         cargo with the Build Tools environment; dist
└─ apps/
   ├─ rcalc/
   ├─ rpad/
   └─ rview/
```

Each app keeps its own sources, `build.rs`, README and translation table.

## License

MIT. See [LICENSE](LICENSE).
