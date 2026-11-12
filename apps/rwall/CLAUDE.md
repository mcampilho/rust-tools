# rwall — muda o fundo do ambiente de trabalho

Muda o wallpaper com imagens do Bing (imagem do dia), da NASA (APOD), do Wallhaven, do Picsum ou de pastas locais,
com a frequência escolhida (diária, 00h/12h, 00h/06h/12h/18h, semanal à segunda, mensal no dia 1, a cada N
minutos). Origem: o capítulo "DAILY WALLPAPER" de `E:\Projects\Git\dynamic_wallpapers.md`. Funcionalidades e
limitações no `README.md`.

Três modos (pedido do utilizador, 2026-11-11, como as outras aplicações):
- `rwall` — só a janela de definições; fechar termina; sem mudanças automáticas (a linha de estado diz que é
  preciso o `--tray`). "Imagem seguinte", posição, fontes e o resto funcionam.
- `rwall --tray` — residente (ícone + janela escondida), muda conforme a frequência.
- `rwall --auto` — uma mudança com as definições guardadas, sem janela, e sai (código 0 ou 1; o erro vai para a
  consola, se houver). Com um rwall a correr, entrega-lhe o pedido (`hand_off` do caminho falso `?next`).

## Comandos

```powershell
./dev.ps1 build --release -p rwall          # perfil release (com unwind): target\release\rwall.exe, ~2,9 MB
./dev.ps1 test -p rwall                     # 34 testes no Windows
./dev.ps1 clippy -p rwall --all-targets
./dev.ps1 test -p rwall --lib screenshots '--' --ignored          # apps/rwall/target/screens/*.png
./dev.ps1 test -p rwall --lib live_services '--' --ignored --nocapture   # rede: uma imagem de cada serviço
./dev.ps1 test -p rwall --lib on_and_off '--' --ignored           # escreve e apaga o valor em HKCU\…\Run
```

## Exceção à regra "sem rede"

Aprovada pelo utilizador em 2026-11-11, só para o rwall: pedidos GET às fontes ligadas pelo utilizador, sem
conta, cookies nem telemetria. Windows: WinHTTP (`net.rs`, `windows-sys`, proxy do sistema); Linux: o programa
`curl`. User-Agent genérico `rwall/<versão>`. As outras ferramentas continuam sem rede.

## Arquitetura (`src/`)

- `main.rs` — janela winit (escondida desde o arranque; mostrada logo sem `--tray`), desenhada só quando visível.
  Com `--tray` é a dona do ícone (`rt_sys::win::set_resident`, `tray::show`/`tray::popup`); `close` esconde com
  `--tray` e termina sem ele. Temporizador: `about_to_wait` com `ControlFlow::WaitUntil` a cada 30 s → `tick()`
  (só com `--tray`) compara `schedule::period(agora)` com `settings.last_period`. Uma mudança corre num fio
  (`changer::run` dentro de `catch_unwind`) e volta como `UserEvent::Changed`; falhou → `retry_at` (10 min).
  Instância única: `hand_off(&IDENT, …, any = true)`. `auto()` faz o `--auto` sem event loop. Linux:
  `Shell::with_items` (ícone com menu próprio) com `--tray`, `Shell::new` sem ícone (só o socket) sem ele.
- `schedule.rs` — períodos em hora local (`d2026-11-11`, `h…-0/1`, `q…-0..3`, `w<semana desde a 1.ª segunda de
  1970>`, `m2026-11`, `c<n>-<minutos/n>`), `next_change`, calendário civil (`days_from_civil`).
  Mudar a frequência grava o período atual: não muda logo.
- `sources/` — `Source` (bing, nasa, wallhaven, picsum, folders), `Request`, `Pick`, `fetch`. `online.rs`: Bing
  `HPImageArchive.aspx?format=js&n=8` (`urlbase` + `_UHD.jpg`; título e créditos tirados de `copyright`, porque
  `title` vem "Info" em alguns mercados), NASA APOD pela lista WordPress de science.nasa.gov (`wp-json/wp/v2/apod-basic`, desde 2026-11-12, pedido do
  utilizador: o `api.nasa.gov/planetary/apod` dava 500/429; sem chave; `_fields=` para não trazer o `basic_html`,
  `per_page`/`page`; os 10 dias mais recentes e depois uma página de 20 ao acaso entre 2 e 501; `hdurl` é a imagem
  grande; títulos com entidades HTML lidos por `plain_text`), Wallhaven (`sorting=random`, `purity=100`, `atleast=<ecrã>`), Picsum (o
  número da foto sai do URL final depois do redirecionamento; autor em `/id/<n>/info`). `folders.rs`: lista
  recursiva (≤ 100 000 ficheiros, profundidade 16, sem pastas `.`), escolha ao acaso fora do histórico.
- `changer.rs` — `job` (pedido a partir das definições), `order` (as fontes rodam: `next_source`), `run` (uma a
  seguir à outra até uma dar certo; `picture::ready`, `wallpaper::set`, `picture::trim`, miniatura), `record`
  (grava a mudança nas definições). Usado pela janela e pelo `--auto`.
- `picture.rs` — tipo pelos primeiros bytes, `check` (≥ 320×320 e legível), `save` (escrita atómica), WebP → PNG em
  `images/converted` (o Windows pode não ter o codec), `thumbnail` (crate `image`), `trim` (nunca apaga o atual).
- `wallpaper.rs` — Windows: `WallpaperStyle`/`TileWallpaper` em `HKCU\Control Panel\Desktop` +
  `SystemParametersInfoW(SPI_SETDESKWALLPAPER, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE)`. Linux: plasma, xfconf,
  gsettings (GNOME/Cinnamon, `picture-uri` e `picture-uri-dark`), feh.
- `autostart.rs` — `HKCU\…\CurrentVersion\Run\rwall = "<exe>" --tray`; Linux `~/.config/autostart/rwall.desktop`.
- `net.rs` — `split_url`, `encode`, `get(url, limite)` → corpo + URL final (`WINHTTP_OPTION_URL`).
- `json.rs` — leitor de JSON próprio (sem serde), com limite de profundidade.
- `settings.rs` — `settings.ini` (`rt_core::store`): frequência, fontes, pastas (`folder=` repetido), posição,
  `keep`, `last_period`, `next_source`, `current.*`, `shown=` (histórico, 200).
- `ui/view.rs` — a janela de definições numa só função `draw`, que devolve os retângulos clicáveis (`Hit`) e a
  altura do conteúdo (roda do rato). Caixas de texto simples (`Field`: escrever, Backspace, Ctrl+V, Enter/Esc).
- `commands.rs` — menu do ícone (`TrayCmd`, `tray_menu`), com teclas de acesso verificadas por teste.
- `i18n/strings.txt` — 10 línguas, como nas outras aplicações.

## Verificado (2026-11-11, Windows 10)

Bing, Wallhaven e Picsum descarregados (`live_services`); a NASA pela nova API em 2026-11-12, incluindo uma página do arquivo ao acaso. Corrido de verdade
em modo portátil: `--tray` no primeiro arranque pôs a imagem do Bing; o pedido de outra instância passou à de
ontem; pasta local com período vencido mudou ao arrancar com `--tray`; "a cada 5 min" mudou às 23:20 e às 23:25;
`--auto` sozinho mudou e saiu com 0, e com um residente a correr entregou-lhe o pedido; sem `--tray`, a janela não
mudou nada sozinha e fechar terminou o processo; "Iniciar com o Windows" escreve e apaga o valor. A janela real
foi vista em capturas. Não verificado: o menu do ícone (não se abriu, havia um jogo no ecrã) e o Linux a correr
(compila, os testes passam no WSL, binário de 6,8 MB).

## Regras

- Nunca mexer nas imagens das pastas do utilizador; só apagar o que está em `images/` (descarregadas e convertidas).
- Antes de testar a correr de verdade, guardar o wallpaper atual (`HKCU\Control Panel\Desktop\WallPaper` e
  `WallpaperStyle`) e repô-lo no fim; usar modo portátil (`rwall.portable` ao lado do `.exe`) e apagá-lo depois;
  preferir a compilação `release` (a de depuração abre uma consola) e não roubar o foco ao utilizador.
- Qualquer texto visível novo precisa de linha completa em `strings.txt`.
- `unsafe` só em `net.rs`, `wallpaper.rs`, `autostart.rs` e `screen_size` no `main.rs` (chamadas Win32), com `SAFETY`.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
