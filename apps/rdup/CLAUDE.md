# rdup — procurador de ficheiros duplicados portátil em Rust

Procura em discos, pastas e partilhas de rede (todos em conjunto) ficheiros com o mesmo conteúdo e mostra-os em
grupos, primeiro os que desperdiçam mais espaço; e pastas que são cópias umas das outras (idênticas ou semelhantes). Um único executável (2,2 MB em Windows), sem rede própria nem telemetria. Só move o que o utilizador
marca e confirma (Reciclagem ou uma pasta), nunca todas as cópias de um grupo. Funcionalidades, atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p rdup
./dev.ps1 test -p rdup         # 38 testes
./dev.ps1 clippy -p rdup --all-targets
./dev.ps1 test -p rdup --release --lib screenshots '--' --ignored   # páginas e ícone em apps/rdup/target/screens/*.png
$env:RDUP_BENCH_DIR = 'C:\|D:\'; ./dev.ps1 test -p rdup --release --lib bench '--' --ignored --nocapture   # RDUP_MIN, RDUP_CACHE
$env:RDUP_BENCH_DIR = 'E:\'; ./dev.ps1 test -p rdup --release --lib edges_rate '--' --ignored --nocapture   # passo 2 com 1/4/16 fios; RDUP_N, RDUP_SKIP, RDUP_MIN
```

Correm na raiz do repositório. Saída: `target\release-small\rdup.exe` (perfil `release-small`).

## Arquitetura (`src/`)

- Leitura dos discos: o crate `rt-scan` (o mesmo do rsize): índice do Everything → MFT do NTFS (administrador)
  → pasta a pasta. `lib.rs` reexporta `format` e `volumes` dele.
- `search.rs` — o motor: `distinct_roots` (normaliza, tira repetidas e pastas dentro de outras), lê cada raiz com
  `rt_scan::scan_with`, junta os ficheiros que o `Filter` deixa (`collect`: sem vazios, sem ligações
  já contadas, sem `$...` e `System Volume Information` na raiz de um disco; pastas excluídas por nome não são
  percorridas), agrupa por tamanho, depois hash BLAKE3 dos primeiros e
  últimos 4 KB (`edges`; até 64 KB, o ficheiro inteiro), depois do conteúdo inteiro (`contents`). Por volume
  (`volume_key`), ficheiros pela ordem da árvore: 4 fios nas pontas, 1 no conteúdo (`workers_per_volume`;
  `RDUP_WORKERS` muda as pontas para medir). Medido em 2026-11-08 em `E:\Projects\AI\Claude` (disco mecânico,
  Everything): 71 217 ficheiros lidos em 0,3 s, 59 649 com tamanho repetido (pastas `target`), pontas a ~15
  ficheiros/s com 1 fio e ~80 com 4, conteúdo 2,9 GB em 78 s; total 243 s, 1 897 grupos, 12 474 ligações físicas
  do cargo reconhecidas. Ligações físicas: identidade (volume + número do
  ficheiro, `identity`) obtida ao abrir; o segundo nome do mesmo ficheiro não é cópia (`Found::links`).
  `start` corre num fio e manda `Event::Progress`/`Done`; `Failure::{Stopped, Unreadable}`. Os passos 1 a 3 estão
  em `compare`, chamado também para os ficheiros pequenos das pastas (`Stage::Folders`). `walk` percorre uma pasta
  com os filtros (sem o tamanho mínimo): os ficheiros que contam numa pasta.
- `cache.rs` — cache de hashes em `hashes.bin` (ao lado das definições): por caminho, com tamanho e data; guarda
  pontas, conteúdo e identidade (ligações físicas). `compare` só lê o que não está na cache ou mudou
  (`Found::cached`). Entradas sem uso há `KEEP_DAYS` (120) dias saem ao gravar; gravada no fim de uma pesquisa
  completa, por um ficheiro `.tmp` e `rename`. Medido em 2026-11-08 em `E:\Projects\AI\Claude` (1 MB): 81,6 s sem
  cache, 0,7 s na segunda vez (3 423 hashes da cache, 215 MB lidos de ficheiros do cargo que mudaram entretanto);
  `hashes.bin` com 536 KB.
- `folders.rs` — pastas duplicadas, depois dos grupos de ficheiros: pares de pastas subindo nível a nível a partir de
  cada par de cópias (ficheiros com mais de `MAX_COPIES` cópias não fazem pares), corte dos pares que não chegam a
  `MIN_PERMILLE` (50 %) mesmo com todos os ficheiros pequenos (`Sums`: bytes por pasta), ficheiros abaixo do tamanho
  mínimo comparados só nessas pastas, e medida exata por conteúdo (`Pair`: ficheiros e bytes em comum, permilagem da
  maior; 1000 = idênticas). `groups(min)` junta as idênticas e esconde um par dentro de outro com permilagem igual ou maior
  (`Pair::parent`), para cada percentagem mais baixa mostrar tudo o que a mais alta mostra.
- `filter.rs` — os filtros: `Choice` (o texto de cada caixa, `Field`, guardado nas definições; um tipo de `KINDS` não
  é guardado à parte: marcá-lo escreve as suas extensões em `OnlyExt`, e está marcado enquanto lá estão todas) → `Choice::filter()` → `Filter` (tamanhos, datas locais, `Patterns` de extensões e nomes com
  `*`/`?`, sem maiúsculas) ou `Bad` (que caixa, porquê; `Why::Both` para a mesma palavra a incluir e a excluir). `KINDS`: os tipos de ficheiro e as suas extensões.
  `parse_size` aceita as unidades em inglês e na língua da interface; `parse_date` aceita ano-mês-dia,
  dia/mês/ano, mês e ano.
- `actions.rs` — `Rule` e `mark_by` (que cópias marcar, ficando uma), `run` (`Op::Recycle` com `rt_sys::recycle`,
  `Op::MoveTo` com `move_into`: nome livre `x (2).ext`, `rename` ou cópia e depois remoção noutro disco), `csv`.
- `app.rs` — estado sem janela: pastas acrescentadas (`saved`, guardadas em `settings.folders`, marcadas ou não;
  `chosen` tem as marcadas e os discos), `compare` (ficheiros, pastas), marcas (`marks`: caminhos; `toggle_mark` recusa a
  última cópia de um grupo e as pastas só semelhantes; `apply_rule`; `marked` nunca devolve um grupo inteiro;
  `removed` tira do resultado o que saiu, desfaz pares de pastas que mudaram e desmarca grupos todos marcados), discos, raízes escolhidas (`chosen`, guardadas em `settings.roots`), filtros e
  a caixa em edição (`focus`, `caret`, `anchor`: seleção por teclado e rato, palavras com Ctrl; cada operação
  corre por `App::edit` num `rt_core::textedit::TextEdit` emprestado, o mesmo da pesquisa do runinst; testado), o calendário aberto (`Calendar`: caixa e mês), pesquisa em curso, resultado e
  linhas da lista (`Row::Group`/`Row::File`, ou `Row::FolderGroup`/`Row::Folder` na vista `View::Folders`, com a
  percentagem `share` guardada em `settings.share`), seleção e deslocamento.
- `ui/view.rs` — as três páginas (discos, pastas e filtros com a barra dos botões fixa em baixo; pesquisa;
  resultado), o calendário por cima (`CalLayout`, semanas a começar ao domingo em en-US, ja e pt-BR) e o
  `Layout` usado também para os cliques (`caret_at` para pôr o cursor numa caixa).
- `main.rs` (com `input.rs`, `menus.rs` e `render.rs`: o mesmo `impl Shell` repartido por assunto; a base da janela, os menus desenhados e o painel de mensagem vêm do `rt-ui`) — janela, menus nativos (Windows) ou desenhados (Linux), rato, teclado, pesquisa em segundo plano
  (eventos com geração; os de pesquisas antigas são ignorados). Modo residente (`--tray`, como o rview e o
  rsize: escondido até pedir; outro arranque passa as pastas com `hand_off` e procura-as, `--new-window` não).
  Menu "Procurar duplicados com o rdup" nas pastas e discos (`rt_sys::win::assoc::register_folders`, só HKCU;
  Ficheiro › Integração com o Windows, `--register`/`--unregister`; o disco chega como `C:"`, `folder_arg`).
  `--help` na consola (`print_to_console`) ou, sem ela, no diálogo da janela.
- `i18n/strings.txt` — traduções (10 idiomas). Os textos da leitura e das unidades estão na tabela comum do
  `rt-core`.

## Fases

1. Feito: crate `rt-scan` (tirado do rsize), escolha de discos e pastas, leitura, duplicados por tamanho + hash
   parcial + hash completo, lista de grupos.
   Antecipado da fase 2 (pedido do utilizador, 2026-11-08): tamanho mínimo na página dos discos
   (`settings::MIN_SIZES`, 1 MB por omissão, hoje a caixa `Field::MinSize`). Medido em C:+D:+E: (5,3 M ficheiros, Everything
   em ~50 s): 97 % partilham o tamanho com outro; ≥ 64 KB: 295 k; ≥ 1 MB: 30 k (78 % do espaço). Teste
   `size_profile` (ignorado) para o repetir.
2. Feito (2026-11-08): filtros de ficheiros — intervalo de tamanhos, de datas, extensões e nomes (de ficheiros
   ou pastas) a incluir ou excluir, tipos de ficheiro. Por fazer: filtros de pastas (datas, número de
   ficheiros) e passar os filtros ao Everything. O Everything 1.5 tem `dupe:size` (e `size:>1mb dupe:size`),
   que devolve só ficheiros com tamanho repetido: a experimentar pelo IPC (`rt_scan::everything`).
3. Feito (2026-11-08): pastas duplicadas, idênticas e semelhantes (≥ 90/75/50 % dos bytes da maior), comparadas
   pelo conteúdo dos ficheiros sem olhar aos nomes, escondendo as subpastas de um par; separadores Ficheiros/Pastas
   (Ctrl+1/Ctrl+2).
4. Feito (2026-11-08): marcar (caixa, Espaço), regras (mais recente, mais antiga, caminho mais curto, numa pasta),
   Reciclagem (Delete) e Mover para (Ctrl+M) num fio à parte depois de confirmar, CSV (Ctrl+S); escolha de
   procurar ficheiros, pastas ou ambos. Cache de hashes feita depois (2026-11-08). Analisado em 2026-11-12 (teste `edges_rate`, ficheiros nunca abertos repartidos
   pelos casos): a primeira pesquisa demora no passo 2 por causa do antivírus. O Bitdefender lê o ficheiro inteiro na
   primeira abertura para leitura (~85 MB/s num disco mecânico); abrir só os atributos é 20 a 100 vezes mais rápido.
   O número de fios quase não conta (discos mecânicos: 14–27 ficheiros/s com 1, 4 ou 16; SSD: 95/389/295 com
   1/4/16, por isso ficam 4). A segunda pesquisa é rápida (cache do antivírus e `hashes.bin`). README: pôr o
   `rdup.exe` nas exceções do antivírus durante uma primeira pesquisa grande. Por experimentar: ler as pontas pela
   MFT (administrador) sem abrir os ficheiros.

## Regras

- Nada é apagado de vez: só Reciclagem ou mover, sempre depois de confirmar, e nunca todas as cópias de um grupo
  (verificado em `toggle_mark`, nas regras e outra vez em `marked`). A confirmação usa os diálogos do sistema,
  que os testes não cobrem.
- `unsafe` só para chamadas Win32 (`search.rs::identity`, `SetErrorMode` no `main`), com `SAFETY`.
- Qualquer string visível nova precisa de linha completa em `strings.txt`; teclas de acesso diferentes dentro de
  cada menu em todas as línguas (teste `access_keys_differ_within_each_menu`).
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
