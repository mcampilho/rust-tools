# rclean — limpador portátil em Rust

Mostra quanto ocupam os ficheiros temporários do sistema e as caches dos browsers e apaga, de vez, o que o
utilizador marca e confirma. Um único executável (1,8 MB em Windows, 5,5 MB em Linux). Pedido do utilizador
(2026-11-13): limpar temporários do Windows e dos browsers, caches dos gestores de pacotes (Scoop, Chocolatey, pip,
npm, Cargo, vcpkg…), entradas inválidas do registo e atalhos inválidos. Decidido nesse dia: temporários e caches
apagados de vez (não para a Reciclagem); caches dos gestores pelos comandos de cada um (pastas só onde não há
comando); começar pelos temporários e browsers. Funcionalidades no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rclean
./dev.ps1 test -p rclean         # 18 testes
./dev.ps1 clippy -p rclean --all-targets
./dev.ps1 test -p rclean --lib screenshots '--' --ignored   # apps/rclean/target/screens/*.png (here.png analisa este PC a sério)
```

Em Linux (WSL, `CARGO_TARGET_DIR=$HOME/rt-target`) os mesmos com `cargo`.

## Arquitetura (`src/`)

- `targets.rs` — o que se limpa: `Target` (`id` estável para as definições, `Group::System`/`Browser(nome)`, `name` e
  `about` são chaves de tradução, `places`, `Kind::Files`/`RecycleBin`, `admin`, `default_on`, `process`: nomes dos
  processos do programa). `Place` = pasta + `Filter` (`All`, `OlderThanHours(24)` nas pastas temporárias,
  `Prefix("thumbcache_")`, `Named("MEMORY.DMP")`; `Prefix`/`Named` nunca descem: `deep()`). `Env` (pastas do
  ambiente, falsas nos testes). `windows(env)`, `linux(env)`, `browsers(env, windows)`: Chromium (`CHROMIUM`: Chrome,
  Edge, Brave, Vivaldi, Opera — perfil na raiz e cache em `%LOCALAPPDATA%` —, Chromium; caches de cada perfil
  `CHROMIUM_PROFILE_CACHES` e da raiz `CHROMIUM_ROOT_CACHES`) e Gecko (`GECKO`: Firefox — em Linux também Snap e
  Flatpak —, LibreWolf, Waterfox, Floorp; `cache2`, `startupCache`, `jumpListCache`). Um browser só entra com cache.
  Nunca histórico, cookies, palavras-passe, sessões.
- `clean.rs` — `analyse` (lista ficheiros e tamanhos; não segue nem apaga ligações/junções; em Linux só os do
  utilizador, `my_uid` por `/proc/self`; pastas interiores para remover se vazias, das mais fundas para cima, só se
  também antigas no filtro de horas) e `clean` (apaga de vez, só-leitura também; o que falha conta como `failed`, um
  ficheiro já desaparecido não). Reciclagem: `SHQueryRecycleBinW`/`SHEmptyRecycleBinW` (todas as unidades).
- `managers.rs` — fase 2 (2026-11-13): um grupo por gestor (`Group::Tool`), só se existir (`which`, sem os falsos do
  `WindowsApps`; `real_file` lê os `.shim` do Scoop). `Kind::Command`: as pastas só medem (contadas em `items`, sem
  lista, sem filtro de dono, sem "pastas por ler"), limpa o `Cmd` do gestor (`run`: sem janela; em Linux `root`
  passa por `pkexec env K=V …`) e volta a medir para saber o que saiu; erro → `Cleaned::error` (fim da saída) no
  resumo. `Locate::Ask` pergunta a pasta durante a análise (`npm config get cache` + `_cacache`, `yarn cache dir`,
  `pnpm store path`, `go env GOCACHE`; o npm do Scoop guarda-a em `persist`). Scoop (`SCOOP` ou `~\scoop`): `cache rm *`
  e versões antigas (`scoop_old_versions`: pastas de versão que não são o destino da junção `current`; `cleanup *`,
  desmarcadas), Chocolatey (`admin`), pip (`python -m pip cache purge`), npm, Yarn, pnpm (`store prune`, desmarcado),
  Go, NuGet (`http-cache`), Cargo e vcpkg (`Kind::Files`, sem comando). Linux: apt, dnf, zypper, pacman (`root`),
  Flatpak `--unused` (`measure: false`: tamanho desconhecido, "?"), Snap `Locate::SnapRevisions` (`snap list --all`,
  `disabled` → `snap remove n --revision=r`, comandos em `Found::commands`). Nomes e descrições marcados com `k("…")`
  (o teste das traduções procura-os). Neste PC (2026-11-13): Scoop 40,3 GB de transferências e 27,2 GB em 132
  versões antigas, pip 1,52 GB, npm 823 MB, Cargo 999 MB; no WSL: Cargo 314 MB, apt 11,6 MB, Snap 105 MB.
  Comandos de limpeza não experimentados a sério.
- `procs.rs` — processos a correr (ToolHelp; `/proc/*/comm` em Linux, nomes cortados a 15): browser aberto → aviso.
- `app.rs` — `Item` (alvo, marca, `State`: `Waiting`, `Analysing`, `Found`, `Cleaning`, `Cleaned`, `NeedsAdmin` —
  não marcável, desmarcado e não guardado), `Row::Group`/`Row::Item`, marcas por item e grupo (`group_tick`),
  `ticks()` (só as diferentes da omissão), `ticked_total`, `Busy`, `freed`.
- `ui/view.rs` — barra dos botões (Analisar, Limpar a cor de destaque, Abrir a pasta; "X a libertar em N ficheiros"),
  ou o trabalho em curso com Parar; aviso de administrador (Windows) com Reiniciar; lista com caixas por grupo e
  item, colunas Ficheiros/Tamanho/Nota; detalhes (descrição, pastas e filtros); barra de estado.
- `main.rs` (com `input.rs`, `menus.rs`, `render.rs`) — analisa ao arrancar; `analyse` e `clean` num fio, eventos
  `Starting`/`Analysed`/`Cleaned`/`Finished` com geração; confirmar com a lista e o total (`*` nos browsers abertos);
  no fim, resumo e nova análise.
- `settings.rs` — tema, língua, janela, `ticks=id:0|1,…`.
- `i18n/strings.txt` — 10 línguas; nomes dos alvos e descrições também (teste `targets_have_translations`).

Medido neste PC (2026-11-13, sem administrador): temporários 3,24 GB em 11 760 ficheiros, Edge 1,56 GB, Chrome
853 MB, sombreadores 1,09 GB; análise de tudo em poucos segundos. Limpar a sério não foi experimentado aqui
(só no teste `analyse_and_clean`, numa pasta própria).

## Fases

1. Feito (2026-11-13): temporários, relatórios de erros, despejos, miniaturas, shaders, Reciclagem/lixo, caches dos
   browsers; analisar, marcar, limpar.
2. Feito (2026-11-13): caches dos gestores de pacotes pelos seus comandos (Scoop, Chocolatey, pip, npm, Yarn, pnpm,
   Go, NuGet; Cargo e vcpkg por pastas; apt, dnf, zypper, pacman, Flatpak, Snap).
3. Atalhos inválidos (pode partilhar o leitor de `.lnk` do runinst, a passar para um crate).
4. Entradas inválidas do registo, com cópia `.reg` antes (como os restos do runinst).

## Regras

- Só apaga o que o utilizador marca e confirma. Nunca histórico, cookies, palavras-passe nem definições.
- Temporários e caches de vez; o que vier a ser registo vai antes para um `.reg`, e atalhos para a Reciclagem.
- Qualquer string visível nova precisa de linha completa em `strings.txt`; teclas de acesso diferentes em cada menu
  em todas as línguas (teste).
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
