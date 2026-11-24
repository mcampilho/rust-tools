# rpad — editor de texto/código portátil em Rust

Editor para Windows e Linux num único executável (~4 MB em Windows): separadores, codificações e
fins de linha, procura/substituição (regex), multi-cursor, realce de sintaxe,
dobragem, ferramentas de texto, XML, pré-visualização de Markdown, recuperação
após crash, modo residente (`--tray`). Sem rede nem telemetria. Funcionalidades,
atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rpad
./dev.ps1 test -p rpad        # 101 testes (102 em Linux); os do código partilhado estão nos crates
./dev.ps1 clippy -p rpad --all-targets
```

Correm na raiz do repositório. Saída: `target\release-small\rpad.exe`. Rust 1.88+, alvo `x86_64-pc-windows-msvc`.
`build.rs` (com `rt-build`) embebe ícone, versão e manifesto (Common Controls 6, DPI per-monitor v2, long paths).

## Arquitetura (`src/`)

- `doc` — buffer rope (`ropey`), multi-seleção, undo/redo transacional, operações de edição.
- `io` — deteção/descodificação/codificação estrita de encodings; leitura e gravação atómica.
- `syntax` — folding e outline; as linguagens e o lexer incremental por linha vêm de `rt-syntax`.
- `search` — planeamento de find/replace e find in files.
- `tools` — transformações de texto, verificador/formatador XML.
- `app` — estado, comandos, sessão, recuperação, definições, input. Fala com o SO através
  do trait `Host` (diálogos, clipboard), o que permite testes de fluxo completos sem janela.
- `ui` — widgets, vista do editor, preview Markdown; o desenho no CPU vem de `rt-gfx`.
- `main.rs` — a janela e o ciclo de eventos; o `impl Shell` está repartido por `input.rs` (teclado, rato, cursor), `menus.rs` (menus e o seu tema) e `render.rs` (desenho e árvore para leitores de ecrã). A criação da janela, o ícone e as teclas de acesso vêm do `rt-ui`.
- `win` — Win32 de `rt-sys` (menus com dark mode via `WM_UAHDRAWMENU`, diálogos, tray, instância única),
  com a identidade do rpad e os seus tipos de ficheiro; `win/assoc.rs`: os tipos para "Editar com o rpad" e
  editor predefinido (`rt_sys::win::assoc`, só HKCU), pelo menu Ficheiro › Integração com o Windows ou
  `--register`/`--associate`/`--unregister` (tratados no `main.rs`, não na `App`). "Definir como editor
  predefinido" abre o painel `rt_ui::assoc::AssocPanel` (`Shell::assoc`; 2026-11-14): tipos a marcar, quem abre
  cada um, "Abrir com…", Aplicar. `.inf` e `.url` acrescentados nesse dia; o `--associate` deixa o `.url` de fora
  (`NOT_BY_DEFAULT`: o duplo clique num atalho de Internet abre o navegador). `.bat` e `.cmd` (2026-11-24): o rpad
  tornava-se o programa predefinido deles (`HKCU\…\Classes\.bat` = `rpad.code`, por cima do `batfile` do Windows) e o duplo
  clique abria o editor em vez de executar; agora `rt_sys::win::assoc::RUNNABLE` (`.bat .cmd .com .exe .msi .scr .lnk
  .pif`) nunca é predefinido, mesmo que se peça, e onde já era é devolvido ao Windows ao Aplicar ou no `--associate`
  (o painel mostra-os "(it runs when opened)" e não os deixa marcar). Ficam com "Editar com o rpad". "Iniciar com o Windows"
  (`Command::Autostart`, `rt_sys::autostart`) também é tratado no `main.rs`; fica nesse submenu (no Linux em
  Ferramentas › Preferências) porque todas as letras dele já são teclas de acesso no menu Ficheiro.
- `trace.rs` (2026-11-17, provisório): `handoff.log` na pasta das definições regista cada arranque com ficheiros (para
  que rpad vão, visível/minimizado, se foram entregues) e o que a janela aberta recebeu e abriu. O utilizador viu
  ficheiros do menu de contexto não abrirem ("nada, nem separador") com o rpad aberto e minimizado; a causa ainda
  não foi encontrada no código. Tirar quando estiver resolvido. Em 2026-11-24 o `handoff.log` (56 linhas) mostrava todas as
  entregas recebidas e abertas (nenhum "NOT opened"), por isso a falha é visual: a janela escondida (tabuleiro) ou
  minimizada abria-se atrás das outras. Corrigido o que se vê no código: `rt_sys::win::hand_off` (com `activate`) espera até
  1 s que a janela fique visível e é o processo acabado de arrancar pelo clique do utilizador que a traz para a frente
  (restaura e `SetForegroundWindow`); `Shell::open_files` também restaura a janela minimizada. **Não verificado a sério**
  (o utilizador tem o rpad aberto e as instâncias reconhecem-se todas pela mesma janela): o `trace.rs` fica até ele confirmar.
- `links.rs` (2026-11-24, pedido do utilizador): ligações `http(s)://`, `mailto:` e `www.` por linha; o `ui/view.rs`
  sublinha-as e o duplo clique (`App::mouse_down`) abre-as com `rt_sys::open_url` (o Ctrl+clique é multi-cursor).
- `Host::info` não abre caixa do sistema: a mensagem (About, `--help` sem consola...) é desenhada na janela,
  com o tema (`Shell::dialog`, `rt_gfx::dialog::draw_message`). `--tray` com a janela maximizada da última
  vez: maximiza só ao mostrar (`maximize_on_show`), porque maximizar mostra a janela no Windows.
- `linux` — de `rt-sys`: Clipboard (arboard), diálogos (zenity ou kdialog), tema, tipos de letra
  (fontconfig), instância única por socket Unix com passagem de ficheiros, tray por StatusNotifierItem.
- `ui/menu.rs` — menus desenhados na janela, só fora do Windows: estado e navegação em `rt-gfx`; `view.rs`
  desenha-os. Vêm da mesma `menu_tree` dos menus nativos. Em Linux, `cfg(not(windows))` significa Linux.
- `i18n/strings.txt` — traduções: uma linha por string, chave = texto en-US, colunas separadas
  por tabs. Labels de menu levam o seu próprio `&`; ja/zh usam `ファイル(&F)`.

## Regras

- `unsafe` só para chamadas Win32 (em `rt-sys` e poucas exceções), sempre com comentário `SAFETY`.
- Nunca corromper texto em silêncio: encodings não representáveis são recusados, não substituídos.
- Qualquer string visível nova precisa de linha completa em `strings.txt` (os testes verificam
  placeholders `{}`, quebras de linha e exatamente uma tecla de acesso nos menus).
- Dentro de cada menu, as teclas de acesso não se repetem em nenhuma língua (`rt_core::menu::access_key_clashes`,
  com teste). Os menus de contexto do editor e dos separadores estão em `app/commands.rs` para o teste os ver.
- Nada de rede; o preview de Markdown nunca abre links nem busca imagens.
- Em Linux, esconder a janela destrói-a e mostrar cria-a de novo, porque o Wayland não esconde janelas.
- Em Linux, gravar segue ligações simbólicas e respeita a falta de permissão de escrita no ficheiro,
  porque o `rename` só precisa de direitos na pasta.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
