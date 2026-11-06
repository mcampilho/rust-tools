# rpad — editor de texto/código portátil em Rust

Editor para Windows e Linux num único executável (~4 MB em Windows): separadores, codificações e
fins de linha, procura/substituição (regex), multi-cursor, realce de sintaxe,
dobragem, ferramentas de texto, XML, pré-visualização de Markdown, recuperação
após crash, modo residente (`--tray`). Sem rede nem telemetria. Funcionalidades,
atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rpad
./dev.ps1 test -p rpad        # 115 testes (122 em Linux)
./dev.ps1 clippy -p rpad --all-targets
```

Correm na raiz do repositório. Saída: `target\release-small\rpad.exe`. Rust 1.88+, alvo `x86_64-pc-windows-msvc`.
`build.rs` embebe ícone, versão e manifesto (Common Controls 6, DPI per-monitor v2, long paths).

## Arquitetura (`src/`)

- `doc` — buffer rope (`ropey`), multi-seleção, undo/redo transacional, operações de edição.
- `io` — deteção/descodificação/codificação estrita de encodings; leitura e gravação atómica.
- `syntax` — definições de linguagens, lexer incremental por linha, folding, outline.
- `search` — planeamento de find/replace e find in files.
- `tools` — transformações de texto, verificador/formatador XML.
- `app` — estado, comandos, sessão, recuperação, definições, input. Fala com o SO através
  do trait `Host` (diálogos, clipboard), o que permite testes de fluxo completos sem janela.
- `ui` — renderer CPU (tiny-skia + ab_glyph), widgets, vista do editor, preview Markdown.
- `win` — Win32: menus (com dark mode via `WM_UAHDRAWMENU`), diálogos, tray, instância única.
- `linux` — Clipboard (arboard), diálogos (zenity ou kdialog), tema, tipos de letra (fontconfig), instância
  única por socket Unix com passagem de ficheiros, tray por StatusNotifierItem (ksni).
- `ui/menu.rs` — menus desenhados na janela, só fora do Windows: estado e navegação; `view.rs` desenha-os.
  Vêm da mesma `menu_tree` dos menus nativos. Em Linux, `cfg(not(windows))` significa Linux.
- `i18n/strings.txt` — traduções: uma linha por string, chave = texto en-US, colunas separadas
  por tabs. Labels de menu levam o seu próprio `&`; ja/zh usam `ファイル(&F)`.

## Regras

- `unsafe` só para chamadas Win32 (módulo `win` e poucas exceções), sempre com comentário `SAFETY`.
- Nunca corromper texto em silêncio: encodings não representáveis são recusados, não substituídos.
- Qualquer string visível nova precisa de linha completa em `strings.txt` (os testes verificam
  placeholders `{}`, quebras de linha e exatamente uma tecla de acesso nos menus).
- Nada de rede; o preview de Markdown nunca abre links nem busca imagens.
- Em Linux, esconder a janela destrói-a e mostrar cria-a de novo, porque o Wayland não esconde janelas.
- Em Linux, gravar segue ligações simbólicas e respeita a falta de permissão de escrita no ficheiro,
  porque o `rename` só precisa de direitos na pasta.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
