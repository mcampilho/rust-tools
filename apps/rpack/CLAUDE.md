# rpack — arquivador portátil em Rust

Navegador de ficheiros que cria arquivos 7z, zip e tar (com os compressores), abre arquivos zip, 7z e tar e ficheiros comprimidos inteiros (gzip, bzip2, xz, lzma, lzip, zstd,
lz4, brotli), mostra-os pasta a pasta, testa-os e extrai tudo ou parte (um arquivo ou vários), e calcula hashes
(CRC-32, CRC-64, XXH64, MD5, SHA-1, SHA-2, SHA3-256, BLAKE2sp, BLAKE3). Um único executável (4,8 MB em Windows, 9,3 MB em Linux), sem rede nem telemetria.
Funcionalidades, atalhos e limitações estão no `README.md`. Objetivo de tamanho: abaixo de 10 MB, de preferência
abaixo de 5 MB (medir depois de cada biblioteca nova).

## Comandos

```powershell
./dev.ps1 build --release -p rpack
./dev.ps1 test -p rpack         # 37 testes
./dev.ps1 clippy -p rpack --all-targets
./dev.ps1 test -p rpack --lib screenshots '--' --ignored   # páginas em apps/rpack/target/screens/*.png
```

Correm na raiz do repositório. Saída: `target\release\rpack.exe`. Perfil `release` (desenrola): um descodificador
que entre em pânico com um arquivo danificado termina a tarefa (`catch_unwind` em `Shell::start` →
`UserEvent::Failed`), não o programa.

## Arquitetura (`src/`)

- `archive/` — o modelo comum: `Listing` (formato, `Entry` com caminho `/`, `Kind`, tamanhos, data, método, CRC,
  modo, ligação), `open` (lista) e `each` (lê o conteúdo das entradas pedidas, pela ordem do arquivo, para
  `extract` e `test`). `Meter`: contadores atómicos (lido do arquivo, escrito, entradas, a atual) que a janela lê
  enquanto a tarefa corre, e o pedido de parar. `Metered` lê o ficheiro contando e devolve o erro `Stop` quando
  pedem para parar (não `Interrupted`, que os leitores repetem para sempre); `open`/`each` transformam qualquer
  erro depois de parar em `Error::Stopped`.
  - `detect.rs` — o formato pelos primeiros bytes (zip, marcas dos compressores, cabeçalho tar com `ustar` ou soma
    certa); lzma e brotli pela extensão. Desconhecido: tenta zip (autoextraíveis).
  - `codec.rs` — `Codec`, os descodificadores (fluxos concatenados lidos como um; zstd com janela até 2^31) e o
    tamanho do conteúdo quando o fluxo o guarda (gzip ISIZE, índice xz, cabeçalho lzma/zstd/lz4, fim dos membros
    lzip). `inner_name`: `a.txt.gz` → `a.txt`.
  - `zip.rs` — crate `zip` (sem as features por omissão; sem AES ainda). Nomes de métodos pelo número do formato
    (`code` com `#[allow(deprecated)]`). Datas DOS em hora local (`rt_scan::format::day_start`).
  - `sevenz.rs` — 7z (`sevenz-rust2`, sem AES por agora); `each` liga as entradas pelo nome (fila por nome para
    repetidos); um bloco sólido lê-se desde o início.
  - `tar.rs` — crate `tar`, simples ou dentro de um codec; `each` para depois da última entrada pedida.
- `extract.rs` — `safe_path` (sem `..`, letras de disco, raiz; nomes que o Windows recusa mudados, `CON` → `_CON`),
  `Job` (destino, máscara de entradas, `strip` = pasta mostrada, `Overwrite`), `extract` e `test` → `Report`.
  Ligações e "outros" nunca são extraídos (contados em `left_out`). Erro a ler uma entrada: apaga o ficheiro a meio
  e regista; erro a escrever (disco cheio): para tudo. Tar comprimido que parte a meio: o feito fica e a quebra é
  mais um erro (`broken_stream`).
- `create.rs` — `Kind` (7z, zip, tar, tar.<codec>, um ficheiro comprimido), `Level` (Store…Ultra → 0–9, zstd 1–19),
  `ZipMethod`, `Spec`; `gather` (ficheiros e pastas, nomes a partir da pasta onde estão, ligações de fora),
  `suggested_name`, `create` (escreve `.<nome>.rpack-part` e só no fim muda o nome; parar ou erro apagam).
  7z: LZMA2 com fios (`from_level_mt`), sólido em blocos de 256 MB (`Lazy` abre cada ficheiro na sua vez); xz com
  `XzWriterMt` (precisa de `block_size`). Escrever brotli ficou de fora: o codificador pesa 1 MB.
- `hash.rs` — `Algo` (11 hashes), `Hasher` (todos de uma vez, cada ficheiro lido uma vez; CRC-64 = CRC_64_XZ),
  `disk` (ficheiros e pastas, recursivo), `entries` (dentro de um arquivo), `sum_file` (formato `hash *nome`).
  Valores de referência nos testes. Por isso o `Cargo.toml` da raiz já não compila sha2, md-5 e brotli-decompressor
  para tamanho.
- `tree.rs` — pastas a partir dos caminhos (`Node`: pastas, ficheiros, entrada própria, totais), `trail`,
  `mark_below`, `single_top` (tudo numa pasta → extrair ao lado do arquivo).
- `app.rs` — estado sem janela: `Place` (discos, pasta no disco, dentro do arquivo), `DiskItem`, arquivo aberto
  (`Opened`) e pasta dele, `Row` (Up, Dir, File, Disk) ordenadas (pastas primeiro; colunas por lugar), `trail`
  (caminho de Este PC até à pasta do arquivo), `up_target` (da raiz do arquivo volta à pasta dele), seleção,
  destino (`dest_for`: pasta com o nome do arquivo, ou ao lado se trouxer pasta própria; `stem` tira `.tar.gz`),
  `job`, `wanted`, `Busy` (tarefa, `Meter`, total, por bytes lidos ou escritos).
- `extract.rs` também tem `extract_each`/`test_each`: vários arquivos selecionados numa pasta, cada um no seu lugar
  (`EachDest`: ao lado com nome inteligente, aqui, numa pasta com o nome, numa pasta escolhida).
- `AddForm` (em `app.rs`): a janela "Adicionar ao arquivo" (nome com cursor, pasta, formato, nível, método zip,
  sólido, cada um à parte); o nome sugerido é sempre livre (`free_path`), e o destino nunca é um dos ficheiros a
  comprimir.
- Menus: `commands::disk_menu` (botão direito no disco, com nomes reais em `M::Dyn`) e `item_menu` (dentro do
  arquivo); no Windows com submenus por `rt_sys::win::menu::popup_at_pointer`.
- `ui/view.rs` — duas páginas (navegador com caminho, lista, barra Hashes/Testar/Extrair; trabalho com barra de
  progresso) e `Layout` para os cliques. `type_name`: o nome do tipo de ficheiro do Windows, guardado por extensão.
- `main.rs` (com `input.rs`, `menus.rs`, `render.rs`; base da janela, menus desenhados e mensagem do `rt-ui`) —
  tarefas em fios (`start`, eventos com geração), discos lidos num fio (`load_drives`), abrir um ficheiro da lista
  (extraído para `%TEMP%\rpack-<pid>\<n>`, apagado ao sair), hashes (Ctrl+C na mensagem copia todos;
  `dialog_copy`), arranque na última pasta (`settings.folder`), na pasta do utilizador ou nos discos.
- `i18n/strings.txt` — traduções (10 línguas).

## Fases

1. Feito (2026-11-12): ler, testar e extrair zip, tar e os 8 compressores; navegação, seleção, destino,
   o que fazer a ficheiros existentes. Depois de o utilizador experimentar: navegador de ficheiros em vez da
   página inicial (como o 7-Zip, o PeaZip, o NanaZip), extrair e testar vários arquivos, e 11 hashes. Depois:
   ler 7z, criar arquivos (janela Adicionar, Adicionar a "x.7z"/"x.zip", cada um à parte), menu do botão direito
   completo, Mostrar no Explorador, Copiar caminho, Mover para a Reciclagem.
2. Criar: 7z (`sevenz-rust2`) e zip com AES, ficheiros comprimidos; janela "Adicionar ao arquivo"; palavras-passe
   para ler (precisa de uma caixa de texto).
3. Só leitura: rar (`unrar`, código da RARLAB: só descomprimir), iso (ISO 9660, Joliet, UDF: código próprio), cab
   (`cab` + `lzxd`).
4. zpaq (`zpaq_rs`, libzpaq em C++; o formato com versões é próprio), Lizard (código C copiado para o repositório, BSD), lzop
   (`lzokay-native` + contentor próprio), métodos extra no 7z como o 7-Zip ZS.
5. Menu do Explorador, tipos de ficheiro, arrastar para fora, linha de comandos (`x`, `a`, `t`).

## Regras

- Nada é escrito fora do destino: todos os caminhos passam por `safe_path`; ligações não são extraídas.
- Um ficheiro nunca fica escrito a meio (erro ou parar → apagado).
- Qualquer string visível nova precisa de linha completa em `strings.txt`; teclas de acesso diferentes dentro de
  cada menu em todas as línguas (teste `access_keys_differ_within_each_menu`).
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
