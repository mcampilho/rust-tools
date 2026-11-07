# rpad — editor de texto/código portátil em Rust

Editor para Windows e Linux num único executável (~4 MB em Windows): separadores, codificações e
fins de linha, procura/substituição (regex), multi-cursor, realce de sintaxe,
dobragem, ferramentas de texto, XML, pré-visualização de Markdown, recuperação
após crash, modo residente (`--tray`). Sem rede nem telemetria. Funcionalidades,
atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rpad
./dev.ps1 test -p rpad        # 100 testes (101 em Linux); os do código partilhado estão nos crates
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
- `win` — Win32 de `rt-sys` (menus com dark mode via `WM_UAHDRAWMENU`, diálogos, tray, instância única),
  com a identidade do rpad e os seus tipos de ficheiro.
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
- Os menus têm teclas de acesso repetidas dentro do mesmo menu (184 casos nas 10 línguas, 11 em inglês, em
  2026-11-07). Por isso o teste `rt_core::menu::access_key_clashes`, usado no rview e no rsize, ainda não corre aqui.
- Nada de rede; o preview de Markdown nunca abre links nem busca imagens.
- Em Linux, esconder a janela destrói-a e mostrar cria-a de novo, porque o Wayland não esconde janelas.
- Em Linux, gravar segue ligações simbólicas e respeita a falta de permissão de escrita no ficheiro,
  porque o `rename` só precisa de direitos na pasta.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
