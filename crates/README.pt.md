# Crates partilhados

[English version](README.md)

O código que o rcalc, o rpad, o rview e o rsize têm em comum. Uma nova ferramenta do mesmo
tipo (um único executável que desenha a sua própria interface no CPU) parte destes
crates e só escreve o que é seu.

| Crate | O que dá | Depende de |
| --- | --- | --- |
| `rt-core` | Dez línguas de interface (`i18n`), definições num ficheiro `chave=valor` com modo portátil (`store`), a árvore de menus (`menu`) | nada |
| `rt-gfx` | Desenho com tiny-skia e texto com ab_glyph (`gfx`), disposição de texto monoespaçado (`layout`), menus desenhados na janela (`menu`), painel de mensagens e texto cortado com reticências (`dialog`) | rt-core, tiny-skia, ab_glyph |
| `rt-sys` | Clipboard, tema, língua, links, hora local, Reciclagem ou lixo; em Windows: mensagens da janela principal, instância única, direitos de administrador, menus nativos e escuros, diálogos, ícone de notificação; em Linux: diálogos zenity/kdialog, instância única por socket Unix, ícone StatusNotifierItem | rt-core, windows-sys / arboard, ksni, libc |
| `rt-syntax` | Definições de linguagens e um lexer linha a linha para o realce de sintaxe | nada |
| `rt-build` | Ajuda para o `build.rs`: ícone, versão e manifesto no executável de Windows | tiny-skia |

As versões para Linux incluem sempre X11 e Wayland: o mesmo executável funciona
nos dois.

## O que fica em cada aplicação

- A janela e o ciclo de eventos (`main.rs`), com winit e softbuffer.
- Os ecrãs e os controlos, as cores do tema e o ícone (`ui/appicon.rs`).
- A tabela de traduções (`src/i18n/strings.txt`). Os textos usados pelos crates
  (botões dos diálogos, erros das definições) estão em `rt-core/src/strings.txt`,
  procurados depois da tabela da aplicação.
- Módulos pequenos que dão aos crates a identidade da aplicação: nome, ficheiro de
  definições, tipos de ficheiro, etiqueta `WM_COPYDATA`.

## Começar uma ferramenta nova

1. Criar `apps/<nome>/` com um `Cargo.toml` que dependa dos crates de que precisa
   (caminhos `../../crates/rt-*`), mais winit, softbuffer e tiny-skia, com as
   features de Linux das outras aplicações (`x11`, `wayland`, ...). O rcalc é o
   exemplo mais pequeno; o rsize, o mais recente, com menus e sem modo residente.
2. `build.rs`: incluir `src/ui/appicon.rs` e chamar
   `rt_build::windows_resources(&rt_build::Resources { name, description, icon, icon_source, manifest })`.
3. Traduções: um `Catalog` sobre o `strings.txt`, instalado antes da primeira procura:

   ```rust
   static TABLE: rt_core::i18n::Catalog = rt_core::i18n::Catalog::new(include_str!("strings.txt"), '\t');
   rt_core::i18n::install(&TABLE);
   rt_core::i18n::set_lang(rt_sys::system_lang());
   ```

   O `rt_core::i18n::check_table` e o `record_misses` ajudam a escrever os testes
   que garantem que todos os textos estão traduzidos.
4. Definições: `rt_core::store::Location::detect(Place { app, file, file_marks_portable })`.
5. Desenho: `rt_gfx::Gfx::new()`, desenhar cada imagem nele e copiá-la para a
   superfície do softbuffer com `copy_to`.
6. Menus: descrevê-los uma vez como `rt_core::menu::Tree<Command>` e implementar
   `MenuCommand` para o tipo dos comandos. Em Windows, o `rt_sys::win::menu::MenuBar`
   mostra-os de forma nativa; em Linux, o `rt_gfx::menu::Menu` desenha-os.
7. Sistema: `rt_sys::win::subclass` na janela principal (Windows) ou
   `rt_sys::linux::shell::Shell` (Linux) para a instância única e o ícone de
   notificação.

O `./dev.ps1 test --workspace` também corre os testes dos crates.
