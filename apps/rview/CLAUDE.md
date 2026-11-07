# rview — visualizador de documentos portátil em Rust

Visualizador para Windows (~4,4 MB, um só `.exe`) e Linux de PDF, Markdown, HTML, texto
e CSV, com zoom, seleção e cópia para o Clipboard (com formatos ou só texto).
Sem rede, sem telemetria, nunca executa scripts.
Integração opcional com o Explorador (`--register`, `--associate`, `--unregister`).
Detalhes por formato, atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --release -p rview
./dev.ps1 test -p rview       # 80 testes (79 em Linux); os do código partilhado estão nos crates
./dev.ps1 clippy -p rview --all-targets
```

Correm na raiz do repositório. Saída: `target\release\rview.exe`. Rust 1.88+, alvo `x86_64-pc-windows-msvc`.
`build.rs` (com `rt-build`) embebe ícone, versão e manifesto.

## Arquitetura (`src/`)

- `doc/` — texto (virtualizado), CSV (deteção de delimitador, grelha virtualizada) e
  `pdf.rs`: texto dos PDF com posições (lopdf + interpretador próprio do conteúdo das páginas).
- `clip.rs` — o que vai para o Clipboard: um só fluxo de eventos escrito como texto, HTML e RTF.
- `view/selection.rs` — seleção por formato (`Sel`), teste de posição e construção do `Payload`.
- `rich/` — Markdown (pulldown-cmark), parser HTML próprio, subconjunto de CSS, motor de layout comum.
- `syntax/` — cache de estados por linha para o realce; linguagens, lexer e cores vêm de `rt-syntax`.
- `media.rs` — PDF via `Windows.Data.Pdf` e imagens via codecs do Windows; em Linux, hayro e image
  (threads em fundo, cache de 256 MB). Os tamanhos das páginas são em DIPs nos dois sistemas.
- `view.rs`, `main.rs` — vista, janela, input, modo `--tray`.
- `win/` — Win32 de `rt-sys` (menus escuros, diálogos, tray) com a identidade do rview, mais `clipboard.rs`
  (o `Payload` em vários formatos) e `assoc.rs` (associações de ficheiros, só HKCU).
- `linux/` — Clipboard (arboard: texto com HTML, ou imagem), e o resto de `rt-sys`: diálogo Abrir (zenity ou
  kdialog), tema, links, e o modo residente com socket Unix e ícone StatusNotifierItem.
- `ui/menu.rs` — menus desenhados na janela (`rt-gfx`), só fora do Windows. Em Linux, `cfg(not(windows))`
  significa Linux.
- `i18n/strings.txt` — traduções (10 idiomas), mesma convenção do rpad/rcalc.

## Regras

- HTML é um subconjunto de documento, não um browser: sem JS, sem recursos remotos.
- Links `http`/`https`/`mailto` abrem no programa por omissão; links nunca podem lançar programas.
- Integração com o Explorador escreve só em HKCU, não sobrepõe defaults escolhidos pelo
  utilizador e `--unregister` repõe o estado anterior; os testes usam uma chave de registo temporária.
- Qualquer string visível nova precisa de linha completa em `strings.txt`.
- Copiar usa sempre as cores originais do documento, nunca as adaptadas ao tema escuro.
- A leitura de texto dos PDF corre numa thread própria com `catch_unwind`: um ficheiro que faça
  a lopdf entrar em pânico só fica sem texto selecionável. Por isso o rview usa sempre o perfil
  `release`, nunca o `release-small`, que aborta em pânicos.
- Em Linux, esconder a janela destrói-a e mostrar cria-a de novo, porque o Wayland não esconde janelas.
- O hayro e o image correm dentro de `catch_unwind`: um ficheiro malformado dá erro, não fecha o programa.
- O teclado e o rato reais são precisos para testar arrastos: o winit confirma a posição real do
  cursor e ignora `WM_MOUSEMOVE` simulado.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
