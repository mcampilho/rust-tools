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

## Layout

```text
rust-tools/
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
