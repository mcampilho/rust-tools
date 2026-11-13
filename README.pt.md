# rust-tools

Pequenas ferramentas portáteis para Windows e Linux, escritas em Rust. Cada uma é
um único executável: sem instalador, sem runtime e sem telemetria. Só o rwall usa
a rede, para descarregar as imagens que põe no ambiente de trabalho.

[English version](README.md)

| Ferramenta | O que é | Windows | Linux |
| --- | --- | --- | --- |
| [rcalc](apps/rcalc/README.pt.md) | Calculadora: padrão, científica, de programador, datas e conversores de unidades | 1,9 MB | 6,6 MB |
| [rpad](apps/rpad/README.pt.md) | Editor de texto e código: separadores, codificações, procura com regex, vários cursores, realce de sintaxe | 4,2 MB | 9,2 MB |
| [rview](apps/rview/README.pt.md) | Visualizador de PDF, Markdown, HTML, texto e CSV, com seleção e cópia | 4,4 MB | 16,6 MB |
| [rsize](apps/rsize/README.pt.md) | Analisador de espaço em disco ao jeito do WizTree: o espaço do volume, pastas e tipos de ficheiro ordenáveis por todas as colunas, treemap com cabeçalhos de pasta, tarte ou anéis, exportação CSV; lê o NTFS diretamente ou o índice do Everything | 1,9 MB | 5,5 MB |
| [rdup](apps/rdup/README.pt.md) | Procurador de ficheiros duplicados: ficheiros com o mesmo conteúdo em vários discos e pastas de uma vez, por tamanho e hashes BLAKE3; lê os discos como o rsize | 1,9 MB | 5,6 MB |
| [rwall](apps/rwall/README.pt.md) | Muda o fundo do ambiente de trabalho: todos os dias, duas ou quatro vezes por dia, todas as semanas, todos os meses ou a cada tantos minutos, com imagens do Bing, da NASA, do Wallhaven, do Picsum ou das suas pastas; no Windows também um fundo dinâmico atrás dos ícones (vídeo, GIF animado ou uma cena desenhada) e widgets no ambiente de trabalho (relógios, calendário, notas, meteorologia, fences, fotos) | 3,7 MB | 6,8 MB |
| [rpack](apps/rpack/README.pt.md) | Arquivador com navegador de ficheiros: cria arquivos 7z, zip e tar; abre, testa e extrai zip, 7z, tar e ficheiros comprimidos com gzip, bzip2, xz, lzma, lzip, zstd, lz4 ou brotli, reconhecidos pelo conteúdo; hashes do CRC-32 ao BLAKE3; palavras-passe, rar, iso e mais vêm a seguir | 4,8 MB | 9,3 MB |
| [runinst](apps/runinst/README.pt.md) | Desinstalador: os programas instalados para todos, para si e (como administrador) para cada outro utilizador, do registo, do Windows Installer, da Loja e do winget numa só lista, o Scoop e o Chocolatey em separadores, com atalhos, ícones e versões mais recentes; desinstala um ou vários seguidos (normal ou sem perguntas, à espera do que cada desinstalador lança); depois encontra e remove os restos (pastas, atalhos, arranque, chaves do registo, serviços, tarefas agendadas; cópia `.reg` e Reciclagem), também de programas removidos antes; separadores para as funcionalidades do Windows (DISM) e o npm, o pip, o Cargo e o vcpkg; em Linux, pacotes dpkg, rpm, Flatpak e Snap | 2,1 MB | 5,9 MB |
| [rclean](apps/rclean/README.pt.md) | Limpador: mostra quanto ocupam os ficheiros temporários do Windows ou do Linux, os relatórios de erros, as miniaturas, as caches de shaders, a Reciclagem e as caches dos browsers (Chrome, Edge, Brave, Vivaldi, Opera, Firefox…), e apaga de vez o que marcar, depois de perguntar; sem tocar no histórico, nos cookies nem nas palavras-passe; caches dos gestores de pacotes, atalhos e registo vêm a seguir | 1,6 MB | 5,5 MB |

Todas desenham a sua própria interface no CPU (winit, softbuffer, tiny-skia e
ab_glyph), sem GPU nem biblioteca de interface. Têm 10 idiomas e seguem o tema
claro ou escuro do sistema. Podem também ficar residentes na área de notificação
com `--tray`.

## Compilar

Requisitos: Windows, Rust 1.88+ com o alvo `x86_64-pc-windows-msvc` e as MSVC
Build Tools (linker e `rc.exe`).

```powershell
./dev.ps1 dist              # compila todas para distribuição, em dist\
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
| rsize | `cargo build --profile release-small -p rsize` | `target\release-small\rsize.exe` |
| rdup | `cargo build --profile release-small -p rdup` | `target\release-small\rdup.exe` |
| rwall | `cargo build --release -p rwall` | `target\release\rwall.exe` |
| rpack | `cargo build --release -p rpack` | `target\release\rpack.exe` |
| runinst | `cargo build --profile release-small -p runinst` | `target\release-small\runinst.exe` |
| rclean | `cargo build --profile release-small -p rclean` | `target\release-small\rclean.exe` |

Há dois perfis porque, num workspace, o `panic` não se pode definir por pacote.
O `release-small` aborta em pânicos, o que torna o rcalc, o rpad, o rsize, o rdup, o runinst e o rclean
cerca de 20% mais pequenos. O rview precisa do `release`: apanha os pânicos da biblioteca de
PDF, para que um PDF malformado não feche o programa. O rwall também o usa, pela
mesma razão, com as imagens que vêm da internet, e o rpack com arquivos danificados. O `./dev.ps1 dist` escolhe
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
[rcalc](apps/rcalc/README.pt.md#linux), [rpad](apps/rpad/README.pt.md#linux),
[rview](apps/rview/README.pt.md#linux), [rsize](apps/rsize/README.pt.md#linux),
[rdup](apps/rdup/README.pt.md#linux), [rwall](apps/rwall/README.pt.md#linux), [rpack](apps/rpack/README.pt.md#linux), [runinst](apps/runinst/README.pt.md#linux) e [rclean](apps/rclean/README.pt.md#linux). Foram
verificados no WSL2, não num ambiente de trabalho Linux real.

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
│  ├─ rclean/
│  ├─ rdup/
│  ├─ rpack/
│  ├─ rpad/
│  ├─ rsize/
│  ├─ runinst/
│  ├─ rview/
│  └─ rwall/
└─ crates/         código partilhado pelas aplicações
   ├─ rt-core/     traduções, definições, árvore de menus
   ├─ rt-gfx/      desenho, disposição de texto, menus desenhados, painel de mensagens
   ├─ rt-scan/     leitura rápida dos discos: Everything, tabela NTFS, pasta a pasta
   ├─ rt-sys/      integração com Windows e Linux
   ├─ rt-ui/       base da janela: janela, tema, menus desenhados, mensagens, controlos
   ├─ rt-syntax/   realce de sintaxe
   └─ rt-build/    ícone e versão no executável de Windows
```

Cada aplicação mantém o seu código, `build.rs`, README e tabela de traduções. Os
[crates partilhados](crates/README.pt.md) explicam o que oferecem e como começar
uma ferramenta nova a partir deles.

## Licença

MIT. Ver [LICENSE](LICENSE).
