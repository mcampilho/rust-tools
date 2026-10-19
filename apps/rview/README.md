# rview

A small, fast, portable document viewer for Windows, written in Rust. It
shows **PDF, Markdown, HTML, text and CSV** files, with zoom, and nothing
else.

`rview.exe` is a single 2.2 MB executable: no installer, no runtime, no
DLLs beyond those that ship with Windows, and no registry writes. It makes
no network connections, has no telemetry and never runs scripts.

## How it renders

| Format | Engine |
| --- | --- |
| Text (`.txt`, `.log`, other files) | rview: decoded once, drawn virtualized (only visible lines) |
| CSV (`.csv`, `.tsv`) | rview: delimiter detection, quoted fields, virtualized grid |
| Markdown (`.md`) | rview: pulldown-cmark → rview's document layout |
| HTML (`.html`, `.htm`) | rview: built-in HTML parser and CSS subset → the same layout |
| PDF (`.pdf`) | Windows' built-in PDF engine (`Windows.Data.Pdf`) |
| Images in Markdown/HTML | Windows' imaging codecs (PNG, JPEG, GIF, BMP, TIFF, ICO, WebP/HEIF if installed) |

The PDF engine and image codecs are part of Windows 10/11, so they add
nothing to the executable. Everything else is drawn by rview's own
CPU renderer (tiny-skia + ab_glyph with the system fonts).

## Build

Requirements: Rust 1.88+ with the MSVC target and the MSVC Build Tools.

```powershell
cargo build --release
# or, with portable Build Tools (loads $env:RVIEW_MSVC_ENV, default
# D:\Portable\Tools\dev\msvc\devcmd.ps1):
./dev.ps1 build --release
./dev.ps1 test
./dev.ps1 clippy --all-targets
```

The output is `target\release\rview.exe`. The C runtime is linked
statically; the icon, version info and manifest (per-monitor DPI v2, long
paths) are embedded by `build.rs`.

## Use

```
rview [file]
```

Open files with Ctrl+O, by dropping them on the window, or from the command
line. Settings (theme, language, word wrap, window size, last folder) are
kept in `%APPDATA%\rview\settings.ini`, or next to `rview.exe` when a file
named `rview.portable` exists there.

| Action | Keys |
| --- | --- |
| Open | Ctrl+O |
| Reload | F5 |
| Close | Ctrl+W |
| Zoom in / out | Ctrl+= / Ctrl+-, Ctrl+mouse wheel (zooms at the pointer) |
| Actual size (100 %) | Ctrl+0 |
| Fit width (PDF) | Ctrl+9 |
| Word wrap (text) | Alt+Z |
| Full screen | F11 (Esc leaves) |
| Scroll | Arrows, Page Up/Down, Space / Shift+Space, Home / End, mouse wheel (Shift for sideways) |
| Previous / next PDF page | Ctrl+Page Up / Ctrl+Page Down |
| Pan | Drag with the mouse |
| Keyboard shortcuts | F1 |

## Details per format

- **Text:** encoding detection (UTF-8, UTF-8 BOM, UTF-16 LE/BE with or
  without BOM, the system code page), CRLF/LF/CR, line numbers, word wrap
  for files up to 16 MB (larger files scroll horizontally), wide CJK
  characters, control characters shown as symbols.
- **CSV:** comma, semicolon, tab or pipe, detected from the first lines;
  quoted fields with `""` escapes and line breaks; sticky header row and row
  numbers; numbers right-aligned; column widths sampled across the file.
- **Markdown:** CommonMark with GitHub tables, task lists, strikethrough
  and footnotes; headings get GitHub-style anchors, so `#links` work;
  embedded HTML is rendered with the HTML engine. Follows the light/dark
  theme.
- **HTML:** documents are shown on a white page like a browser. Supported:
  headings, paragraphs, lists (nested, numbered with `start`), tables
  (with `colspan`), `pre`/`code`, quotes, links and anchors, local and
  `data:` images, `<style>` blocks, local `<link rel=stylesheet>` files,
  `style=""`, legacy `bgcolor`/`align`/`<font>`/`<center>`. CSS: colours,
  backgrounds, font weight/style/size/family (monospace), text alignment,
  text decoration, `display: none`, `white-space: pre`; selectors by tag,
  class, id, attribute, descendant and child, with specificity.
- **PDF:** pages are rendered on a background thread for the visible pages
  (plus the next and previous one) at the exact zoom; up to 256 MB of
  rendered pages are kept and the farthest are dropped first. Fit width is
  the default.
- **Links:** `#anchors` scroll; links to local documents of the supported
  types open in rview; `http`, `https` and `mailto` links open in the
  default browser or mail program on click. Links can never start a
  program. The target is shown in the status bar on hover.

Also: light/dark/system theme (dark menus included), and the interface in
German, English (UK/US), Spanish, French, Italian, Japanese, Portuguese
(Brazil/Portugal) and Simplified Chinese, following Windows or chosen in
View › Display Language.

## Tests and verification

`cargo test` runs 45 tests: the text, CSV, Markdown, HTML and CSS parsers,
the layout engine (wrapping, zoom, long words, links), encodings, links
and image paths, settings, the translation table (every string in the
code and menus has all languages; placeholders and access keys match),
a German render of every document type that fails on any untranslated
string, and the Windows engines: a PDF generated in the test is opened and
page 1 rendered through `Windows.Data.Pdf` (text pixels checked), a broken
PDF reports an error, and a PNG is decoded through the Windows codecs.

Measured on Windows 10 (release build):

- 101 MB log (1,000,001 lines): loaded in 0.37 s, about 140 MB of memory;
  End jumps to the last line instantly.
- 42 MB CSV (1,000,000 rows): loaded in 0.13 s, about 80 MB of memory.
- 12-page illustrated PDF: first page shown at fit width; zoom to 300 %
  re-renders sharply; about 90-125 MB of memory with high-resolution pages.
- Markdown (this kind of README) and an HTML report with CSS, tables, lists,
  entities, a local photo, hidden elements and a script that must not run:
  rendered correctly.
- Release build: no warnings, clippy clean; `dumpbin /dependents` lists only
  Windows system DLLs.

Not exercised by hand: clicking links and dragging with the mouse (the
link rules and hit-testing are unit-tested), screen readers, high-DPI
monitors, very large or password-protected PDFs, right-to-left documents.

## Limitations

- Windows 10 or later (PDF and images use Windows' engines).
- HTML is a document subset, not a browser: no JavaScript, no floats,
  flexbox, grid or absolute positioning, no web fonts, no `@media` queries
  with widths; remote images and style sheets are never fetched. Pages
  made for reading (reports, exported docs, saved articles) look right;
  complex web applications will not.
- Text is drawn without complex shaping: right-to-left scripts and
  scripts needing shaping (Arabic, Devanagari, Thai) show separate glyphs;
  emoji are monochrome.
- PDF: no text selection, search, outline or forms; password-protected
  files cannot be opened.
- No text selection or search in any format, no printing (it is a viewer
  with zoom, by design).
- Files over 512 MB (text, CSV, Markdown, HTML) are refused.

## License

MIT. See [LICENSE](LICENSE).
