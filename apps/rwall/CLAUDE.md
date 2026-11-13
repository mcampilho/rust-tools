# rwall — muda o fundo do ambiente de trabalho

Muda o wallpaper com imagens do Bing (imagem do dia), da NASA (APOD), do Wallhaven, do Picsum ou de pastas locais,
com a frequência escolhida (diária, 00h/12h, 00h/06h/12h/18h, semanal à segunda, mensal no dia 1, a cada N
minutos). Origem: o capítulo "DAILY WALLPAPER" de `E:\Projects\Git\dynamic_wallpapers.md`. Funcionalidades e
limitações no `README.md`.

Desde 2026-11-12 tem também um **fundo dinâmico** (capítulo "DYNAMIC WALLPAPERS" do mesmo documento), só no
Windows, num segundo separador da janela: vídeo, pasta de vídeos (roda com a frequência do separador estático),
GIF/WebP animado ou cena desenhada pelo rwall (auroras, céu estrelado, mar ao pôr do sol, fluxo de cores). Escolhas
do utilizador: os quatro tipos; o mesmo vídeo em cada ecrã (Preencher/Ajustar); pausa em ecrã inteiro, janelas
maximizadas em todos os ecrãs, PC bloqueado/ecrã apagado e bateria (todas ligadas por omissão); Linux fica para
depois (o separador diz que não está disponível). Requisito: tudo o que está no ambiente de trabalho (ícones,
ficheiros, notificações, widgets) fica por cima.

Desde 2026-11-12 tem também **widgets** (capítulo "DESKTOP WIDGETS"), só no Windows, num terceiro separador:
relógio analógico, relógio digital, calendário, relógio + calendário, nota adesiva, anotação, meteorologia, fence e
fotos. Escolhas do utilizador: presos ao ambiente de trabalho por omissão (por cima do fundo e dos ícones, por baixo
das janelas) com a opção "Sempre visível" por widget; meteorologia da Open-Meteo (sem chave; é a segunda fonte de rede
aprovada, só GET); fence = caixa que mostra uma pasta (duplo clique abre, arrastar para fora e para dentro); anotação =
texto solto sobre o fundo (as notas adesivas são papelinhos coloridos).

Três modos (pedido do utilizador, 2026-11-11, como as outras aplicações):
- `rwall` — só a janela de definições; fechar termina; sem mudanças automáticas (a linha de estado diz que é
  preciso o `--tray`). "Imagem seguinte", posição, fontes e o resto funcionam.
- `rwall --tray` — residente (ícone + janela escondida), muda conforme a frequência.
- `rwall --auto` — uma mudança com as definições guardadas, sem janela, e sai (código 0 ou 1; o erro vai para a
  consola, se houver). Com um rwall a correr, entrega-lhe o pedido (`hand_off` do caminho falso `?next`).

## Comandos

```powershell
./dev.ps1 build --release -p rwall          # perfil release (com unwind): target\release\rwall.exe, ~3,7 MB
./dev.ps1 test -p rwall                     # 54 testes no Windows
./dev.ps1 clippy -p rwall --all-targets
./dev.ps1 test -p rwall --lib screenshots '--' --ignored          # apps/rwall/target/screens/*.png
./dev.ps1 test -p rwall --lib live_services '--' --ignored --nocapture   # rede: uma imagem de cada serviço
./dev.ps1 test -p rwall --lib on_and_off '--' --ignored           # escreve e apaga o valor em HKCU\…\Run
./dev.ps1 test -p rwall --lib scene_pictures '--' --ignored       # target/screens/scene-*.png e flow.gif (GIF de teste)
./dev.ps1 test -p rwall --release --lib scene_speed '--' --ignored --nocapture   # ms por imagem de cada cena
./dev.ps1 test -p rwall --lib widget_pictures '--' --ignored      # target/screens/widgets-*.png (claro, escuro, 150 %, céus)
./dev.ps1 test -p rwall --lib weather_live '--' --ignored --nocapture   # Open-Meteo de verdade (Lisboa)
./dev.ps1 test -p rwall --lib pinned_window_order '--' --ignored --nocapture   # janela invisível: fica logo acima dos ícones
```

## Exceção à regra "sem rede"

Aprovada pelo utilizador em 2026-11-11, só para o rwall: pedidos GET às fontes ligadas pelo utilizador, sem
conta, cookies nem telemetria. Windows: WinHTTP (`net.rs`, `windows-sys`, proxy do sistema); Linux: o programa
`curl`. User-Agent genérico `rwall/<versão>`. As outras ferramentas continuam sem rede. Desde 2026-11-12 também a
Open-Meteo (geocodificação e previsão) para o widget de meteorologia, escolhida pelo utilizador.

## Arquitetura (`src/`)

- `main.rs` (com `input.rs`, `live_tab.rs` e `widgets_tab.rs`: o mesmo `impl Shell` repartido por assunto; a janela e os controlos do `ui/view.rs` vêm do `rt-ui`) — janela winit (escondida desde o arranque; mostrada logo sem `--tray`), desenhada só quando visível.
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
- `json.rs` — reexporta o leitor de JSON (sem serde, com limite de profundidade), que passou para `rt_core::json` em 2026-11-13 (o runinst também o usa).
- `settings.rs` — `settings.ini` (`rt_core::store`): frequência, fontes, pastas (`folder=` repetido), posição,
  `keep`, `last_period`, `next_source`, `current.*`, `shown=` (histórico, 200).
- `ui/view.rs` — a janela de definições numa só função `draw`, que devolve os retângulos clicáveis (`Hit`) e a
  altura do conteúdo (roda do rato). Caixas de texto simples (`Field`: escrever, Backspace, Ctrl+V, Enter/Esc).
- `commands.rs` — menu do ícone (`TrayCmd`, `tray_menu`), com teclas de acesso verificadas por teste. `LivePause`
  (só Windows) troca o texto para `RESUME` quando em pausa.
- `live/` — fundo dinâmico. `mod.rs`: `LiveSettings` (`live.*` no `settings.ini`), `Kind`, `LiveFit`, `PauseWhen`,
  `Show`/`Spec`/`State`/`Reason`, `pick_video`, `place` (recorte normalizado e destino por ecrã), `composed_size` e
  `compose` (imagem por ecrã à escala da imagem original, cortada ou com barras), `preview` (miniatura).
  `scene.rs`: `Scene` e `Painter` (CPU, `0x00RRGGBB`, fundo fixo pré-desenhado em `base`; 960×540, o fluxo 320×180;
  0,4–2,8 ms por imagem). `anim.rs`: GIF/WebP para memória (≤ 384 MB, reduz se preciso). Só Windows:
  `desktop.rs` (truque `0x052C` → `WorkerW`; 24H2: filho do `Progman` logo abaixo do `SHELLDLL_DefView`; senão
  `HWND_TOP` dentro do `WorkerW`, por cima de outras apps de fundos como o Bing Wallpaper; verificações de pausa;
  `repaint_wallpaper` volta a pôr o fundo estático com `SPI_SETDESKWALLPAPER`), `player.rs` (fio próprio com
  ciclo de mensagens: uma janela por ecrã, cada uma com swap chain D3D11 `DXGI_SCALING_STRETCH`; vídeo pelo
  `IMFMediaEngine` em modo frame-server com `TransferVideoFrame`; cenas/GIF por `UpdateSubresource` na swap chain
  pequena, que a placa estica — o GDI `StretchDIBits` HALFTONE gastava ~50 % de um núcleo; janela de controlo
  escondida para `TaskbarCreated`, `WM_DISPLAYCHANGE` e `GUID_CONSOLE_DISPLAY_STATE`; verifica a cada segundo se
  continua ligado e se deve pausar), `thumb.rs` (miniatura de vídeo pelo `IShellItemImageFactory`).
- `live_tab.rs` (fundo dinâmico) — `apply_live`/`live_changed`/`next_video`; o `Player` nasce na primeira vez e é largado
  em `quit` (pára e repõe o fundo). Com o dinâmico ligado, o `tick` não muda a imagem estática (só roda o vídeo da
  pasta). Erro do leitor → `live.on = false` e mensagem.
- `widgets/` — widgets do ambiente de trabalho. `mod.rs`: `WKind` (9 tipos, tamanhos por omissão e mínimos),
  `Widget` (posição em píxeis do ecrã, `on_top`, `locked`, opções por tipo), `WidgetSettings` (`widgets`,
  `widgets.ids` e `widget.<id>.*` no `settings.ini`; acrescentam-se até 16 (`MAX_WIDGETS`), lêem-se até 64; relógios,
  calendários e meteorologia: um de cada tipo, `WKind::single`, `can_add`). `region.rs`: definições regionais lidas uma
  vez (`GetLocaleInfoEx`: 24 h, °F por `LOCALE_IMEASURE`, primeiro dia só se sábado/domingo/segunda;
  `GetUserDefaultGeoName`: país; `icu.dll` `ucal_getDefaultTimeZone`: fuso IANA → cidade "Lisbon, PT"); valores
  iniciais de `Widget::new`. `holidays.rs`: feriados nacionais de 9 países (`COUNTRIES`; fixos, Páscoa, "n-ésima
  segunda"); nomes são chaves da tabela. `calendar.rs`: dia da semana, grelha de 6 semanas,
  nomes traduzidos e modelos de data `%W %D %M %Y` (linhas da tabela; sem definição regional, a semana começa ao
  domingo em en-US, pt-BR e ja). `edit.rs`: `Editor` (cursor, seleção, palavras) e `layout` (quebra de linhas). `weather.rs`: Open-Meteo
  (`count=10`, fica o mais populoso, ou o da região/país depois da vírgula; códigos WMO → `Sky`). `paint.rs`: desenha
  um widget numa imagem transparente (tiny-skia + `Gfx`) e devolve as zonas (`Zone`; `Zone::Day` nos feriados, para
  os nomear por baixo do mês); testável em qualquer sistema. Um widget novo vai para o primeiro sítio livre a partir
  do canto superior direito (`Host2::place`).
  `host/` (Windows; `mod.rs` o fio e as janelas, `input.rs` rato, teclado e menu, `timers.rs` o tempo e o desenho, `fence.rs` arrastar e largar): fio próprio com OLE, uma janela `WS_EX_LAYERED | WS_EX_TOOLWINDOW` por widget
  (`UpdateLayeredWindow`); presas = donas (`GWLP` owner) da janela que tem os ícones (`desktop::icons_window`, depois
  do `0x052C`) e `HWND_BOTTOM` → logo acima dos ícones; `WM_WINDOWPOSCHANGING` com `SWP_NOZORDER` segura o lugar
  (mesmo ao escrever numa nota, que se ativa; as outras têm `WS_EX_NOACTIVATE`). O procedimento só põe eventos numa
  fila; o ciclo trata-os (sem reentrância). Arrastar/redimensionar à mão (`SetCapture`), menu `TrackPopupMenuEx` na
  janela de controlo escondida, fences com `IDropTarget` (`SHFileOperationW` num fio; mover mesmo disco, copiar de
  outro; a um mover responde `DROPEFFECT_NONE` para a origem não apagar) e `DoDragDrop` com o `IDataObject` do
  Explorador; ícones por `thumb::shell_image` num fio. Verifica a cada 2 s o dono, se as janelas existem e as pastas
  das fences; `TaskbarCreated` recria. Eventos para a janela: `Changed`, `Remove`, `NewNote`, `Settings`, `Status`.
- `widgets_tab.rs` (widgets) — `apply_widgets` arranca/para o `Host` e manda a lista e o estilo (tema, acento, língua: a
  língua é por fio). `widget_event` guarda o que os widgets mudam (sem pisar a cidade a ser escrita). Cidade nova →
  `place` vazio → o anfitrião procura-a outra vez.
- `i18n/strings.txt` — 10 línguas, como nas outras aplicações.

## Verificado (2026-11-11, Windows 10)

Bing, Wallhaven e Picsum descarregados (`live_services`); a NASA pela nova API em 2026-11-12, incluindo uma página do arquivo ao acaso. Corrido de verdade
em modo portátil: `--tray` no primeiro arranque pôs a imagem do Bing; o pedido de outra instância passou à de
ontem; pasta local com período vencido mudou ao arrancar com `--tray`; "a cada 5 min" mudou às 23:20 e às 23:25;
`--auto` sozinho mudou e saiu com 0, e com um residente a correr entregou-lhe o pedido; sem `--tray`, a janela não
mudou nada sozinha e fechar terminou o processo; "Iniciar com o Windows" escreve e apaga o valor. A janela real
foi vista em capturas. Não verificado: o menu do ícone (não se abriu, havia um jogo no ecrã) e o Linux a correr
(compila, os testes passam no WSL, binário de 6,8 MB).

Fundo dinâmico (2026-11-12, Windows 10 22H2, um ecrã 1920×1080, com a app Bing Wallpaper também na mesma camada):
cena das auroras, `dynamic_boreal.mp4` (vídeo, ~1,4 % de um núcleo, 81 MB) e um GIF animado passaram atrás dos
ícones, com ícones e barra de tarefas por cima (capturas do ecrã); auroras a ~9 % de um núcleo depois de passar o
esticar para a placa. Fechar a janela sem `--tray` parou o vídeo e repôs o fundo estático (saída 0). Morto à força,
a última imagem fica até o fundo voltar a ser pintado. Não verificado: Windows 11 24H2 (outra camada), vários ecrãs,
reinício do Explorador, as pausas a acontecer de verdade (só a lógica), a rotação da pasta de vídeos e o separador
dinâmico na janela real (só nas capturas do teste `screenshots`).

Widgets (2026-11-12, Windows 10 22H2, 1920×1080, release em modo portátil, com o utilizador a jogar): as nove janelas
nasceram donas do `WorkerW` dos ícones, logo acima dele e abaixo de todas as outras (lista das janelas pela API; o jogo
ficou por cima, como deve); o conteúdo de cada uma foi visto com `PrintWindow` (ícones reais do Explorador na fence,
foto carregada, meteorologia de Lisboa); 9 widgets (3 com segundos) a ~0,9 % de um núcleo e 36 MB. A procura com
`count=1` deu "Lisboa, Moçambique": corrigido (mais populoso). Não verificado: as interações (arrastar, escrever nas
notas, menus, largar ficheiros numa fence, arrastar para fora), Win+D, Windows 11 24H2, vários ecrãs, DPI misto e o
reinício do Explorador — o utilizador estava a jogar e não se roubou o foco.

## Regras

- Nunca mexer nas imagens das pastas do utilizador; só apagar o que está em `images/` (descarregadas e convertidas).
- Antes de testar a correr de verdade, guardar o wallpaper atual (`HKCU\Control Panel\Desktop\WallPaper` e
  `WallpaperStyle`) e repô-lo no fim; usar modo portátil (`rwall.portable` ao lado do `.exe`) e apagá-lo depois;
  preferir a compilação `release` (a de depuração abre uma consola) e não roubar o foco ao utilizador.
- Qualquer texto visível novo precisa de linha completa em `strings.txt`.
- `unsafe` só em `net.rs`, `wallpaper.rs`, `autostart.rs`, `live/desktop.rs`, `live/player.rs`, `live/thumb.rs`,
  `widgets/host.rs` e `screen_size` no `main.rs` (chamadas Win32/COM), com `SAFETY`.
- Widgets presos nunca acima das outras janelas; não roubar o foco (só uma nota em que se clica se ativa).
- Fundo dinâmico: nunca pôr a janela acima da camada dos ícones; ao parar (e ao sair) repor sempre o fundo estático.
  Caminhos escritos à mão no `settings.ini` levam `\\` (o ficheiro usa escapes).
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
