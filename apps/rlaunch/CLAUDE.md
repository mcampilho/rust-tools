# rlaunch — menu portátil de aplicações

Pedido do utilizador (2026-11-26): um menu genérico, ao estilo dos da PortableApps e da Lupo PenSuite, que inclua as
aplicações do Scoop sem se limitar a elas, muito personalizável, em Windows e Linux, a funcionar em disco local e em
pen USB. Decidido: Ctrl+Alt+Espaço; categorias por lista interna, corrigíveis à mão (fase 2); nome `rlaunch` (há
vários `rmenu`). Funcionalidades e fases no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rlaunch      # ~1,7 MB
./dev.ps1 test -p rlaunch
./dev.ps1 test -p rlaunch --lib screenshots '--' --ignored --nocapture   # target/screens/*.png com os programas deste PC
```

## Arquitetura (`src/`)

- `sources.rs` — `Entry`, `Root`, `roots` (Scoop = `Start Menu\Programs\Scoop Apps` do utilizador e de todos, depois
  as pastas, depois o menu Iniciar sem as do Scoop), `collect` (`.lnk/.exe/.bat/.cmd`, 3 níveis, sem seguir junções,
  sem desinstaladores nem atalhos para documentos; um por nome), `desktop` (`.desktop`, testado em qualquer sistema).
- `category.rs` — `Category` e `guess` por palavras inteiras; a primeira categoria da lista `KNOWN` ganha.
- `app.rs` — vista (categorias, dentro de uma, opções), procura (sem acentos), seleção e scroll. Sem desenho.
- `settings.rs` — `Settings`, `Hotkey`; `pack`/`unpack`: em modo portátil as pastas do disco do rlaunch ficam `*:\…`.
- `ui/view.rs` — o menu numa função `draw` que devolve os alvos de clique (`Hit`), as linhas visíveis e os programas
  mostrados (cujos ícones se pedem). 350×480 lógicos (×`Settings::size`; era 560×620, grande demais para o utilizador). As opções ocupam o menu todo. `ui/theme.rs` cores.
- `main.rs` — janela sem moldura, sempre por cima, fora da barra de tarefas; residente (ícone, `rt_sys::win::set_hotkey`,
  `hand_off`); esconde ao perder o foco (menos com um diálogo aberto, `asking`); leitura das pastas num fio
  (`Scanned` com geração) a cada abertura; ícones noutro fio (`rt_sys::win::icon::of_file`, com COM). Posição pelo
  canto da área de trabalho junto ao ponteiro (`rt_sys::win::work_area_at_cursor`). Linux: a janela é destruída ao
  esconder; sem anfitrião do ícone, fechar termina.

## Verificado (2026-11-26, Windows 10)

276 programas neste PC (23 em "Outros"), capturas do teste `screenshots`; arranque `--tray` em modo portátil (17 MB) e
segundo arranque entregue ao primeiro. Clippy e testes em Windows e no WSL. Neste PC o Ctrl+Alt+Espaço já está
ocupado (provavelmente pela aplicação do Claude). Depois de reduzir o menu para 350×480 e acrescentar a
transparência (lida numa janela real), o utilizador experimentou-o: "parece estar tudo a funcionar bem". Não
verificado: pen USB, vários ecrãs e o Linux a correr.

## Regras

- Nunca mexer nos atalhos nem nos programas: o menu só os lê e abre.
- Sem rede, salvo "Procurar atualizações".
- Qualquer texto visível novo precisa de linha completa em `i18n/strings.txt`.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
