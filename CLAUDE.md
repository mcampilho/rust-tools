# rust-tools — ferramentas portáteis em Rust para Windows

Monorepo (workspace Cargo) com nove aplicações, cada uma um único `.exe`, e os crates que partilham
(`crates/`):

- `apps/rcalc` — calculadora.
- `apps/rpad` — editor de texto e código.
- `apps/rview` — visualizador de documentos.
- `apps/rsize` — analisador de espaço em disco.
- `apps/rdup` — procurador de ficheiros duplicados (lê os discos com o `rt-scan`, como o rsize).
- `apps/rwall` — muda o wallpaper a partir da área de notificação (Bing, NASA, Wallhaven, Picsum, pastas locais) e,
  no Windows, põe um fundo dinâmico atrás dos ícones (vídeo, GIF/WebP animado, cenas próprias) e widgets no ambiente de
  trabalho (relógios, calendário, notas, anotações, meteorologia da Open-Meteo, fences, fotos) e barra de tarefas
  transparente por ecrã.
- `apps/rpack` — arquivador com navegador de ficheiros (em curso, por fases): cria 7z, zip e tar; abre, testa e
  extrai zip, 7z, tar e ficheiros comprimidos; hashes.
- `apps/rinst` — desinstalador: lista os programas de todos os utilizadores e os atalhos, desinstala e remove os
  restos; separadores para Scoop, Chocolatey, DISM, npm, pip, Cargo e vcpkg; em Linux, dpkg, rpm, Flatpak e Snap.
- `apps/rclean` — limpador: temporários do sistema, caches dos browsers e dos gestores de pacotes, apagados de vez
  depois de confirmar; atalhos inválidos (para a Reciclagem); entradas inválidas do registo (cópia `.reg` antes).
- `apps/rcare` — manutenção do Windows numa só janela, só Windows (desde 2026-11-22, em curso): barra lateral
  recolhível que escolhe a página, e as páginas são as das apps pequenas (rclean, rdup, rsize, rinst), partilhadas por
  código (`rclean::page::CleanPage`, `rdup::page::DupPage`, `rsize::page::SizePage`, `rinst::page::InstPage`, `rt_ui::page::Effects`). Plano e passagem de testemunho em `docs/rcare.pt.md`; perfil `release`; ver o
  `apps/rcare/CLAUDE.md`. As apps pequenas ficam.
- `apps/rscoop-cli` — linha de comandos, só Windows (desde 2026-11-17): verifica uma instalação do Scoop e corrige o que
  aponta para uma raiz antiga (`--fix`); port do `scoop-check` em Python, com cache SQLite (`rusqlite` bundled).
  Não tem janela nem `--tray`; textos só em en/pt/es.
- `apps/rsize-cli` — linha de comandos, Windows e Linux (desde 2026-11-18): o `du` do rsize (`-h`, `-s`, `-d`, `-a`,
  `-c`, `--apparent-size`, mais `--top`, `--sort`, `--json`, `--csv`, `--no-index`); lê com o `rt-scan` (Everything →
  NTFS → pasta a pasta). Textos em en/pt/es pelo `rt-cli`.
- `apps/rdup-cli` — linha de comandos, Windows e Linux (desde 2026-11-18): o `fdupes` do rdup (grupos de cópias, `-S`,
  `-m`, `-1`, `-f`, filtros, `--json`, `--csv`, cache de hashes partilhada com o rdup). Só lista: não apaga nada.
  Usa o motor do crate `rt-dups`.

Cada aplicação tem o seu `CLAUDE.md` com arquitetura e regras próprias.

## Comandos

```powershell
./dev.ps1 dist                               # todas, com o perfil certo, em dist\
./dev.ps1 test --workspace
./dev.ps1 clippy --workspace --all-targets
./release.ps1 -DryRun                        # releases: o patch sobe, tags de 3 em 3 (ver o script)
```

O `dev.ps1` carrega o MSVC portátil (`RUST_TOOLS_MSVC_ENV`, ou `\Portable\Tools\dev\msvc\devcmd.ps1`
em `E:` ou `D:`). Come o `--`: passa `'--'` entre aspas.

## Regras comuns

- "Pronto" = sem avisos, clippy limpo, testes a passar.
- Perfis só na raiz do `Cargo.toml`. rcalc, rpad, rsize, rdup, rinst e rclean usam `release-small` (`panic = "abort"`);
  o rview, o rwall e o rpack usam sempre `release`, porque precisam de `catch_unwind`.
  O perfil de cada aplicação está num só sítio: `[package.metadata.rust-tools] profile = "…"` no seu `Cargo.toml`
  (lido pelo `dev.ps1` e pelo `release.yml`). Uma aplicação nova tem de o declarar.
- Compilar para distribuição com `./dev.ps1 dist`, que escolhe o perfil de cada aplicação.
- Versão do Rust fixada em `rust-toolchain.toml` (o CI e os releases leem-na; mudar de propósito, num commit à parte).
  O `rust-version` dos `Cargo.toml` é o mínimo, compilado pelo job `msrv`.
- Máx. 3 tags por push: o GitHub não dispara os workflows de mais do que três de uma vez (lançar com
  `gh workflow run release.yml -f tag=<tag>`). Reconstruir uma tag anterior a 2026-11-18 não funciona (sem o perfil
  no `Cargo.toml` nem `rust-toolchain.toml`).
- Uma aplicação só de Windows declara `platforms = "windows"` em `[package.metadata.rust-tools]` (o rscoop-cli) e o
  `release.yml` não faz o pacote Linux. As `-cli` não têm `build.rs`, por isso não há recurso de versão no `.exe`
  (o passo "Check the executable" salta-as) e as notas do release dizem que se corre num terminal.
- `README.md` e `README.pt.md` são atualizados em conjunto, na raiz e em cada aplicação.
- Textos visíveis ficam no `strings.txt` de cada aplicação, nunca no código de desenho.
- Sem rede, sem telemetria, sem biblioteca de interface. Exceção aprovada em 2026-11-11: o rwall descarrega
  imagens (só GET às fontes que o utilizador liga; WinHTTP no Windows, `curl` no Linux) e, desde 2026-11-12, a
  meteorologia da Open-Meteo para o widget de meteorologia. E, desde 2026-11-17, "Procurar atualizações…" em todas
  (pedido do utilizador): só quando o utilizador escolhe, GET à API de releases do GitHub e ao pacote e ao
  `SHA256SUMS.txt` do release (`rt_sys::update`, `rt_sys::net` — o cliente HTTPS que era do rwall —,
  `rt_ui::update` para os diálogos; instalado pelo Scoop → `scoop update <app>`). E, desde 2026-11-24 (pedido do
  utilizador), o rview descarrega as imagens `http(s)` de um documento, só se ele ligar Ver › Mostrar imagens da web
  (desligado por omissão; GET simples por `rt_sys::net`, nunca a `localhost` nem a redes privadas). Nenhuma outra.
- Bucket do Scoop na pasta `bucket/` (desde 2026-11-17): um manifesto por app com release (rpack, rinst e rclean
  entram com o primeiro), `checkver` pelo `tag_name` dos releases e `autoupdate` com a soma do `SHA256SUMS.txt`;
  depois de um release, `checkver.ps1 -Dir bucket -Update`.
- Aplicações só de linha de comandos (sem janela) levam o sufixo `-cli` na pasta, no crate e no `.exe`
  (`rscoop-cli`, `rsize-cli`, `rdup-cli`). Partilham o crate `rt-cli` (textos
  en/pt/es, língua, tamanhos como o `du`). Têm texto em en/pt/es,
  não têm modo residente, "Iniciar com o Windows" nem "Procurar atualizações…", e no bucket do Scoop ficam sem
  `shortcuts`. As pastas de dados mantêm o nome antigo (`%LOCALAPPDATA%\rscoop`) para não perder cache nem cópias.
- `rustfmt.toml`: `max_width = 140` e `use_small_heuristics = "Max"`. O código segue o rustfmt (job `fmt` do CI):
  `cargo fmt --all` antes de cada commit. Neste PC o `cargo-fmt` do rustup (sem toolchain) vem primeiro no PATH:
  pôr `E:\Portable\scoop\apps\rust\current\bin` à frente. A reformatação de 2026-11-18 está em
  `.git-blame-ignore-revs` (`git config blame.ignoreRevsFile .git-blame-ignore-revs`).

## Verificação automática

`.github/workflows/ci.yml` corre `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace`, com `--locked`, em Windows e Linux (ubuntu-22.04, o mesmo das releases), a cada push ao `master` e em cada pull request. Um aviso do clippy falha o CI: é a regra `Pronto` desta página. Além disso: `msrv` (`cargo check` com o maior `rust-version` declarado) e `audit` (`cargo deny check advisories`, também todas as segundas-feiras). O Dependabot (`.github/dependabot.yml`) propõe, por semana, crates e actions mais novos. A instalação do Rust, da cache e do `rc.exe` está em `.github/actions/setup-rust`.

## Releases

`.github/workflows/release.yml` corre com tags `<app>-v<versão>` (ex.: `rview-v1.1.0`). A versão da
tag tem de ser igual à de `apps/<app>/Cargo.toml`. Todas são publicadas para Windows e Linux (o rsize desde a
0.1.0, em 2026-11-07, e a 0.1.1 em 2026-11-08 com a correção do "Mostrar no Explorador" com espaços; o rdup desde a
0.1.0, em 2026-11-08). Em 2026-11-23 saíram o rcare 0.1.0 e o rtsvc 0.1.0 (os primeiros; manifestos `bucket/rcare.json` e `bucket/rtsvc.json`; o rtsvc pelo `release.ps1`). Em 2026-11-09 saíram rcalc 1.2.1, rpad 1.2.0, rview 1.2.1, rsize 0.1.2 e rdup 0.1.1 (ajuda na
consola ou num diálogo, modo residente e menus do Explorador).

## Testar em Linux

O WSL corre X11 e Wayland, mas não simula tudo: o winit em X11 precisa da `libxkbcommon-x11`, e o ícone
de notificação precisa de um anfitrião StatusNotifierItem, que o WSL não tem. Em Wayland, a barra de título
(`wayland-csd-adwaita`) faz o `weston` do WSLg cair com `signal 11` em cerca de 1 arranque em 4, e o cliente
acaba com `Broken pipe`: ver `/mnt/wslg/stderr.log`. Não é erro das aplicações; testar com `WAYLAND_DISPLAY=`.

No Ubuntu do WSL: compilar com `CARGO_TARGET_DIR=$HOME/rt-target` (fora de `/mnt`, e o `/tmp` perde-se entre
chamadas). A `libxkbcommon-x11` e a `libxcb-xkb1` estão extraídas sem sudo em `~/xkb` (`apt-get download` e
`dpkg -x`, mais a ligação `libxkbcommon-x11.so` → `.so.0`): correr com
`LD_LIBRARY_PATH=$HOME/xkb/usr/lib/x86_64-linux-gnu`.

## Modo residente e arranque com a sessão

As nove aplicações ficam residentes com `--tray` (ícone na área de notificação; outro arranque mostra a janela por
`hand_off`, `--new-window` não). Desde 2026-11-14 cada uma tem "Iniciar com o Windows" (Linux: "Iniciar ao entrar na
sessão"), por `rt_sys::autostart` (`HKCU\…\Run\<app>` ou `~/.config/autostart/<app>.desktop`, sempre `"<exe>" --tray`):
no menu Ficheiro (rpad: Ficheiro › Integração com o Windows; em Linux Ferramentas › Preferências), nas definições
(rcalc) ou ao lado dos separadores (rwall). As teclas de acesso desse item mudam por aplicação e por sistema para não
colidirem (testes em Windows e no WSL).

## Crates partilhados

O código comum está em `crates/` (ver `crates/README.pt.md`): `rt-core` (traduções, definições, árvore de
menus), `rt-gfx` (desenho, texto, menus desenhados), `rt-sys` (Windows e Linux), `rt-scan` (leitura dos
discos: Everything, MFT do NTFS, pasta a pasta; a árvore `model`, `volumes` e `format`; usado pelo rsize e
pelo rdup; o rinst usa a pesquisa livre `everything::search` para os restos), `rt-ui` (base da janela: criar a janela, tema, menus desenhados, painel de mensagem, modo residente em Linux, menu do Explorador, painel "Aplicação predefinida por tipo de ficheiro" `assoc` do rview e do rpad, controlos das páginas de definições), `rt-syntax` (realce), `rt-build` (recursos do `.exe`) e `rt-dups` (a procura de duplicados do rdup e do rdup-cli: `search`, `cache`, `folders`, `filter`, `testing`) e `rt-cli` (o que as aplicações `-cli` partilham: `Texts` en/pt/es, `language`, `table_problems`, `human`). As aplicações guardam módulos pequenos com o mesmo nome de antes
(`ui/gfx.rs`, `storage.rs`, `win/mod.rs`, `linux/mod.rs`...) que reexportam dos crates e lhes dão a
identidade da aplicação. Uma correção num destes assuntos faz-se no crate, uma vez.

- Impressão (2026-11-24, só Windows; rview e rpad): `rt_sys::win::print` (o diálogo `PrintDlgW`, `Printer` com
  `page_size`/`render_dpi` até 300 ppp e `print` que manda páginas em bitmap por `StretchDIBits`; `named` abre uma
  impressora sem diálogo, para testes) e `rt_ui::print` (`TextJob`, `plan`/`geometry`/`draw_page` para texto
  colorido, `frame`/`draw_frame` para o cabeçalho e o rodapé, `start`/`start_text` que mostram o diálogo e imprimem
  numa thread, `highlighted_lines` pelo `rt-syntax`). As outras aplicações não imprimem. Em Linux ainda não há.
- Os crates não dependem das aplicações. Textos visíveis dos crates vão para `crates/rt-core/src/strings.txt`
  (procurado depois da tabela da aplicação).
- Linux inclui sempre X11 e Wayland.
- Uma ferramenta nova começa pelos passos de `crates/README.md`.

## Serviço com privilégios (2026-11-20, em curso)

`apps/rtsvc` (serviço do Windows `RustToolsSvc`; fora da regra do sufixo `-cli`: é um serviço, com comandos
`install`, `uninstall`, `status`, `console`, `run`) e o crate `crates/rt-svc` (protocolo, cliente, pipe, verificação
de pastas). Qualquer utilizador lê o sistema todo através dele; só um cliente elevado altera (decidido no token que
chega pelo pipe). O desenho, o protocolo e o modelo de ameaças estão em `docs/service.md` (e `.pt.md`); as
atualizações continuam em cada aplicação. Feito: o esqueleto (pipe com DACL próprio, níveis User/Elevated, verificação do
servidor pelo cliente, `install`/`uninstall`/`status`/`console`, verificação de pasta segura) e `ScanVolume` (o
`rt_scan::scan_with` usa o serviço antes de ler o NTFS por si, se a aplicação não está elevada; `rt_scan::wire`
leva a árvore). Fotografias (
tsvc/src/snapshots.rs: Store, 	ick do agendador, ficheiros .snap numa pasta só para administradores; kinds.rs; idle.rs: parado = CPU < 33 % e na corrente, porque a sessão 0 não vê o teclado; watch.rs: o registo dos programas marca a fotografia como suja): programs (
t_inst::snapshot, filtrada pelo nível: um utilizador vê a máquina e o que é seu) e clean (
t_clean::snapshot: só tamanhos dos alvos dmin). O rinst usa-a na primeira leitura se não está elevado; o rclean mostra tamanhos nas linhas que precisam de administrador (Item::known). Sem indicador na janela de que veio do serviço, nem botão para a ignorar (as leituras seguintes são ao vivo). Os motores do rclean e do rinst passaram para crates/rt-clean e crates/rt-inst. Tarefas (`jobs.rs`/`work.rs`, só `Elevated`: `Clean` por id de alvos `admin` de ficheiros, `UninstallMsi` por código de produto da máquina; cliente em `rt_svc::client::Client::submit_job`), sem uso ainda nas janelas do rclean e do rinst. Endurecimento: `crates/rt-svc/tests/fuzz.rs` (descodificadores com bytes aleatórios e mutações); por fazer: restringir os privilégios do serviço e experimentar `install`/`uninstall`/o serviço como SYSTEM numa máquina elevada. O `apps/rtsvc/CLAUDE.md` tem os módulos.
- Testes do pipe em `crates/rt-svc/tests/pipe.rs` (servidor e cliente no mesmo processo, pipes de nome único);
  `rtsvc console` corre o serviço como o utilizador no pipe de desenvolvimento. `install`/`uninstall`/serviço real
  só se experimentam elevados (não foram corridos em máquina nenhuma ainda).
- `Everyone:(F)` na raiz `C:\` (como neste PC) faz a verificação recusar qualquer pasta em `C:`, `Program Files`
  incluída: ver `docs/service.md` §9.
- Neste PC compilar com `CARGO_INCREMENTAL=0` (o disco E: encheu com 36 GB de `target\debug\incremental`).

## Git

Ramo `master`. Commit e push só com aprovação explícita.
