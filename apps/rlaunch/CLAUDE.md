# rlaunch — menu portátil de aplicações

Pedido do utilizador (2026-11-26): um menu genérico, ao estilo dos da PortableApps e da Lupo PenSuite, que inclua as
aplicações do Scoop sem se limitar a elas, muito personalizável, em Windows e Linux, a funcionar em disco local e em
pen USB. Decidido: Ctrl+Alt+Espaço; categorias por lista interna, corrigíveis à mão (fase 2); nome `rlaunch` (há
vários `rmenu`). Funcionalidades e fases no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rlaunch      # ~1,9 MB
./dev.ps1 test -p rlaunch
./dev.ps1 test -p rlaunch --lib screenshots '--' --ignored --nocapture   # target/screens/*.png com os programas deste PC
```

## Arquitetura (`src/`)

- `sources.rs` — `Entry`, `Root`, `roots` (Scoop = `Start Menu\Programs\Scoop Apps` do utilizador e de todos, depois
  as pastas, depois o menu Iniciar sem as do Scoop), `collect` (`.lnk/.exe/.bat/.cmd`, 3 níveis, sem seguir junções,
  sem desinstaladores nem atalhos para documentos; um por nome), `desktop` (`.desktop`, testado em qualquer sistema).
- `category.rs` — `Category` (dez de origem e `Own`, com o nome internado) e `Group` (categoria e, no máximo, um
  subgrupo; nas definições `Nome/Subgrupo`, com os nomes ingleses nas de origem). `guess` por palavras inteiras; a
  primeira categoria da lista `KNOWN` ganha. Precedência: escolha do utilizador > pasta (`sources::place`, conforme
  `settings::FolderMode`) > adivinhação; mudar o nome ou remover uma categoria são `aliases` em `items.rs`, porque as
  pastas continuam com o nome antigo.
- `items.rs` — o que o utilizador mudou (nome, categoria, ícone, favorito, oculto, por caminho) e os programas
  acrescentados; `apply` sobrepõe-o ao que a leitura deu (`main.rs` guarda `raw`), sem voltar a ler as pastas.
- `portableapps.rs` — o formato PortableApps (`appinfo.ini`) e a importação do `PortableAppsMenu.ini` (UTF-16:
  `AppsFavorite`, `AppsHidden`, `AppsRenamed`, `AppsRecategorized`; só o primeiro visto num ficheiro real). Só lê.
- `xdg.rs` — Linux, em ficheiros simples (testado em qualquer sistema): tema de ícones e procura do ícone, pastas do
  utilizador, `OnlyShowIn`/`NotShowIn`. `ui/linux_icon.rs` desenha PNG e SVG (`resvg`, só em Linux). `ui/symbols.rs`:
  os símbolos do menu em linhas, onde não há a fonte de ícones do Windows.
- `app.rs` — vista (categorias com os favoritos no topo, dentro de uma, ocultos, opções), procura (sem acentos), seleção e scroll. Sem desenho.
- `settings.rs` — `Settings`, `Hotkey`; `pack`/`unpack`: em modo portátil as pastas do disco do rlaunch ficam `*:\…`.
- `ui/view.rs` — o menu numa função `draw` que devolve os alvos de clique (`Hit`), as linhas visíveis e os programas
  mostrados (cujos ícones se pedem). 350×480 lógicos (×`Settings::size`; era 560×620, grande demais para o utilizador). As opções ocupam o menu todo. `ui/theme.rs` cores.
- `main.rs` — janela sem moldura, sempre por cima, fora da barra de tarefas; residente (ícone, `rt_sys::win::set_hotkey`,
  `hand_off`); esconde ao perder o foco (menos com um diálogo aberto, `asking`); leitura das pastas num fio
  (`Scanned` com geração) a cada abertura; ícones noutro fio (`rt_sys::win::icon::of_file`, com COM). Posição: sempre o
  canto inferior direito (junto ao relógio) do ecrã do ponteiro; pelo lado do ponteiro, o utilizador viu-o por cima
  do botão Iniciar (`rt_sys::win::work_area_at_cursor`). Linux: a janela é destruída ao
  esconder; sem anfitrião do ícone, fechar termina.

## Verificado (2026-11-26, Windows 10)

276 programas neste PC (23 em "Outros"), capturas do teste `screenshots`; arranque `--tray` em modo portátil (17 MB) e
segundo arranque entregue ao primeiro. Clippy e testes em Windows e no WSL. Neste PC o Ctrl+Alt+Espaço já está
ocupado (provavelmente pela aplicação do Claude). Depois de reduzir o menu para 350×480 e acrescentar a
transparência (lida numa janela real), o utilizador experimentou-o: "parece estar tudo a funcionar bem". Não
verificado: pen USB, vários ecrãs e o Linux a correr.

Fase 2 (2026-11-26): menu do programa (`rt_gfx::menu`, desenhado na janela; ancorado à esquerda para o submenu
caber), mudar o nome na própria linha (`rt_core::textedit`), afixar. Experimentado numa cópia com outra identidade
(`IDENT` mudado só para o teste, porque o rlaunch do utilizador estava a correr), enviando mensagens à janela: mudar o
nome, favorito, categoria, ocultar e a lista de ocultos. Não experimentado: arrastar ficheiros, os diálogos de
escolher programa e ícone, executar como administrador.

Administrador (2026-11-26): "Executar sempre como administrador" por programa (escudo), "Modo admin" e "Arrancar
sempre como administrador" (`restart_elevated` com `--elevated-restart`, que espera a instância anterior sair);
elevado, `accept_hand_off` deixa um arranque normal mostrar o menu. O utilizador confirmou a posição à direita, o
arrastar, o ícone, abrir a localização e o escudo; o arranque sempre elevado não foi confirmado.

Numa cópia de teste com o código alterado, ao repor o ficheiro atualizar-lhe a data, ou o cargo não recompila e o
executável fica o de teste.

Fases 3 e 4 (2026-11-26): opções em dois separadores, cores (`ui/theme.rs`, `theme.ini`), linhas grandes, posição.
Verificado com a PortableApps do utilizador em `S:\Portable\PortableApps`: 333 programas em 0,15 s, 37 escolhas.
PStart, LiberKey e Lupo/ASuite não foram feitos: não há instalação para verificar o formato.

Fase 5 (2026-11-27): verificada no WSL com aplicações de exemplo (ícones PNG e SVG, capturas) e arranque em X11 e
Wayland durante 4 s; o executável Linux ficou com 7,6 MB (o resvg junta cerca de 0,8 MB). Não verificado num ambiente
de trabalho Linux a sério (área de notificação, abrir programas, temas de ícones do GNOME/KDE, posição).

Categorias em dois níveis (2026-11-27, pontos 1 a 5 e melhorias pedidas): pastas, menu Iniciar (`sources::group_suites`),
à mão, ordem própria (`Items::order`, `App::custom_order`), arrastar (`main.rs`: `Press`, `drop_at`, `drop_on`), marcar
vários (`App::marked`), consolas (`sources::console_program` lê o subsistema do PE; `rt_sys::win::launch_in_console`).
Experimentado numa cópia de teste: arrastar para dentro, promover, reordenar, marcar (o Ctrl simulado por um ficheiro,
porque mensagens enviadas não mudam o estado das teclas), mover os marcados e o `Clockres64` numa consola. Não
experimentado: Shift+clique, arrastar programas, consola como administrador. Pastas dele: `E:\Portable\scoop\links` e
`E:\Portable\Tools\sys\SysInternals` / `...\sys\WSCC\Apps\Sysinternals Suite` (modo "Uma categoria").
Numa cópia de teste em `debug` há uma consola: escolher a janela pela largura.

## Regras

- Nunca mexer nos atalhos nem nos programas: o menu só os lê e abre.
- Sem rede, salvo "Procurar atualizações".
- Qualquer texto visível novo precisa de linha completa em `i18n/strings.txt`.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
