# rpad

A portable, offline text and code editor for Windows, written in Rust.
`rpad.exe` is one 4 MB executable with no installer, no runtime and no DLLs
beyond those that ship with Windows. It makes no network connections and
has no telemetry.

rpad takes ideas from the classic Windows text editors (tabs, encodings and
line endings, search and replace, text tools, XML helpers), but its code,
icon and artwork are its own.

## Build

Requirements: Rust 1.88+ with the MSVC target (`x86_64-pc-windows-msvc`) and
the MSVC Build Tools (linker and `rc.exe`).

```powershell
# With the Build Tools on PATH (a "Developer PowerShell"):
cargo build --release

# Or with portable Build Tools: dev.ps1 loads the environment from
# $env:RPAD_MSVC_ENV (default D:\Portable\Tools\dev\msvc\devcmd.ps1) first.
./dev.ps1 build --release
./dev.ps1 test
./dev.ps1 clippy --all-targets
```

The output is `target\release\rpad.exe`. The C runtime is statically linked
(`.cargo/config.toml`), and the icon, version info and the application
manifest are embedded by `build.rs`. The manifest enables Common Controls 6,
per-monitor DPI awareness v2 and long paths.

## Run

```
rpad [--tray] [--new-window] [files...]
```

- Files on the command line are opened in tabs. A file that does not exist
  yet opens as an empty tab and is only created when you save it.
- By default a second `rpad` call hands its files to the running window
  (Tools › Preferences › Open Files in the Existing Window). `--new-window` starts a separate window.
- Drag and drop files onto the window to open them.

### Resident mode (`--tray`)

`rpad --tray` starts rpad hidden in the notification area (system tray) for
instant access:

- Clicking the tray icon, or starting `rpad` again (with or without files),
  shows the window immediately; files are opened in it.
- Closing the window (or closing the last tab when it is set to close the
  window) only hides it. Open documents, including unsaved ones, stay as
  they are, and recovery data and settings are written when it hides.
- To quit, use the tray icon's menu (right-click › Exit) or File › Exit.
  The usual rules apply: with recovery on, unsaved text is kept for the
  next start; with recovery off, rpad asks about unsaved documents.
- Hidden, rpad uses about 20-30 MB and no CPU.

To start it with Windows, put a shortcut to `rpad.exe --tray` in the
Startup folder (`shell:startup`); rpad does not add itself.

### Interface language

The interface is available in German (de-DE), English (en-GB, en-US),
Spanish (es-ES), French (fr-FR), Italian (it-IT), Japanese (ja-JP),
Portuguese (pt-BR, pt-PT) and Simplified Chinese (zh-CN). By default it
follows the Windows display language (other languages use English);
View › Display Language changes it immediately, including the menus.

### Portable mode and data

Settings, the session, crash-recovery files, local history and snippets are
stored in `%APPDATA%\rpad`. To keep everything next to the executable (on a
USB stick, for example), create an empty file named `rpad.portable` beside
`rpad.exe`. No administrator rights or registry writes are needed. The
registry is only read, for the font list, theme and accent colour.

If `settings.ini` is damaged, invalid values fall back to defaults. If the
data folder cannot be written, rpad still works and shows a notice.

## Features

**Documents**
- Tabs with modified markers, middle-click to close and a context menu;
  the tab strip scrolls when there are many tabs.
- New, Open (multi-select), Save, Save As, Save All, Close, Close Others,
  Close All, Reopen Closed Tab (Ctrl+Shift+T), Reload, Open Recent (with
  Clear), Copy Path, Reveal in Explorer.
- Closing the last tab either keeps an empty document or closes the window
  (Tools › Preferences).
- Unsaved-changes prompt (Save / Don't Save / Cancel) when closing a tab,
  and when exiting while crash recovery is off.
- **Crash recovery / hot exit** (on by default, can be turned off):
  unsaved text is written to recovery files 2 seconds after editing stops
  and on focus loss, and restored on the next start. Original files are never
  touched by recovery.
- Session restore (open tabs, caret, scroll position).
- **Local history** (off by default): a copy of the previous version is
  kept before each save; File › Local History opens old versions read-only.
- Optional autosave when the window loses focus (off by default).
- Atomic save: a temporary file plus `ReplaceFileW`, falling back to an
  in-place write when the folder does not allow a temporary file.
- Detects files changed or deleted by other programs (when the window gets
  focus), offers Reload / Keep my version, and asks before overwriting a
  newer file on disk.
- Read-only files open read-only; files with binary content (NUL bytes)
  open view-only so they cannot be damaged. Save As works for both.

**Encodings and line endings**
- UTF-8, UTF-8 with BOM, UTF-16 LE/BE (with or without BOM), plus 22 legacy
  code pages (Windows-125x, ISO-8859-x, KOI8, IBM866, Shift_JIS, EUC-JP,
  GBK, Big5, EUC-KR).
- Detection: BOM → UTF-16 heuristics → valid UTF-8 → the system code page.
  Reopen with a different encoding at any time.
- Text is never silently corrupted. If bytes cannot be decoded, the
  document says so and asks before saving replacement characters. Saving
  to an encoding that cannot represent a character is refused, with the
  line and column of the first such character, and the file is left
  unchanged.
- CRLF / LF / CR are detected and preserved; mixed line endings are
  reported. Convert from the Format menu or the status bar.

**Editing**
- Undo/redo with typing coalesced into words; every tool and replace is
  one undo step.
- Multiple carets: Ctrl+click, Alt+drag column selection, Ctrl+Alt+Up/Down,
  add next occurrence (Ctrl+Alt+D), select all occurrences (Ctrl+Shift+L),
  split selection into lines.
- Duplicate, delete, move and join lines; indent/outdent; toggle comment;
  auto-indent with bracket-aware indentation; smart Home; overwrite mode.
- Tabs or spaces, tab width 2/4/8, and converting indentation either way.
- Line numbers, current-line highlight, whitespace and EOL markers, control
  characters drawn as symbols, word wrap, long-line ruler, zoom
  (Ctrl+wheel), font chooser, full screen.
- Wide CJK characters and emoji take two columns; combining marks attach
  to the previous character.

**Search**
- Find and replace bar: match case, whole word, regular expression, in
  selection, wrap-around, live match count ("3 of 12"), highlighted
  matches with markers on the scrollbar, and find/replace history.
- Replace All shows a preview (count, lines, first changes) and asks to
  confirm. Regex replacements support `$1` / `${name}` and `\n`, `\t`.
- Find in open documents, and find in files (folder, filter such as
  `*.rs;*.txt`; skips binary files, files over 32 MB and `.git` / `target`
  / `node_modules` style folders; runs in the background and can be
  stopped). Results are listed in a panel; Enter or a click jumps there.
- Go to line (`line` or `line:column`, previews while typing), go to
  matching bracket (Ctrl+B), go to symbol, and an outline panel.
- Regular expressions use the `regex` crate (linear time, no catastrophic
  backtracking) with size limits.

**Syntax**
- Highlighting for Rust, C, C++, C#, Java, JavaScript, TypeScript, Go,
  Python, HTML, CSS, JSON, XML, Markdown, SQL, Shell, PowerShell, Batch,
  INI, TOML, YAML, Properties and Diff. The language is detected from the
  extension and the first line (shebang, `<?xml`), and can be overridden
  from the status bar. Unknown files are plain text.
- Code folding (braces, tags, indentation, Markdown headings) and matching
  bracket highlight.

**Text tools** (Tools menu; selection or the whole document; all undoable)
- Case: UPPER, lower, Title, Sentence, iNVERT.
- Lines: sort ascending/descending/case-insensitive/natural, reverse,
  remove duplicates, remove blank lines, trim trailing/leading whitespace,
  add prefix/suffix.
- Whitespace: tabs → spaces, leading spaces → tabs.
- Encode/decode: JSON string, XML entities, URL percent-encoding, Base64.
- Changes to many lines of the whole document ask first.

**Markdown and XML**
- Markdown preview pane (off by default, Ctrl+Shift+M), rendered offline.
  Links are shown but never opened; images are shown as their alt text and
  nothing is fetched.
- XML: well-formedness check with line and column of the first error;
  Format XML re-indents element-only content and keeps text, CDATA,
  comments and `xml:space="preserve"` exactly as they are. It refuses to
  touch documents that are not well-formed.

**Other**
- Command palette (Ctrl+Shift+P) with fuzzy search over every command.
- Snippets: insert from a list, save the selection as a snippet, or edit
  `snippets.txt`.
- Light, dark or system theme (follows Windows), the system accent colour,
  and per-monitor DPI. In the dark theme the menu bar, its menus, the
  context menus and the tray menu are dark too.
- Ten interface languages (see above), switchable at run time.
- Native menus with shortcuts and mnemonics, tooltips on toolbar and
  status-bar items, and a status bar showing position, selection, line
  count, insert/overwrite mode, zoom, indentation, encoding, line ending
  and language. Clicking a status item changes it.
- Screen readers: the window exposes its controls (tabs, buttons, fields,
  lists and the editor's current line) through UI Automation via AccessKit.

## Keyboard shortcuts

| Action | Shortcut |
| --- | --- |
| New / Open / Save / Save As | Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S |
| Save All | Ctrl+Alt+S |
| Close tab | Ctrl+W, Ctrl+F4 |
| Reopen closed tab | Ctrl+Shift+T |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+PgDn / Ctrl+PgUp |
| Undo / Redo | Ctrl+Z / Ctrl+Y, Ctrl+Shift+Z |
| Cut / Copy / Paste | Ctrl+X / Ctrl+C / Ctrl+V (Shift+Del, Ctrl+Ins, Shift+Ins) |
| Select all | Ctrl+A |
| Duplicate line or selection | Ctrl+D |
| Delete line | Ctrl+Shift+K |
| Move line up / down | Alt+Up / Alt+Down (Ctrl+Shift+Up / Down) |
| Join lines | Ctrl+J |
| Indent / outdent | Tab / Shift+Tab, Ctrl+] / Ctrl+[ |
| Toggle comment | Ctrl+/ |
| Caret above / below | Ctrl+Alt+Up / Ctrl+Alt+Down |
| Add next occurrence / select all occurrences | Ctrl+Alt+D / Ctrl+Shift+L |
| Split selection into lines | Alt+Shift+I |
| Column selection | Alt+drag |
| Add caret | Ctrl+click |
| Insert date/time | F5 |
| Insert snippet | Ctrl+Shift+J |
| Overwrite mode | Insert |
| Find / Replace | Ctrl+F / Ctrl+H |
| Find next / previous | F3 / Shift+F3 (Enter / Shift+Enter in the find box) |
| Find options (in the find bar) | Alt+C case, Alt+W word, Alt+R regex, Alt+L in selection |
| Replace all (in the find bar) | Alt+A, Ctrl+Alt+Enter |
| Find in files | Ctrl+Shift+F |
| Go to line / bracket / symbol | Ctrl+G / Ctrl+B / Ctrl+Shift+O |
| Word wrap | Alt+Z |
| Fold / unfold current | Ctrl+Shift+[ / Ctrl+Shift+] |
| Fold all / unfold all | Alt+0 / Alt+Shift+0 |
| Zoom in / out / reset | Ctrl+= / Ctrl+- / Ctrl+0, Ctrl+wheel |
| Markdown preview | Ctrl+Shift+M |
| Full screen | F11 |
| Command palette | Ctrl+Shift+P |
| UPPERCASE / lowercase | Ctrl+Shift+U / Ctrl+U |
| Keyboard shortcuts list | F1 |

## Architecture

| Module | Contents |
| --- | --- |
| `doc` | Rope buffer (`ropey`), multi-selection, transactional undo/redo, editing operations |
| `io` | Encoding detection, decoding and strict encoding; file load and atomic save |
| `syntax` | Language definitions, per-line incremental lexer, folding, outline |
| `search` | Find/replace planning and find in files |
| `tools` | Text transformations and the XML checker/formatter |
| `app` | Application state, commands, documents, session, recovery, settings, input handling |
| `ui` | CPU renderer (tiny-skia + ab_glyph with system fonts), widgets, the editor view, Markdown preview |
| `i18n` | Interface languages: the translation table (`strings.txt`), lookup with fallbacks |
| `win` | Win32 integration: menus (with dark mode), common dialogs, clipboard, tray icon, single-instance hand-off |

The application logic talks to the OS through a small `Host` trait (dialogs
and clipboard). This lets the workflow tests drive the whole app without a
window. `unsafe` is limited to Win32 calls: the `win` module, plus single
calls for `ReplaceFileW` (atomic save), `GetACP` (system code page), local
time and the cursor position. Each one has a `SAFETY` comment.

Translations live in `src/i18n/strings.txt`: one line per string, keyed by
its US English text, with tab-separated columns per language. Menu labels
carry their own `&` access key; Japanese and Chinese use the Windows
convention `ファイル(&F)`. Empty en-GB or pt-PT cells fall back to en-US or
pt-BR.

Dark menus use the same undocumented techniques as other Win32 editors:
uxtheme's preferred-app-mode functions for popup menus, and the
`WM_UAHDRAWMENU`/`WM_UAHDRAWMENUITEM` messages to paint the menu bar
(`src/win/darkmode.rs`). The functions are looked up at run time, so on
Windows versions without them the menus just stay light.

## Tests

`cargo test` runs 115 tests. They cover the buffer and editing operations,
encodings (round trips, detection, unmappable characters), line endings,
the lexer, search, text tools, XML, settings, session and recovery files,
layout, and end-to-end workflows through a mock host:

- byte-exact round trips for UTF-8, UTF-8 BOM, UTF-16 LE/BE, CR/LF/CRLF
  and Windows-1252;
- refusing to save unmappable characters, and leaving the file untouched;
- prompts on close (Cancel / Save with cancelled dialog / Save / Don't
  Save), and closing the last tab;
- external change and deletion detection, and confirming before
  overwriting a newer file;
- replace all with preview and a single undo, regex groups, invalid regex;
- crash recovery round trip, recovery disabled, session restore, and local
  history;
- text tools and their undo, line operations, multi-caret typing, the
  clipboard and the command palette;
- a 16+ MB file opening in large-file mode;
- translations: every row has all languages, matching `{}` placeholders,
  line breaks and exactly one menu access key; every string the code
  translates (`tr`, `tf`, notices, tool names, every command, menu and
  palette label) has a row; and the whole window is rendered in German
  with all panels open while recording any lookup without a translation.

## Verified manually (Windows 10, 100 % scaling)

Release build: no warnings, clippy clean, 115/115 tests passing.
`dumpbin /dependents` shows only system DLLs (no VC runtime).

In the running app:
- tabs, Rust and Markdown highlighting, folding markers, the status bar;
- the find/replace bar with live count and scrollbar markers;
- Alt+A → Replace All preview dialog → replaced and marked modified;
- the command palette, Markdown preview, outline panel, toolbar, light and
  dark themes;
- recovery after the process was force-killed (unsaved edits restored);
- the single-instance hand-off;
- a 106 MB / 1,000,001-line log opening in about 0.5 s using about 145 MB
  of RAM, with instant Ctrl+End and Go to line and a background count of
  1,000,000 matches;
- switching the language at run time (German) and starting in Japanese:
  menu bar, menus, find bar, status bar, tabs and notices are translated,
  and CJK text renders in the window and the menus;
- dark menu bar and dark File/Edit/Format menus, context menu and tray
  menu in the dark theme;
- `--tray`: starts hidden (about 20 MB), a second launch shows the window,
  Alt+F4 hides it with unsaved edits kept, launching with a file shows it
  and opens the file, and tray menu › Exit quits (hot exit with recovery).

Not exercised by hand (code paths exist; some are covered by unit tests):
IME composition with a real Asian input method, screen-reader use, every
language other than German and Japanese on screen (they share the same
code and table checks), restarting Explorer while resident (the icon is
re-added on `TaskbarCreated`), high-DPI
monitors and DPI changes, drag-and-drop from Explorer, find in files in the
UI (its search engine is unit-tested), snippets and local history dialogs
in the UI, and fonts other than the default.

## Limitations

- Windows only. The code is mostly portable, but there is no tested build
  for other systems.
- Files over 256 MB are refused. Files over 16 MB open in large-file mode:
  syntax highlighting, folding, the outline and word wrap are off.
- Text is drawn on a monospace cell grid without complex text shaping:
  right-to-left scripts (Arabic, Hebrew) and scripts that need shaping
  (Devanagari, Thai) display as separate glyphs in logical order, emoji are
  monochrome, and there are no ligatures.
- Highlighting uses a per-line lexer per language, not a full parser, so
  rare constructs may be coloured incorrectly. Folding is based on
  brackets, tags, indentation or headings.
- Match highlighting in the editor is capped for very large match counts;
  the count itself is exact.
- Regular expressions match within the document text but do not support
  look-around or backreferences (a limitation of the linear-time engine).
- Undo history is not kept across restarts, although unsaved text is.
- No printing, spell checking, split views, macros or plugins.
- Message boxes and the Open/Save/Font dialogs are drawn by Windows; they
  stay light in the dark theme, and their buttons follow the Windows
  language, not rpad's.
- Keyboard shortcut names (Ctrl, Shift…) are shown in English in menus in
  every language. The command-line help (`--help`) is in English.
- The translations were written for this project; corrections from native
  speakers are welcome in `src/i18n/strings.txt`.

## License

MIT. See [LICENSE](LICENSE).
