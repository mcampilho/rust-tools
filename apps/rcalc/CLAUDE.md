# rcalc — calculadora portátil em Rust

Calculadora de secretária para Windows inspirada na Calculadora do Windows
(Standard, Scientific, Programmer, Date, Converters). Um único `rcalc.exe`
(~1,9 MB), sem instalação, sem rede. Detalhes de funcionalidades, precisão
numérica e atalhos estão no `README.md` (e `README.pt.md`) — consulta-o antes de mudar comportamento.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rcalc
./dev.ps1 test -p rcalc       # 69 testes (os do código partilhado estão nos crates)
./dev.ps1 clippy -p rcalc --all-targets
./dev.ps1 test -p rcalc --lib screenshots '--' --ignored   # renderiza todas as vistas para apps/rcalc/target/screens/*.png
```

Correm na raiz do repositório. Saída: `target\release-small\rcalc.exe`. Rust 1.88+, alvo `x86_64-pc-windows-msvc`.
`.cargo/config.toml` liga o CRT estaticamente; `build.rs` (com `rt-build`) desenha o ícone e
embebe ícone + versão com `rc.exe` (sem `rc.exe` compila na mesma, sem ícone).

## Arquitetura

- `src/num/` — tipo decimal de precisão arbitrária (`coeficiente × 10^exp`), 64 dígitos.
- `src/expr/` — tokenizer + parser Pratt + avaliador.
- `src/calc/` — motores do teclado (Standard/Scientific/Programmer) e inteiros do Programmer.
- `src/app.rs` — estado da aplicação, ações, atalhos, persistência.
- `src/ui/` — camada immediate-mode própria: `gfx` (reexporta `rt-gfx`), `view` (layouts),
  `icons.rs` (ícones em caminhos vetoriais), `appicon.rs` (ícone da app).
- `src/platform.rs` — Clipboard, tema, cor de destaque e data, de `rt-sys`; tecla Calculadora (Windows).
- `src/winshell.rs` — janela escondida que recebe a ativação, a tecla Calculadora e o ícone de notificação
  (`rt_sys::win::tray`). `src/linuxshell.rs` — instância única e ícone, com `rt_sys::linux::shell`.
  `crate::shell` é o módulo da plataforma.
- `src/storage.rs`, `src/i18n/mod.rs` — `rt-core` com o nome, o ficheiro `rcalc.ini` e a tabela do rcalc;
  o `i18n` acrescenta os separadores de números e as datas.
- `src/i18n/strings.txt` — tabela de traduções (10 idiomas), uma linha por string, colunas separadas por `|`.
- Stack: winit + softbuffer (sem GPU) + tiny-skia + ab_glyph + accesskit. Não introduzir toolkits de UI (egui/iced/Slint).

## Regras

- Qualquer texto visível novo tem de ter linha em `src/i18n/strings.txt` com todos os idiomas;
  os testes de i18n falham se faltar (incluindo um render em alemão que deteta inglês no ecrã).
- Nada de rede: a app nunca abre ligações (taxas de câmbio vêm embutidas ou de `currency.txt`).
- Não copiar código, ícones ou traduções da Calculadora da Microsoft — só comportamento/aspeto.
- Partes só-Windows atrás de `cfg(windows)`.
- Em Linux, esconder a janela destrói-a e mostrar cria-a de novo, porque o Wayland não esconde janelas.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
