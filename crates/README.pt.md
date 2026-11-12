# Crates partilhados

[English version](README.md)

O código que o rcalc, o rpad, o rview, o rsize, o rdup e o rwall têm em comum. Uma nova ferramenta do mesmo
tipo (um único executável que desenha a sua própria interface no CPU) parte destes
crates e só escreve o que é seu.

| Crate | O que dá | Depende de |
| --- | --- | --- |
| `rt-core` | Dez línguas de interface (`i18n`), definições num ficheiro `chave=valor` com modo portátil (`store`), a árvore de menus (`menu`) | nada |
| `rt-gfx` | Desenho com tiny-skia e texto com ab_glyph (`gfx`), disposição de texto monoespaçado (`layout`), menus desenhados na janela (`menu`), painel de mensagens e texto cortado com reticências (`dialog`) | rt-core, tiny-skia, ab_glyph |
| `rt-sys` | Clipboard, tema, língua, links, hora local, Reciclagem ou lixo; em Windows: mensagens da janela principal, instância única, direitos de administrador, menus nativos e escuros, diálogos, ícone de notificação com um menu da própria aplicação (`tray::popup`), a consola de onde um programa foi arrancado (`print_to_console`), integração com o Explorador para o utilizador atual (`assoc`: tipos de ficheiro e pastas), o nome dos tipos de ficheiro (`file_type_name`); em Linux: diálogos zenity/kdialog, instância única por socket Unix, ícone StatusNotifierItem (com um menu da própria aplicação: `Shell::with_items`) | rt-core, windows-sys / arboard, ksni, libc |
| `rt-scan` | Leitura de uma árvore de pastas da forma mais rápida que houver: o índice do Everything, a tabela do NTFS lida diretamente, ou pasta a pasta (`scan_with`); a árvore compacta onde fica (`model`), os discos e o espaço do volume de uma pasta (`volumes`), tamanhos e datas como se mostram (`format`) | rt-core, rt-sys, windows-sys / libc |
| `rt-ui` | A base da janela: criar a janela e a superfície (`window`), o tema da barra de título e dos menus, teclado e rato dos menus desenhados (`menus`), o painel de mensagem do Acerca, atalhos e ajuda (`Message`), o modo residente em Linux (`resident`), o menu do Explorador nas pastas (`explorer`), e os controlos das páginas de definições: botões, caixas de escolha, campos de texto, `steppers` (`controls`) | rt-core, rt-gfx, rt-sys, winit, softbuffer |
| `rt-syntax` | Definições de linguagens e um lexer linha a linha para o realce de sintaxe | nada |
| `rt-build` | Ajuda para o `build.rs`: ícone, versão e manifesto no executável de Windows | tiny-skia |

As versões para Linux incluem sempre X11 e Wayland: o mesmo executável funciona
nos dois.

## O que fica em cada aplicação

- O seu `Shell` e o ciclo de eventos (`main.rs`): o que cada comando faz, as
  suas teclas e o rato. A janela é criada com o `rt_ui::window`.
- Os ecrãs, as cores do tema e o ícone (`ui/appicon.rs`). As páginas de
  definições usam o `rt_ui::controls` (rwall); o rcalc e o rpad mantêm os seus
  controlos, com foco do teclado e nós para leitores de ecrã.
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
5. Janela e desenho: `rt_ui::window::create(el, attrs, nome)` dá a janela e a
   superfície (e também `icon`, `theme`, `apply_theme`, `remember_size`).
   Desenhar cada imagem num `rt_gfx::Gfx` e mostrá-la com
   `rt_ui::window::present`. Controlos: `rt_ui::controls::Painter` com o tipo de
   clique da aplicação, e `Palette::new(escuro, acento)`.
6. Menus: descrevê-los uma vez como `rt_core::menu::Tree<Command>` e implementar
   `MenuCommand` para o tipo dos comandos. Em Windows, o `rt_sys::win::menu::MenuBar`
   mostra-os de forma nativa (`rt_ui::window::menu_theme` para o modo escuro); em
   Linux, o `rt_gfx::menu::Menu` desenha-os e o `rt_ui::menus` transforma teclas e
   cliques num `Step` (abrir um menu, correr um comando, redesenhar). O
   `menu_step` do rsize serve de modelo.
7. Mensagens: `rt_ui::Message::new(título, texto)` para o Acerca, os atalhos de
   teclado e o `--help` sem consola.
8. Sistema: `rt_sys::win::subclass` na janela principal (Windows) ou
   `rt_ui::resident::start` (Linux) para a instância única e o ícone de
   notificação; `rt_ui::explorer` para um verbo nas pastas do Explorador.

O `./dev.ps1 test --workspace` também corre os testes dos crates.
