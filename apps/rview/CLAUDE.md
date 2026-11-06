# rview — visualizador de documentos portátil em Rust

Visualizador para Windows (~4,4 MB, um só `.exe`) de PDF, Markdown, HTML, texto
e CSV, com zoom, seleção e cópia para o Clipboard (com formatos ou só texto).
Sem rede, sem telemetria, nunca executa scripts.
Integração opcional com o Explorador (`--register`, `--associate`, `--unregister`).
Detalhes por formato, atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --release     # carrega o MSVC portátil (RVIEW_MSVC_ENV, por omissão D:\Portable\Tools\dev\msvc\devcmd.ps1)
./dev.ps1 test                # 95 testes
./dev.ps1 clippy --all-targets
```

Saída: `target\release\rview.exe`. Rust 1.88+, alvo `x86_64-pc-windows-msvc`.
`build.rs` embebe ícone, versão e manifesto. "Pronto" = sem avisos, clippy limpo, testes a passar.

## Arquitetura (`src/`)

- `doc/` — texto (virtualizado), CSV (deteção de delimitador, grelha virtualizada) e
  `pdf.rs`: texto dos PDF com posições (lopdf + interpretador próprio do conteúdo das páginas).
- `clip.rs` — o que vai para o Clipboard: um só fluxo de eventos escrito como texto, HTML e RTF.
- `view/selection.rs` — seleção por formato (`Sel`), teste de posição e construção do `Payload`.
- `rich/` — Markdown (pulldown-cmark), parser HTML próprio, subconjunto de CSS, motor de layout comum.
- `syntax/` — lexer por linha para realce de código (partilhado em espírito com o rpad).
- `media.rs` — PDF via `Windows.Data.Pdf` e imagens via codecs do Windows (threads em fundo, cache de 256 MB).
- `view.rs`, `main.rs` — vista, janela, input, modo `--tray`.
- `win/` — Win32: menus escuros, diálogos, tray, `clipboard.rs`, `assoc.rs` (associações de ficheiros, só HKCU).
- `i18n/strings.txt` — traduções (10 idiomas), mesma convenção do rpad/rcalc.

## Regras

- HTML é um subconjunto de documento, não um browser: sem JS, sem recursos remotos.
- Links `http`/`https`/`mailto` abrem no programa por omissão; links nunca podem lançar programas.
- Integração com o Explorador escreve só em HKCU, não sobrepõe defaults escolhidos pelo
  utilizador e `--unregister` repõe o estado anterior; os testes usam uma chave de registo temporária.
- Qualquer string visível nova precisa de linha completa em `strings.txt`.
- Copiar usa sempre as cores originais do documento, nunca as adaptadas ao tema escuro.
- A leitura de texto dos PDF corre numa thread própria com `catch_unwind`: um ficheiro que faça
  a lopdf entrar em pânico só fica sem texto selecionável. Por isso o perfil release não usa
  `panic = "abort"`.
- O teclado e o rato reais são precisos para testar arrastos: o winit confirma a posição real do
  cursor e ignora `WM_MOUSEMOVE` simulado.
- `README.md` e `README.pt.md` são atualizados em conjunto.
- `rustfmt.toml`: `max_width = 140`.

## Git

Repositório local, ramo `master`, sem remote.
