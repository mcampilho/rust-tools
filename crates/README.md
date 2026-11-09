# Shared crates

[Versão portuguesa](README.pt.md)

The code that rcalc, rpad, rview, rsize and rdup have in common. A new tool of the same kind
(a single executable that draws its own interface on the CPU) starts from these
crates and only writes what is its own.

| Crate | What it gives | Depends on |
| --- | --- | --- |
| `rt-core` | Ten UI languages (`i18n`), settings in a `key=value` file with portable mode (`store`), the menu tree (`menu`) | nothing |
| `rt-gfx` | Drawing with tiny-skia and text with ab_glyph (`gfx`), monospace text layout (`layout`), menus drawn in the window (`menu`), a message panel and text cut with an ellipsis (`dialog`) | rt-core, tiny-skia, ab_glyph |
| `rt-sys` | Clipboard, theme, language, links, local time, the Recycle Bin or trash; on Windows: main window messages, single instance, administrator rights, native and dark menus, dialogs, tray icon, the console a program was started from (`print_to_console`), Explorer integration for the current user (`assoc`: file types and folders); on Linux: zenity/kdialog dialogs, single instance over a Unix socket, StatusNotifierItem icon | rt-core, windows-sys / arboard, ksni, libc |
| `rt-scan` | Reading a folder tree the fastest way there is: the Everything index, the NTFS table read directly, or folder by folder (`scan_with`); the compact tree it is read into (`model`), the drives (`volumes`), sizes and dates as shown (`format`) | rt-core, rt-sys, windows-sys / libc |
| `rt-syntax` | Language definitions and a line-at-a-time lexer for syntax highlighting | nothing |
| `rt-build` | `build.rs` helper: icon, version and manifest in the Windows executable | tiny-skia |

Linux builds always include both X11 and Wayland: the same executable runs on
either.

## What stays in each app

- Its window and event loop (`main.rs`) with winit and softbuffer.
- Its screens and widgets, its theme colours and its icon (`ui/appicon.rs`).
- Its translation table (`src/i18n/strings.txt`). Strings used by the crates
  (dialog buttons, settings errors) are in `rt-core/src/strings.txt`, looked up
  after the app's table.
- Small modules that give the crates the app's identity: name, settings file,
  file types, `WM_COPYDATA` tag.

## Starting a new tool

1. Create `apps/<name>/` with a `Cargo.toml` that depends on the crates it needs
   (paths `../../crates/rt-*`), plus winit, softbuffer and tiny-skia, with the
   Linux features of the other apps (`x11`, `wayland`, ...). rcalc is the
   smallest example; rsize the newest, with menus and no resident mode.
2. `build.rs`: include `src/ui/appicon.rs` and call
   `rt_build::windows_resources(&rt_build::Resources { name, description, icon, icon_source, manifest })`.
3. Translations: a `Catalog` over `strings.txt`, installed before the first lookup:

   ```rust
   static TABLE: rt_core::i18n::Catalog = rt_core::i18n::Catalog::new(include_str!("strings.txt"), '\t');
   rt_core::i18n::install(&TABLE);
   rt_core::i18n::set_lang(rt_sys::system_lang());
   ```

   `rt_core::i18n::check_table` and `record_misses` help write the tests that
   keep every string translated.
4. Settings: `rt_core::store::Location::detect(Place { app, file, file_marks_portable })`.
5. Drawing: `rt_gfx::Gfx::new()`, draw each frame into it and copy it to the
   softbuffer surface with `copy_to`.
6. Menus: describe them once as `rt_core::menu::Tree<Command>` and implement
   `MenuCommand` for the command type. On Windows, `rt_sys::win::menu::MenuBar`
   shows them natively; on Linux, `rt_gfx::menu::Menu` draws them.
7. System: `rt_sys::win::subclass` on the main window (Windows) or
   `rt_sys::linux::shell::Shell` (Linux) for single instance and the tray icon.

`./dev.ps1 test --workspace` also runs the crates' own tests.
