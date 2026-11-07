# rsize — analisador de espaço em disco portátil em Rust

Lê um disco, uma pasta ou uma partilha de rede e mostra o que ocupa espaço: lista de pastas por
tamanho e um gráfico à escolha (treemap com sombreado em almofada, tarte ou anéis). Um único executável
(~1,8 MB em Windows, 5,5 MB em Linux),
só leitura, sem rede própria nem telemetria. Funcionalidades, atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rsize
./dev.ps1 test -p rsize       # 41 testes (40 em Linux)
./dev.ps1 clippy -p rsize --all-targets
./dev.ps1 test -p rsize --release --lib screenshots '--' --ignored   # páginas em apps/rsize/target/screens/*.png
$env:RSIZE_BENCH_DIR = 'C:\'; ./dev.ps1 test -p rsize --release --lib bench '--' --ignored --nocapture
```

Correm na raiz do repositório. Saída: `target\release-small\rsize.exe` (perfil `release-small`: não precisa de
`catch_unwind`).

## Arquitetura (`src/`)

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
- `treemap.rs` — disposição squarified e cushions (van Wijk), desenhados pixel a pixel numa `Pixmap`; mapa
  pixel → bloco para os cliques. Cores pelas 12 extensões maiores.
- `chart.rs` — tarte (12 fatias, o resto em "Outros", legenda) e anéis (até 6 níveis, cor pelo ângulo, mais
  clara para fora, segmentos com menos de 3 px omitidos). Desenhados uma vez numa `Pixmap`; o segmento sob o
  ponteiro (e, nos anéis, o que tem dentro) é redesenhado deslocado para fora em `view.rs`.
- `volumes.rs` — discos (Windows) ou montagens reais (`/proc/self/mounts`, Linux), lidos num fio à parte.
- `app.rs` — estado sem janela: página, linhas da lista, pastas abertas, seleção, zoom do treemap.
- `format.rs` — tamanhos (binários, 3 algarismos), contagens, percentagens, datas locais.
- `ui/view.rs` — as três páginas (discos, leitura, resultado) e o `Layout` usado também para os cliques;
  `ui/theme.rs`, `ui/appicon.rs`, `ui/menu.rs` (menus desenhados, só fora do Windows).
- `main.rs` — janela, menus nativos (Windows) ou desenhados (Linux), rato, teclado, leitura em segundo plano
  (eventos `Scan(geração, …)`; os de leituras antigas são ignorados). O treemap é refeito quando muda a pasta,
  o tamanho ou a ordem; durante o redimensionamento é esticado e refeito 150 ms depois. A tarte e os anéis
  são refeitos logo (poucos milhares de segmentos), porque esticados ficariam ovais.
- `i18n/strings.txt` — traduções (10 idiomas), mesma convenção das outras aplicações.

## Fases

1. Feito (0.1): leitura pelas APIs do sistema, lista e treemap.
2. Feito: gráficos de tarte (TreeSize/JDiskReport) e anéis (Baobab/Scanner), com separadores e F6/F7/F8.
3. Feito: leitura direta do NTFS (MFT), com "Ficheiro > Reiniciar como administrador".
4. Índice do Everything (IPC por `WM_COPYDATA`, sem a DLL), quando está a correr e a pasta está indexada.
5. Estatísticas por extensão, exportação CSV, apagar para a Reciclagem (com confirmação).

## Regras

- Só leitura: nada escreve, move ou apaga no disco analisado (até à fase 5, e aí só para a Reciclagem).
- `unsafe` só para chamadas Win32 (`scan/win.rs`, `scan/ntfs.rs`, `volumes.rs`, `SetErrorMode` no `main`), com
  `SAFETY`. O volume é aberto só para leitura.
- Qualquer string visível nova precisa de linha completa em `strings.txt`.
- Os textos de menu têm de ter teclas de acesso diferentes dentro de cada menu, em todas as línguas (teste com
  `rt_core::menu::access_key_clashes`). A tabela foi gerada por script, mas edita-se à mão: uma linha por texto, colunas separadas por tab.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
