# rview

A small, fast, portable document viewer for Windows and [Linux](#linux), written
in Rust. It
shows **PDF, Markdown, HTML, text and CSV** files, with zoom, and lets you
select and copy what you see.

[Versão portuguesa](README.pt.md)

rview is part of [rust-tools](../../README.md). Commands below run from the
repository root.

`rview.exe` is a single 4.4 MB executable: no installer, no runtime, no
DLLs beyond those that ship with Windows, and no registry writes unless you
ask for Explorer integration. It makes no network connections, has no
telemetry and never runs scripts.

## Linux

The Linux package is a `tar.gz` with the `rview` binary. It runs on X11 and
Wayland, and needs glibc 2.34 or newer (Ubuntu 22.04, Debian 12, Fedora 35).
Building it needs Rust 1.92 or newer.

- **Rendering.** PDF pages are drawn by [hayro](https://github.com/LaurenzV/hayro)
  and images are decoded by the `image` crate, both inside the binary, so nothing
  else is needed, but it is 16.6 MB, almost four times the Windows one. hayro is young
  (version 0.9): complex PDFs may come out worse than on Windows. Fonts a PDF does
  not embed use the standard PDF fonts, so such Chinese, Japanese or Korean text is
  not shown. Password-protected PDFs do not open.
- **Libraries.** It loads the display libraries when it starts, and they are on
  any desktop: `libwayland-client` and `libxkbcommon` on Wayland, `libX11`,
  `libxcursor`, `libxkbcommon` and `libxkbcommon-x11` on X11. If X11 stops with
  an error naming `libxkbcommon-x11.so`, install `libxkbcommon-x11-0`.
- **Open dialog.** It uses `zenity` or `kdialog`, whichever is installed. Without
  them the status bar says so, and files still open from the command line. While the
  dialog is open the window does not respond, and some desktops may offer to close it.
- **Menus.** They are drawn in the window, with the same items and shortcuts.
  Alt+letter or F10 opens them. The Windows integration is not there.
- **Clipboard.** Copying puts text and HTML, or an image for Alt+drag on a PDF. RTF
  is not offered, and an image goes without its text. Copied data lives in rview
  until a clipboard manager takes it, so it may be lost when rview closes on a
  desktop without one.
- **Fonts.** It uses DejaVu, Noto or Liberation, found under `/usr/share/fonts`,
  `/usr/local/share/fonts`, `~/.local/share/fonts` or `~/.fonts`. Japanese and
  Chinese need Noto Sans CJK, WenQuanYi Zen Hei or Droid Sans Fallback.
- **Notification area.** `--tray` uses StatusNotifierItem, as on KDE, Xfce, Cinnamon
  and others. GNOME needs the AppIndicator extension. With no host for the icon,
  `--tray` is ignored and rview opens as a normal window.
- **Theme and language.** "System" reads the GNOME settings when rview starts, and
  does not follow later changes. The language comes from `LC_ALL`, `LC_MESSAGES` or
  `LANG`. Settings go in `~/.config/rview`.
- **Differences.** Dropping a file on the window works on X11, not on Wayland. Links
  and *Show in Folder* use `xdg-open`, for web and mail links only.

Checked under WSL2 on X11: PDF pages, including ones with LaTeX fonts, selecting and
copying their text, Alt+drag, Markdown with a PNG image, menus and context menu, copy
with and without formats, a damaged PDF, and resident mode. `zenity` was replaced by a
test script, so the real dialog was not seen, and `kdialog` was not run. The tray icon
was checked against a simulated host. Wayland was started, not used. JPEG and the other
image formats, PDF scrolling on long documents, and following links were not tried.

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
CPU renderer (tiny-skia + ab_glyph with the system fonts). Windows' PDF
engine only draws pages, so the text of a PDF is read separately, on a
background thread, with the lopdf library.

## Build

Requirements: Rust 1.88+ with the MSVC target and the MSVC Build Tools.

```powershell
cargo build --release -p rview
# or, with portable Build Tools (loads $env:RUST_TOOLS_MSVC_ENV, or looks in
# \Portable\Tools\dev\msvc on E: and D:):
./dev.ps1 build --release -p rview
./dev.ps1 test -p rview
./dev.ps1 clippy -p rview --all-targets
```

The output is `target\release\rview.exe`. Use the `release` profile: unlike
`release-small`, it keeps panic unwinding, so a malformed PDF cannot close the
program. The C runtime is linked
statically; the icon, version info and manifest (per-monitor DPI v2, long
paths) are embedded by `build.rs`.

## Use

```
rview [--tray] [--new-window] [file]
rview --register | --associate | --unregister
```

Open files with Ctrl+O, by dropping them on the window, or from the command
line.

**Resident mode:** `rview --tray` starts hidden in the notification area.
Launching rview again (with or without a file) or clicking the tray icon
shows the window instantly and opens the file there; closing the window
(or Ctrl+W) only hides it. Quit from the tray icon's menu or File › Exit.
`--new-window` opens a separate window even when rview is resident. To
start it with Windows, put a shortcut to `rview.exe --tray` in
`shell:startup`. Settings (theme, language, word wrap, window size, last folder) are
kept in `%APPDATA%\rview\settings.ini`, or next to `rview.exe` when a file
named `rview.portable` exists there.

**Explorer integration** (File › Windows Integration, or the command
line). Everything is written for the current user only (no administrator
rights) and points to the `rview.exe` you run it from:

- *Explorer Context Menu* / `--register`: adds "Open with rview" to the
  right-click menu of `.pdf`, `.md`, `.markdown`, `.html`, `.htm`, `.txt`,
  `.log`, `.csv` and `.tsv` files, and rview to their "Open with" list.
  Unchecking it (or `--unregister`) removes everything rview added,
  including defaults, and restores the previous ones.
- *Set as Default Viewer* / `--associate`: also registers rview in
  Windows' Default apps and makes it the default for the types where no
  default was chosen by you. Windows protects the defaults you picked
  yourself (for example a PDF reader or browser); rview does not override
  them, but opens Settings › Default apps and lists those types so you can
  pick rview there.

If you move `rview.exe`, the menu item shows unchecked; check it again to
point the entries at the new location. From a console, the command-line
options print the result; otherwise it is shown in a message box. The
exit code is 0 on success.

| Action | Keys |
| --- | --- |
| Open | Ctrl+O |
| Reload | F5 |
| Close (hide in resident mode) | Ctrl+W |
| Select | Drag with the mouse (see below) |
| Select all | Ctrl+A |
| Copy with formats | Ctrl+C |
| Copy as plain text | Ctrl+Shift+C |
| Clear the selection | Esc |
| Zoom in / out | Ctrl+= / Ctrl+-, Ctrl+mouse wheel (zooms at the pointer) |
| Actual size (100 %) | Ctrl+0 |
| Fit width (PDF) | Ctrl+9 |
| Word wrap (text) | Alt+Z |
| Full screen | F11 (Esc leaves) |
| Scroll | Arrows, Page Up/Down, Space / Shift+Space, Home / End, mouse wheel (Shift for sideways) |
| Previous / next PDF page | Ctrl+Page Up / Ctrl+Page Down |
| Pan | Drag with the middle mouse button, or Ctrl+drag |
| Keyboard shortcuts | F1 |

## Selecting and copying

Drag to select, double-click for a word, triple-click for a line or
paragraph, Shift+click to extend, and drag past the edge to keep scrolling.
Ctrl+A selects everything. The Edit menu and the right-click menu offer the
same commands. A link is followed on a click; dragging from it selects.

**Copy** (Ctrl+C) puts the selection on the clipboard as text, HTML and RTF,
so Word, Outlook, Excel, browsers and WordPad keep the formatting: fonts,
sizes, bold, italic, colors, links, headings, lists, quotes, tables and
highlighted code. **Copy as Plain Text** (Ctrl+Shift+C) puts text only.
Copies use the document's own colors, even in the dark theme.

- **Text and code:** any characters; code keeps its syntax colors.
- **Markdown and HTML:** any characters, across paragraphs, lists and
  tables. A selected image goes as a link in the HTML; an image selected on
  its own also goes as a picture.
- **CSV:** cells, in a rectangle; click a row number for the row, a column
  title for the column, or the corner for all. Plain text is tab-separated
  (with spreadsheet quoting), so it pastes into cells.
- **PDF:** text, across pages, with fonts, sizes and colors; lines of a
  paragraph are joined in formatted copies. Alt+drag selects a rectangle of
  the page instead, copied as a picture as shown on screen (and as text, if
  it holds any). Pages with no text, such as scans, always select a
  rectangle; Ctrl+A on a document with no text at all takes the current
  page.

Selections over 8 MB of text (or 200,000 CSV cells) are copied as plain text
only, and the status bar says so.

## Details per format

- **Syntax highlighting:** source files opened as text (Rust, C, C++,
  C#, Java, JavaScript, TypeScript, Go, Python, HTML, CSS, JSON, XML, SQL,
  shell, PowerShell, batch, INI, TOML, YAML, properties, diff; detected
  from the file name or a `#!` line) and code blocks in Markdown
  (` ```rust `) and HTML (`<pre><code class="language-…">`). Files over
  16 MB are shown without highlighting.
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
  the default. Text is read from embedded, Type 1, TrueType, composite and
  Type 3 fonts, in rotated pages and inside form XObjects; it keeps the
  file's own order, which is reading order in most documents.
- **Links:** `#anchors` scroll; links to local documents of the supported
  types open in rview; `http`, `https` and `mailto` links open in the
  default browser or mail program on click. Links can never start a
  program. The target is shown in the status bar on hover.

**Dark mode:** in the dark theme, Markdown, text and CSV use dark colors,
and HTML and PDF pages are shown dark too: HTML author colors are adapted
(light backgrounds become dark, dark text becomes light, hues are kept)
and PDF pages have their lightness inverted. PDF pages that are already
dark (full-page photos, dark slides) keep their colors; photos inside
light pages are inverted with the page. View › Dark Pages in Dark Theme
turns this off: every document (Markdown, text, CSV, HTML, PDF) is then
shown on a light page, as printed, while the window, menus and status bar
stay dark. The About and Keyboard shortcuts panels are drawn by rview in
the current theme.

Also: light/dark/system theme (dark menus included), and the interface in
German, English (UK/US), Spanish, French, Italian, Japanese, Portuguese
(Brazil/Portugal) and Simplified Chinese, following Windows or chosen in
View › Display Language.

## Tests and verification

`cargo test -p rview` runs 95 tests on Windows and 97 on Linux: the text, CSV, Markdown, HTML and CSS parsers,
the layout engine (wrapping, zoom, long words, links), encodings, links
and image paths, settings, the translation table (every string in the
code and menus has all languages; placeholders and access keys match),
a German render of every document type that fails on any untranslated
string (also in the dark theme), syntax highlighting (code fences, HTML
code classes, multi-line comments), dark-page color adaptation (HTML colors
and PDF pages, which keep dark pages unchanged), Explorer integration
(register, associate and unregister against a scratch registry key: a
user's own default is left alone, a previous default is restored, other
programs' verbs survive), and the Windows engines: a PDF generated in the test is opened and
page 1 rendered through `Windows.Data.Pdf` (text pixels checked), a broken
PDF reports an error, and a PNG is decoded through the Windows codecs.
Selection is covered per format (characters, words, lines, wrapped text,
zoom keeping the selection, CSV rows and columns, PDF regions), as are the
plain text, HTML, RTF and picture written for the clipboard, and PDF text
(positions, rotation, crop box, form XObjects, composite fonts, ligatures,
Type 3 fonts).

Measured on Windows 10 (release build):

- 101 MB log (1,000,001 lines): loaded in 0.37 s, about 140 MB of memory;
  End jumps to the last line instantly.
- 42 MB CSV (1,000,000 rows): loaded in 0.13 s, about 80 MB of memory.
- 12-page illustrated PDF: first page shown at fit width; zoom to 300 %
  re-renders sharply; about 90-125 MB of memory with high-resolution pages.
- Markdown (this kind of README) and an HTML report with CSS, tables, lists,
  entities, a local photo, hidden elements and a script that must not run:
  rendered correctly.
- Dark theme: an HTML report with author colors and a text PDF render dark
  and readable; a PDF whose cover is a dark photo keeps it as is. With
  Dark Pages off, Markdown, CSV and PDF are shown light in the dark window;
  the Shortcuts and About panels are dark and close with Esc / Enter.
- Highlighting: a Rust source file and Markdown code blocks (Rust, JSON).
- `--tray`: starts hidden (about 21 MB); a launch with a file hands it to
  the resident window, which shows it; the close button hides it with the
  document kept; a launch without a file shows it; File › Exit quits.
- Selection and copied contents, on documents of every type in the light
  and dark themes: Markdown, HTML, CSV and source code, a PDF with embedded
  TrueType fonts (including a rotated page and a page with only an image)
  and PDFs from pdflatex (Type 1 and Type 3 fonts). With the real mouse and
  keyboard: dragging, double and triple click, the right-click menu, Ctrl+C
  and Ctrl+Shift+C, dragging past the edge, Alt+drag on a PDF, panning with
  the middle button; the Windows clipboard was read back after each copy,
  and the RTF loads correctly in RichEdit (WordPad's engine).
- Release build: no warnings, clippy clean; `dumpbin /dependents` lists only
  Windows system DLLs.

Not exercised by hand: clicking links (the link rules are unit-tested),
pasting into Word, Excel or Outlook, screen readers, high-DPI monitors, very
large or password-protected PDFs, right-to-left documents.

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
- PDF: no search, outline or forms; password-protected files cannot be
  opened. Text with no Unicode information in the file (some older or
  deliberately encoded fonts) copies as `�`, and scanned pages have no text
  (a picture of them can still be copied). Selection follows the file's
  text order, which can jump in unusual layouts.
- No search in any format, no printing (it is a viewer with zoom, by
  design).
- Selection is by mouse, plus Ctrl+A; there is no keyboard caret.
- Files over 512 MB (text, CSV, Markdown, HTML) are refused.

## License

MIT. See [LICENSE](../../LICENSE).
