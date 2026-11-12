# rsize — analisador de espaço em disco portátil em Rust

Lê um disco, uma pasta ou uma partilha de rede e mostra o que ocupa espaço, disposto como no WizTree: bloco
informativo em cima (pasta, tempo e forma da leitura, espaço total/usado/reservado/livre do volume), lista de
pastas à esquerda e tipos de ficheiro à direita (todas as colunas ordenam), e um gráfico em baixo (treemap com
sombreado em almofada e cabeçalhos de pasta, tarte ou anéis);
exporta para CSV e manda para a Reciclagem. Um único executável (~1,9 MB em Windows, 5,5 MB em Linux), sem
rede própria nem telemetria. Funcionalidades, atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rsize
./dev.ps1 test -p rsize -p rt-scan   # 28 + 28 testes no Windows
./dev.ps1 clippy -p rsize --all-targets
./dev.ps1 test -p rsize --release --lib screenshots '--' --ignored   # páginas em apps/rsize/target/screens/*.png
$env:RSIZE_BENCH_DIR = 'C:\'; ./dev.ps1 test -p rt-scan --release --lib bench '--' --ignored --nocapture
./dev.ps1 test -p rt-scan --lib probe '--' --ignored --nocapture   # o que o Everything responde para RSIZE_BENCH_DIR
```

Correm na raiz do repositório. Saída: `target\release-small\rsize.exe` (perfil `release-small`: não precisa de
`catch_unwind`).

## Arquitetura (`src/`)

Desde 2026-11-07 a leitura dos discos, a árvore, `volumes` e `format` estão no crate `crates/rt-scan` (partilhado
com o rdup); o `lib.rs` do rsize reexporta-os (`scan`, `model`, `volumes`, `format`) e os testes deles correm com
`./dev.ps1 test -p rt-scan` (a imagem NTFS está em `crates/rt-scan/tests/fixtures`). Os nomes abaixo são os
módulos do crate:

- `model.rs` — a árvore num bloco compacto (`Tree`, ~56 bytes por nó mais o nome). As entradas de uma pasta
  entram juntas, com ids seguidos e sempre depois da pasta; `finish` soma os tamanhos do último id para o
  primeiro e ordena os filhos. Dois tamanhos: `Logical` (comprimento) e `Allocated` (espaço em disco).
- `scan/` — `mod.rs`: coordenador (monta a árvore, conta ligações físicas uma vez) e até 8 fios que listam
  pastas; `win.rs`: `GetFileInformationByHandleEx(FileIdBothDirectoryInfo)` com prefixo `\\?\`, e
  `FindFirstFile` como recurso; `unix.rs`: `readdir` + `lstat`, sem atravessar sistemas de ficheiros.
  Ligações, junções e pontos de montagem não são seguidos.
- `scan/ntfs.rs` — leitura direta da MFT (Windows, volume local NTFS, administrador): setor de arranque,
  runs da `$MFT` (incluindo a lista de atributos de uma MFT fragmentada), registos com fixups, árvore montada
  pelos `$FILE_NAME`. O `scan` tenta-a primeiro e cai para pasta a pasta (`Scan::admin_would_help` quando falta
  administrador). Lê qualquer `Disk` (Read + Seek): os testes usam `tests/fixtures/ntfs-small.img.zlib`, feita
  pelo `make-ntfs.sh` ao lado (mkntfs + ntfscp no WSL). Neste PC não há administrador: nunca foi corrida num
  disco real por mim. `rt_sys::win::{elevated, restart_elevated}` para o reinício como administrador.
- `scan/everything.rs` — índice do Everything pela interface de mensagens (sem a DLL): `WM_COPYDATA` "query 2"
  (dwData 18) para a janela `EVERYTHING_TASKBAR_NOTIFICATION`, resposta "list 2" para uma janela só de
  mensagens do fio da leitura. Pede nome, pasta, tamanho e data; a árvore sai das pastas (sem distinguir
  maiúsculas). Só é usado se o primeiro nível da pasta bate com o disco (≥ 95 %) e há tamanhos (≥ 90 %).
  Ordem no `scan`: Everything → NTFS → pasta a pasta (`scan_with` + `Ways`; os testes usam `Ways::SYSTEM`).
  O comando 411 do IPC antigo ("está indexado?") responde 0 no Everything 1.5: não usar. Enquanto o Everything
  carrega a base de dados, o 401 responde 0 e uma pergunta pode levar mais de um minuto: por isso `running()`
  exige o 401. Medido em 2026-11-07 com o Everything 1.5.0.1423b: C:\ em 2,3 s, E:\Projects (1,6 M) em 5,6 s.
- `treemap.rs` — disposição squarified e cushions (van Wijk), desenhados pixel a pixel numa `Pixmap`; mapa
  pixel → bloco para os cliques. Cores pelas 12 extensões maiores. `Map::new(.., head)`: as pastas com pelo
  menos 5× `head` de largura e 3× de altura recebem um cabeçalho (`Tile::head`) e uma moldura de 1 px, pintados
  com `head_color(depth)` antes do conteúdo e donos desses pixels (um clique no cabeçalho dá a pasta); o
  conteúdo é disposto por baixo. O texto dos cabeçalhos é desenhado a cada imagem em `view::draw_heads`.
- `chart.rs` — tarte (12 fatias, o resto em "Outros", legenda) e anéis (até 6 níveis, cor pelo ângulo, mais
  clara para fora, segmentos com menos de 3 px omitidos). Desenhados uma vez numa `Pixmap`; o segmento sob o
  ponteiro (e, nos anéis, o que tem dentro) é redesenhado deslocado para fora em `view.rs`.
- `export.rs` — CSV da pasta do gráfico (BOM UTF-8, vírgulas, CRLF, cabeçalhos traduzidos).
- Eliminar: `rt_sys::recycle` / `rt_sys::win::recycle` (`SHFileOperationW` com `FOF_ALLOWUNDO |
  FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING`: o Windows avisa antes de apagar de vez) e `rt_sys::linux::trash`
  (`gio trash`, ou o lixo freedesktop em `~/.local/share/Trash`). Depois, `Tree::remove` e `App::removed`
  atualizam a árvore sem voltar a ler; `tree_gen` sobe para refazer os gráficos.
- Painel de tipos (sempre visível, à direita da lista; o separador Tipos e o F9 saíram em 2026-11-11):
  `App::exts` = `Tree::ext_totals(zoom)`, refeito pelo `main` quando muda a pasta do gráfico, o modo ou a árvore
  (`exts_key`); desenhado em `view::draw_exts`. Nome do tipo: `rt_sys::win::file_type_name` (`SHGetFileInfoW`
  com `SHGFI_USEFILEATTRIBUTES`, só registo), pedido num fio à parte depois de cada leitura
  (`UserEvent::TypeNames`); sem nome, "{} File" traduzido.
- `volumes.rs` — discos (Windows) ou montagens reais (`/proc/self/mounts`, Linux), lidos num fio à parte.
  `space_of(pasta)` dá o volume da pasta (`GetVolumePathNameW` + `GetDiskFreeSpaceExW`, ou `statvfs`):
  reservado = livre − disponível para o utilizador. Pedido num fio à parte (`UserEvent::Space`).
- `app.rs` — estado sem janela: página, linhas da lista, pastas abertas, seleção, zoom do treemap, ordenação
  (`Sort<Col>` da lista, `Sort<ExtCol>` dos tipos). A árvore guarda os filhos pelo tamanho do modo (o treemap
  precisa disso); outras ordens são feitas em `rebuild` só para as pastas abertas.
- `format.rs` — tamanhos (binários, 3 algarismos), contagens, percentagens, datas locais.
- `ui/view.rs` — as três páginas (discos, leitura, resultado) e o `Layout` usado também para os cliques. No
  resultado: `info` em cima, lista e tipos lado a lado (`vsplit`, `settings.split_x`), gráfico em baixo
  (`splitter`, `settings.split`). As colunas saem de `columns()`: largura fixa e prioridade; com pouco espaço
  saem as de número mais alto, e uma coluna (`Name`, `Type`) fica com o resto;
  `ui/theme.rs`, `ui/appicon.rs`, `ui/menu.rs` (menus desenhados, só fora do Windows).
- `main.rs` — janela, menus nativos (Windows) ou desenhados (Linux), rato, teclado, leitura em segundo plano
  (eventos `Scan(geração, …)`; os de leituras antigas são ignorados). O treemap é refeito quando muda a pasta,
  o tamanho ou a ordem; durante o redimensionamento é esticado e refeito 150 ms depois. A tarte e os anéis
  são refeitos logo (poucos milhares de segmentos), porque esticados ficariam ovais. Modo residente (`--tray`,
  como o rview: ícone `rt_sys::win::tray` ou StatusNotifierItem em Linux; outro arranque passa a pasta com
  `hand_off`, `--new-window` não). Menu "Analisar com o rsize" nas pastas e discos (`rt_sys::win::assoc::
  register_folders`, só HKCU; Ficheiro › Integração com o Windows, `--register`/`--unregister`): o disco chega
  como `C:"` (`folder_arg`). `--help` na consola (`print_to_console`) ou, sem ela, no diálogo da janela.
- `i18n/strings.txt` — traduções (10 idiomas), mesma convenção das outras aplicações.

## Fases

1. Feito (0.1): leitura pelas APIs do sistema, lista e treemap.
2. Feito: gráficos de tarte (TreeSize/JDiskReport) e anéis (Baobab/Scanner), com separadores e F6/F7/F8.
3. Feito: leitura direta do NTFS (MFT), com "Ficheiro > Reiniciar como administrador".
4. Feito: índice do Everything (IPC por `WM_COPYDATA`, sem a DLL), quando está a correr e a pasta está indexada.
5. Feito: separador Tipos (F9), exportação CSV (Ctrl+S), eliminar para a Reciclagem (Delete, com confirmação).
   A confirmação e a gravação usam os diálogos do sistema, que os testes não cobrem.
6. Feito (2026-11-11, inspirado em WizTree, WinDirStat, TreeSize e SpaceSniffer): bloco informativo, colunas
   Alocado/Itens/Pastas, ordenação em todas as colunas, painel de tipos sempre visível com o nome do tipo,
   cabeçalhos de pasta no treemap (clique seleciona na lista, duplo clique mostra a pasta). Por fazer: clicar
   num tipo realçar os seus ficheiros no treemap.
7. Feito (2026-11-11): duas disposições (`settings::ViewMode`, F3/F4, botões Pastas/Mapa à direita do bloco informativo, sempre no mesmo sítio):
   Pastas = lista à esquerda, tipos à direita, treemap em baixo (só treemap: `Settings::chart_shown`); Mapa =
   gráfico à esquerda, tipos à direita, lista em baixo (treemap, tarte ou anéis). Cada uma guarda as suas barras
   (`Settings::folders`/`map`, `Panes`). Clique num tipo: `Map::highlighted` escurece o que não é dessa extensão
   (imagem guardada em `main::lit`). Clique direito no gráfico = Retrocesso; Shift+clique direito abre o menu.

Em curso: os 76 s da leitura direta do NTFS no E: (disco mecânico). Hipótese: o volume era aberto com cache, e
o gestor de cache parte as leituras grandes de um volume em pedaços pequenos, lentos num disco mecânico. Agora
lê sem cache (`ntfs::Volume`, `FILE_FLAG_NO_BUFFERING`, leituras alinhadas a 4 KB) num fio próprio, com
outro a interpretar. `rsize --timings <pasta>` (como administrador) lê duas vezes, sem e com cache, e mostra o
tamanho da MFT, os pedaços, os registos em uso e os tempos de disco, interpretação e árvore.
Medido no E: em 2026-11-07: MFT de 4669 MB em 9 pedaços, 4,78 M registos (3,55 M em uso); 44 s, dos quais 41 s
de disco a 113 MB/s, com ou sem cache (a hipótese da cache estava errada). Os 76 s iniciais eram de uma cópia
grande a correr ao mesmo tempo no mesmo disco; sem ela, ~40 s. É o limite sequencial do disco: só
ler menos ajuda. Por isso, o `$BITMAP` da `$MFT` (registo 0, ou registos de extensão) é lido e as
sequências de registos livres de pelo menos `MIN_GAP` (2 MB) são saltadas (`stretches`, `read_stream`); sem
mapa lê tudo. Medido no E:: lê 3900 MB em 100 sequências em vez de 4669 MB, 40 s em vez de 44 s (102 MB/s, pelos
saltos). É o limite do disco; num disco mecânico grande o caminho rápido é o Everything (15 s no E:).

## Regras

- Só leitura, com uma exceção pedida pelo utilizador: Eliminar, que só manda para a Reciclagem (ou o lixo)
  depois de confirmar. Nunca apagar de vez por iniciativa própria; o aviso do Windows trata dos casos sem
  Reciclagem. O CSV só é escrito onde o utilizador escolhe.
- `unsafe` só para chamadas Win32 (`scan/win.rs`, `scan/ntfs.rs`, `volumes.rs`, `SetErrorMode` no `main`), com
  `SAFETY`. O volume é aberto só para leitura.
- Qualquer string visível nova precisa de linha completa em `strings.txt`.
- Os textos de menu têm de ter teclas de acesso diferentes dentro de cada menu, em todas as línguas (teste com
  `rt_core::menu::access_key_clashes`). A tabela foi gerada por script, mas edita-se à mão: uma linha por texto, colunas separadas por tab.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
