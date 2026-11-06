# rpad — editor de texto/código portátil em Rust

Editor para Windows num único `rpad.exe` (~4 MB): separadores, codificações e
fins de linha, procura/substituição (regex), multi-cursor, realce de sintaxe,
dobragem, ferramentas de texto, XML, pré-visualização de Markdown, recuperação
após crash, modo residente (`--tray`). Sem rede nem telemetria. Funcionalidades,
atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --release     # carrega o MSVC portátil (RPAD_MSVC_ENV, por omissão D:\Portable\Tools\dev\msvc\devcmd.ps1)
./dev.ps1 test                # 115 testes
./dev.ps1 clippy --all-targets
```

Saída: `target\release\rpad.exe`. Rust 1.88+, alvo `x86_64-pc-windows-msvc`.
`build.rs` embebe ícone, versão e manifesto (Common Controls 6, DPI per-monitor v2, long paths).
O critério de "pronto" é: build release sem avisos, clippy limpo, todos os testes a passar.

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
- `i18n/strings.txt` — traduções: uma linha por string, chave = texto en-US, colunas separadas
  por tabs. Labels de menu levam o seu próprio `&`; ja/zh usam `ファイル(&F)`.

## Regras

- `unsafe` só para chamadas Win32 (módulo `win` e poucas exceções), sempre com comentário `SAFETY`.
- Nunca corromper texto em silêncio: encodings não representáveis são recusados, não substituídos.
- Qualquer string visível nova precisa de linha completa em `strings.txt` (os testes verificam
  placeholders `{}`, quebras de linha e exatamente uma tecla de acesso nos menus).
- Nada de rede; o preview de Markdown nunca abre links nem busca imagens.
- `rustfmt.toml`: `max_width = 140`.

## Git

Repositório local, ramo `master`, sem remote.
