# rust-tools

Pequenas ferramentas portáteis para Windows, escritas em Rust. Cada uma é um
único `.exe`: sem instalador, sem runtime, sem DLL além das que vêm com o
Windows e sem telemetria.

[English version](README.md)

| Ferramenta | O que é | Tamanho |
| --- | --- | --- |
| [rcalc](apps/rcalc/README.pt.md) | Calculadora: padrão, científica, de programador, datas e conversores de unidades | 1,9 MB |
| [rpad](apps/rpad/README.pt.md) | Editor de texto e código: separadores, codificações, procura com regex, vários cursores, realce de sintaxe | 4,2 MB |
| [rview](apps/rview/README.pt.md) | Visualizador de PDF, Markdown, HTML, texto e CSV, com seleção e cópia | 4,4 MB |

As três desenham a sua própria interface no CPU (winit, softbuffer, tiny-skia e
ab_glyph), sem GPU nem biblioteca de interface. Têm 10 idiomas, seguem o tema
claro ou escuro do Windows e podem ficar residentes na área de notificação com
`--tray`.

## Compilar

Requisitos: Windows, Rust 1.88+ com o alvo `x86_64-pc-windows-msvc` e as MSVC
Build Tools (linker e `rc.exe`).

```powershell
./dev.ps1 dist              # compila as três para distribuição, em dist\
./dev.ps1 test --workspace
./dev.ps1 clippy --workspace --all-targets
```

O `dev.ps1` corre o cargo depois de carregar um ambiente portátil das Build
Tools, se existir: o de `$env:RUST_TOOLS_MSVC_ENV`, ou o de
`\Portable\Tools\dev\msvc` nos discos `E:` ou `D:`. Com as Build Tools já no
`PATH`, também funciona só com o cargo:

| Ferramenta | Comando | Resultado |
| --- | --- | --- |
| rcalc | `cargo build --profile release-small -p rcalc` | `target\release-small\rcalc.exe` |
| rpad | `cargo build --profile release-small -p rpad` | `target\release-small\rpad.exe` |
| rview | `cargo build --release -p rview` | `target\release\rview.exe` |

Há dois perfis porque, num workspace, o `panic` não se pode definir por pacote.
O `release-small` aborta em pânicos, o que torna o rcalc e o rpad cerca de 20%
mais pequenos. O rview precisa do `release`: apanha os pânicos da biblioteca de
PDF, para que um PDF malformado não feche o programa. O `./dev.ps1 dist` escolhe
o perfil certo para cada ferramenta.

## Organização

```text
rust-tools/
├─ Cargo.toml      membros do workspace e perfis de compilação
├─ dev.ps1         cargo com o ambiente das Build Tools; dist
└─ apps/
   ├─ rcalc/
   ├─ rpad/
   └─ rview/
```

Cada aplicação mantém o seu código, `build.rs`, README e tabela de traduções.

## Licença

MIT. Ver [LICENSE](LICENSE).
