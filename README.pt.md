# rust-tools

Pequenas ferramentas portáteis para Windows e Linux, escritas em Rust. Cada uma é
um único executável: sem instalador, sem runtime e sem telemetria.

[English version](README.md)

| Ferramenta | O que é | Windows | Linux |
| --- | --- | --- | --- |
| [rcalc](apps/rcalc/README.pt.md) | Calculadora: padrão, científica, de programador, datas e conversores de unidades | 1,9 MB | 6,6 MB |
| [rpad](apps/rpad/README.pt.md) | Editor de texto e código: separadores, codificações, procura com regex, vários cursores, realce de sintaxe | 4,2 MB | 9,2 MB |
| [rview](apps/rview/README.pt.md) | Visualizador de PDF, Markdown, HTML, texto e CSV, com seleção e cópia | 4,4 MB | 16,6 MB |

As três desenham a sua própria interface no CPU (winit, softbuffer, tiny-skia e
ab_glyph), sem GPU nem biblioteca de interface. Têm 10 idiomas, seguem o tema
claro ou escuro do sistema e podem ficar residentes na área de notificação com
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

## Transferir

Cada ferramenta tem as suas [releases](https://github.com/mcampilho/rust-tools/releases),
num zip com o `.exe` para Windows 10/11 de 64 bits, e num `tar.gz` para Linux de
64 bits. Basta extrair e executar: não há nada para instalar.

Para publicar uma versão, define-a em `apps/<ferramenta>/Cargo.toml` e envia uma tag
como `rview-v1.1.0`. O [workflow de release](.github/workflows/release.yml) testa e
compila essa ferramenta e publica-a com a soma SHA-256 e a proveniência da compilação.

## Linux

Os pacotes para Linux precisam da glibc 2.35 ou mais recente (2.34 no rcalc e no
rpad) e funcionam em X11 e Wayland. O que muda face ao Windows está em cada README:
[rcalc](apps/rcalc/README.pt.md#linux), [rpad](apps/rpad/README.pt.md#linux) e
[rview](apps/rview/README.pt.md#linux). Foram verificados no WSL2, não num ambiente de
trabalho Linux real.

No WSLg, a barra de título que estas ferramentas desenham em Wayland pode fazer cair
o compositor, e elas podem fechar com o erro "Broken pipe". Executá-las com
`WAYLAND_DISPLAY=` usa o X11.

## Organização

```text
rust-tools/
├─ .github/        workflow de release
├─ Cargo.toml      membros do workspace e perfis de compilação
├─ dev.ps1         cargo com o ambiente das Build Tools; dist
├─ apps/
│  ├─ rcalc/
│  ├─ rpad/
│  └─ rview/
└─ crates/         código partilhado pelas aplicações
   ├─ rt-core/     traduções, definições, árvore de menus
   ├─ rt-gfx/      desenho, disposição de texto, menus desenhados
   ├─ rt-sys/      integração com Windows e Linux
   ├─ rt-syntax/   realce de sintaxe
   └─ rt-build/    ícone e versão no executável de Windows
```

Cada aplicação mantém o seu código, `build.rs`, README e tabela de traduções. Os
[crates partilhados](crates/README.pt.md) explicam o que oferecem e como começar
uma ferramenta nova a partir deles.

## Licença

MIT. Ver [LICENSE](LICENSE).
