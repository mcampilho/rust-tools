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
| [rwall](apps/rwall/README.pt.md) | Muda o fundo do ambiente de trabalho: todos os dias, duas ou quatro vezes por dia, todas as semanas, todos os meses ou a cada tantos minutos, com imagens do Bing, da NASA, do Wallhaven, do Picsum ou das suas pastas; no Windows também um fundo dinâmico atrás dos ícones (vídeo, GIF animado ou uma cena desenhada) e widgets no ambiente de trabalho (relógios, calendário, notas, meteorologia, fences, fotos), e uma barra de tarefas transparente | 3,7 MB | 6,8 MB |
| [rpack](apps/rpack/README.pt.md) | Arquivador com navegador de ficheiros: cria arquivos 7z, zip e tar; abre, testa e extrai zip, 7z, tar e ficheiros comprimidos com gzip, bzip2, xz, lzma, lzip, zstd, lz4 ou brotli, reconhecidos pelo conteúdo; hashes do CRC-32 ao BLAKE3; palavras-passe, rar, iso e mais vêm a seguir | 4,8 MB | 9,3 MB |
| [rinst](apps/rinst/README.pt.md) | Desinstalador: os programas instalados para todos, para si e (como administrador) para cada outro utilizador, do registo, do Windows Installer, da Loja e do winget numa só lista, o Scoop e o Chocolatey em separadores, com atalhos, ícones e versões mais recentes; desinstala um ou vários seguidos (normal ou sem perguntas, à espera do que cada desinstalador lança); depois encontra e remove os restos (pastas, atalhos, arranque, chaves do registo, serviços, tarefas agendadas; cópia `.reg` e Reciclagem), também de programas removidos antes; separadores para as funcionalidades do Windows (DISM) e o npm, o pip, o Cargo e o vcpkg; em Linux, pacotes dpkg, rpm, Flatpak e Snap | 2,1 MB | 5,9 MB |
| [rclean](apps/rclean/README.pt.md) | Limpador: mostra quanto ocupam os ficheiros temporários do Windows ou do Linux, os relatórios de erros, as miniaturas, as caches de shaders, a Reciclagem e as caches dos browsers (Chrome, Edge, Brave, Vivaldi, Opera, Firefox…) e as dos gestores de pacotes (Scoop, pip, npm, Cargo, vcpkg, apt… pelos seus próprios comandos), e apaga de vez o que marcar, depois de perguntar; os atalhos inválidos (para a Reciclagem) e as entradas inválidas do registo (guardadas antes num `.reg`); sem tocar no histórico, nos cookies nem nas palavras-passe | 1,9 MB | 5,6 MB |
| [rlaunch](apps/rlaunch/README.pt.md) | Menu de aplicações na área de notificação: os programas do Scoop, do menu Iniciar e das tuas pastas por categorias, com procura e atalho de teclado; lê pastas PortableApps; temas; funciona numa pen USB | 1,9 MB | 7,6 MB |
| [rcare](apps/rcare/README.pt.md) | Manutenção do Windows numa só janela (só Windows, em curso, ver [o plano](docs/rcare.pt.md)): uma barra lateral recolhível escolhe a página, e as páginas são as das ferramentas pequenas. Por agora a System Maintenance (as verificações de uma vez), a Custom Clean (lixo, duplicados e ficheiros grandes de uma vez), o Speed Up, o Startup Optimizer, o Program Deactivator, as Extensions, o Registry Cleaner, o Protect System, o Internet Booster, o Privacy Sweep, o System Information, o Reparar, o Agendador, as Ferramentas de ficheiros (pastas vazias, File Shredder), o Context Menu Manager, os Processos (com Auto Shutdown) e as páginas do rclean (limpador), do rdup (duplicados), do rsize (espaço em disco) e do rinst (desinstalador); depois as ferramentas de sistema | – | – |
| [rscoop-cli](apps/rscoop-cli/README.pt.md) | Um [Scoop](https://scoop.sh) mais rápido na linha de comandos (faz ele próprio todos os comandos do Scoop, instalar e remover aplicações incluídos, com a extração e os scripts dos manifestos no código do próprio Scoop; os comandos entregues ao Scoop são escolha de cada um) e verificador da instalação: shims, ligações `current`, dados persistentes, instalações falhadas, atalhos do menu Iniciar, registo, variáveis e ficheiros de configuração estragados ou que ainda apontam para uma raiz antiga do Scoop (depois de mudar o Scoop de disco); o `--fix` corrige-os com cópia de segurança; uma cache SQLite dos ficheiros já lidos torna as corridas seguintes cerca de 15 vezes mais rápidas | 3,5 MB | – |
| [rsize-cli](apps/rsize-cli/README.pt.md) | Uso do disco na linha de comandos, como o `du`: quanto espaço ocupam os ficheiros e as pastas de um caminho (no disco ou pelo comprimento), em bytes, KiB, MiB ou `1.5K`, com profundidade, totais, as N maiores, JSON e CSV; lê o NTFS diretamente ou o índice do Everything como o rsize | 0,5 MB | – |
| [rdup-cli](apps/rdup-cli/README.pt.md) | Procura de duplicados na linha de comandos, como o `fdupes`: ficheiros com o mesmo conteúdo nas pastas e discos indicados em conjunto, listados em grupos (primeiro o maior desperdício) ou em resumo, JSON ou CSV, com filtros por tamanho, data, extensão e nome e a cache de hashes do rdup; nunca apaga nada | 0,8 MB | – |

Todas desenham a sua própria interface no CPU (winit, softbuffer, tiny-skia e
ab_glyph), sem GPU nem biblioteca de interface. Têm 10 idiomas e seguem o tema
claro ou escuro do sistema. Podem também ficar residentes na área de notificação
com `--tray`, e cada uma pode arrancar assim com o Windows (ou ao entrar na sessão,
em Linux) a partir do seu menu ou das suas definições. O rscoop-cli, o rsize-cli e o rdup-cli são as exceções:
ferramentas de linha de comandos (o sufixo `-cli`), em inglês, português e espanhol.

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
| rinst | `cargo build --profile release-small -p rinst` | `target\release-small\rinst.exe` |
| rclean | `cargo build --profile release-small -p rclean` | `target\release-small\rclean.exe` |
| rlaunch | `cargo build --profile release-small -p rlaunch` | `target\release-small\rlaunch.exe` |
| rcare | `cargo build --release -p rcare` | `target\release\rcare.exe` |
| rscoop-cli | `cargo build --profile release-small -p rscoop-cli` | `target\release-small\rscoop-cli.exe` |
| rsize-cli | `cargo build --profile release-small -p rsize-cli` | `target\release-small\rsize-cli.exe` |
| rdup-cli | `cargo build --profile release-small -p rdup-cli` | `target\release-small\rdup-cli.exe` |

Há dois perfis porque, num workspace, o `panic` não se pode definir por pacote.
O `release-small` aborta em pânicos, o que torna o rcalc, o rpad, o rsize, o rdup, o rinst, o rclean, o rscoop-cli, o rsize-cli e o rdup-cli
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

O `./release.ps1` fá-lo para várias ferramentas de uma vez: sobe o patch de cada uma
(`x.y.z` → `x.y.z+1`; uma versão já subida à mão, ou um primeiro release, fica como
está), faz um commit, envia o `master` e depois as tags de três em três (o GitHub não
arranca os workflows de mais de três tags de um só envio). Por omissão escolhe as
ferramentas alteradas desde o último release; `-Apps rcare,rpad` ou `-All` escolhem,
`-DryRun` só mostra o plano, `-Wait` espera pelos releases de cada grupo. Pergunta
antes de mudar alguma coisa. Depois, `./dev.ps1 dist` atualiza as compilações locais e
`checkver.ps1 -Dir bucket -Update` o bucket do Scoop.

### Com o Scoop

O repositório é também um bucket do [Scoop](https://scoop.sh) (a pasta `bucket`):

```powershell
scoop bucket add rust-tools https://github.com/mcampilho/rust-tools
scoop install rpad
```

Os manifestos sabem onde está cada versão nova e tiram a soma do `SHA256SUMS.txt`
do release. Depois de um release, `checkver.ps1 -Dir bucket -Update` (da pasta `bin`
do próprio Scoop) atualiza-os. Todas as ferramentas estão no bucket; as de linha de
comandos (`rscoop-cli`, `rsize-cli`, `rdup-cli`) ficam no `PATH` e não têm atalho no
menu Iniciar.

### Atualizações

Cada ferramenta pode procurar uma versão mais recente de si própria, só quando
pedes: **Ajuda › Procurar atualizações…** (rcalc: Definições › Acerca de; rwall: o
menu do ícone da área de notificação). Pergunta ao GitHub pelos releases da
ferramenta. Se houver uma versão mais recente, diz qual é e pergunta antes de
descarregar. A transferência é conferida com a soma SHA-256 publicada e posta no
lugar do programa, e escolhes quando iniciar a versão nova. Uma ferramenta
instalada pelo Scoop propõe antes `scoop update <ferramenta>`, para o Scoop saber
que versão está instalada. Mais nada vai à rede.

## Serviço (opcional, Windows)

O [rtsvc](apps/rtsvc/README.pt.md) é um serviço do Windows que, instalado numa linha de comandos elevada (`rtsvc install`), deixa o rsize, o rdup, o rinst e o rclean mostrar o que precisa de administrador (a tabela NTFS, os programas de todos os utilizadores, o tamanho dos ficheiros do próprio Windows) sem serem abertos como administrador, e mantém parte disso pronta em segundo plano. Alterar coisas continua a precisar da ferramenta aberta como administrador. As ferramentas funcionam como antes sem ele. [Desenho](docs/service.pt.md) (ainda não corrido como serviço instalado).

## Linux

Os pacotes para Linux precisam da glibc 2.35 ou mais recente (2.34 no rcalc e no
rpad) e funcionam em X11 e Wayland. O que muda face ao Windows está em cada README:
[rcalc](apps/rcalc/README.pt.md#linux), [rpad](apps/rpad/README.pt.md#linux),
[rview](apps/rview/README.pt.md#linux), [rsize](apps/rsize/README.pt.md#linux),
[rdup](apps/rdup/README.pt.md#linux), [rwall](apps/rwall/README.pt.md#linux), [rpack](apps/rpack/README.pt.md#linux), [rinst](apps/rinst/README.pt.md#linux) e [rclean](apps/rclean/README.pt.md#linux). Foram
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
│  ├─ rcare/
│  ├─ rclean/
│  ├─ rlaunch/
│  ├─ rdup/
│  ├─ rdup-cli/
│  ├─ rpack/
│  ├─ rpad/
│  ├─ rsize/
│  ├─ rsize-cli/
│  ├─ rinst/
│  ├─ rscoop-cli/
│  ├─ rtsvc/
│  ├─ rview/
│  └─ rwall/
└─ crates/         código partilhado pelas aplicações
   ├─ rt-cli/      o que as aplicações de linha de comandos partilham: textos en/pt/es, tamanhos como o `du`
   ├─ rt-core/     traduções, definições, árvore de menus
   ├─ rt-clean/    o que o limpador limpa: os alvos, medi-los e removê-los
   ├─ rt-dups/     procura de ficheiros e pastas duplicados: a pesquisa, a cache de hashes, os filtros
   ├─ rt-inst/     os programas instalados: o registo, o Windows Installer, a Loja, dpkg e rpm
   ├─ rt-svc/      o serviço com privilégios: protocolo, pipe, cliente, a verificação da pasta dele
   ├─ rt-gfx/      desenho, disposição de texto, menus desenhados, painel de mensagens
   ├─ rt-ops/      apagar e mover em vários fios, os que o disco aguenta
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
