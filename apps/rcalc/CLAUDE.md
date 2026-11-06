# rcalc — calculadora portátil em Rust

Calculadora de secretária para Windows inspirada na Calculadora do Windows
(Standard, Scientific, Programmer, Date, Converters). Um único `rcalc.exe`
(~1,9 MB), sem instalação, sem rede. Detalhes de funcionalidades, precisão
numérica e atalhos estão no `README.md` — consulta-o antes de mudar comportamento.

## Comandos

```powershell
./dev.ps1 build --release     # carrega o MSVC portátil (RCALC_MSVC_ENV, por omissão D:\Portable\Tools\dev\msvc\devcmd.ps1)
./dev.ps1 test
./dev.ps1 clippy --all-targets
cargo test --lib screenshots -- --ignored   # renderiza todas as vistas para target/screens/*.png
```

Saída: `target\release\rcalc.exe`. Rust 1.88+, alvo `x86_64-pc-windows-msvc`.
`.cargo/config.toml` liga o CRT estaticamente; `build.rs` desenha o ícone e
embebe ícone + versão com `rc.exe` (sem `rc.exe` compila na mesma, sem ícone).

## Arquitetura

- `src/num/` — tipo decimal de precisão arbitrária (`coeficiente × 10^exp`), 64 dígitos.
- `src/expr/` — tokenizer + parser Pratt + avaliador.
- `src/calc/` — motores do teclado (Standard/Scientific/Programmer) e inteiros do Programmer.
- `src/app.rs` — estado da aplicação, ações, atalhos, persistência.
- `src/ui/` — camada immediate-mode própria: `gfx` (tiny-skia + ab_glyph), `view` (layouts),
  `icons.rs` (ícones em caminhos vetoriais), `appicon.rs` (ícone da app).
- `src/platform.rs`, `src/winshell.rs` — Win32 (clipboard, tema, tray, instância única, tecla Calculadora).
- `src/i18n/strings.txt` — tabela de traduções (10 idiomas), uma linha por string.
- Stack: winit + softbuffer (sem GPU) + tiny-skia + ab_glyph + accesskit. Não introduzir toolkits de UI (egui/iced/Slint).

## Regras

- Qualquer texto visível novo tem de ter linha em `src/i18n/strings.txt` com todos os idiomas;
  os testes de i18n falham se faltar (incluindo um render em alemão que deteta inglês no ecrã).
- Nada de rede: a app nunca abre ligações (taxas de câmbio vêm embutidas ou de `currency.txt`).
- Não copiar código, ícones ou traduções da Calculadora da Microsoft — só comportamento/aspeto.
- `rustfmt.toml`: `max_width = 140`.
- Partes só-Windows atrás de `cfg(windows)`.

## Git

Repositório local, ramo `master`, sem remote.
