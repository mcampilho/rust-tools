# rsize

Um analisador de espaço em disco portátil para Windows e [Linux](#linux), escrito
em Rust. Lê um disco, uma pasta ou uma partilha de rede e mostra o que ocupa o
espaço, dispostos como no WizTree: o espaço do disco em cima, as pastas e os
ficheiros à esquerda, o espaço por tipo de ficheiro à direita e um gráfico em baixo:
um **treemap** em que cada ficheiro é um retângulo do tamanho do ficheiro e cada
pasta uma caixa com o seu nome, uma **tarte** com o conteúdo da pasta, ou **anéis**
com as pastas dentro das pastas. Todas as colunas ordenam. Também exporta para CSV e
manda para a Reciclagem o que já não quer.

[English version](README.md)

O rsize faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo correm na
raiz do repositório.

O `rsize.exe` é um único executável de 1,9 MB: sem instalador, sem runtime, sem DLLs
além das que vêm com o Windows. Não faz ligações de rede por iniciativa própria (lê
as partilhas de rede que escolher) e não tem telemetria. Só lê, com uma exceção que
é pedida por si: **Eliminar**, que manda um item para a Reciclagem depois de
perguntar.

## Utilização

```text
rsize [--tray] [--new-window] [pasta]
rsize --register | --unregister
```

Sem pasta, o rsize mostra os discos, com o sistema de ficheiros e quão cheios estão;
um clique lê o disco. **Escolher pasta** (Ctrl+O) lê qualquer pasta, também na rede:
no diálogo, ou na linha de comandos como `\\servidor\partilha\pasta`. Uma pasta
largada na janela também é lida.

`rsize --help` escreve as opções na consola onde foi escrito; arrancado sem
consola (do Executar, por exemplo), o rsize abre e mostra-as na sua janela.

**Modo residente:** `rsize --tray` arranca escondido na área de notificação. Voltar
a arrancar o rsize (com ou sem pasta) ou clicar no ícone mostra a janela de imediato
e lê lá a pasta; fechar a janela só a esconde, e uma leitura continua. Para sair, usa
o menu do ícone ou Ficheiro › Sair. `--new-window` abre uma janela à parte mesmo com
o rsize residente. Para o arrancar com o Windows, coloca um atalho para
`rsize.exe --tray` em `shell:startup`.

**Integração com o Explorador** (Windows): Ficheiro › Integração com o Windows ›
Menu de contexto do Explorador, ou `--register`, acrescenta "Analisar com o rsize"
ao menu do botão direito das pastas e dos discos, e do espaço vazio dentro de uma
pasta; lê essa pasta (no rsize residente, se houver). É escrito só para o
utilizador atual, a apontar para o `rsize.exe` de onde foi ligado; desmarcá-lo, ou
`--unregister`, tira-o.

Durante a leitura, a janela mostra quantos ficheiros e pastas já encontrou e o seu
tamanho. **Parar** (Esc) termina a leitura e mostra o que já foi lido; a barra de
estado indica então que o resultado está incompleto.

O resultado tem quatro partes, numa de duas disposições (**Ver**, F3 e F4, ou os
botões **Pastas** e **Mapa** à direita do bloco de cima):

- **Pastas** (como o WizTree): a lista à esquerda, os tipos de ficheiro à direita,
  o treemap em baixo.
- **Mapa**: o gráfico à esquerda, os tipos de ficheiro à direita, a lista em
  baixo. O gráfico fica com a área maior, e só aqui pode ser a tarte ou os anéis.

As barras entre as partes podem arrastar-se; cada disposição lembra-se das suas.

- **Em cima**, o que foi lido e o espaço do seu volume: a pasta (com o nome do
  volume), quanto tempo a leitura demorou e como (do índice do Everything, da tabela
  do NTFS, pasta a pasta), quantos ficheiros e pastas contém; depois o espaço total
  do volume, o espaço usado (com a percentagem), o espaço reservado e o espaço livre,
  com uma linha que fica vermelha acima de 90 %. **Reservado** é o espaço livre que
  este utilizador não pode ocupar: quotas de disco no Windows, os blocos que o ext4
  guarda para o root em Linux.
- **A lista**: cada pasta com a sua parte da pasta acima (uma barra e
  uma percentagem), o tamanho, o espaço que ocupa no disco (**Alocado**), quantos
  **itens** (ficheiros e pastas), **ficheiros** e **pastas** contém, e a alteração
  mais recente. As pastas abrem-se com a seta, um duplo clique, a seta para a
  direita ou o Espaço. As pastas que não se conseguiram ler, as ligações e as
  ligações físicas ficam assinaladas depois do nome. Com a janela estreita, as
  colunas menos precisas escondem-se primeiro (Pastas, Itens, Alocado).
- **Os tipos de ficheiro**, à direita: os ficheiros da pasta que o gráfico mostra,
  por extensão: a cor que têm no treemap, o nome que o Windows lhes dá ("Ficheiro
  JSON", "Aplicação", na língua do sistema; "Ficheiro EXT" em Linux), a percentagem
  da pasta, o tamanho, o espaço em disco e quantos ficheiros há. Um clique num tipo
  faz sobressair os seus ficheiros no treemap (o resto escurece), na pasta mostrada
  e nas que se abrirem depois; outro clique nele acaba com isso.
- **O gráfico**. No treemap, um retângulo por ficheiro, dentro dos
  retângulos das suas pastas, sombreados em almofada. Os ficheiros têm a cor da
  extensão: as doze extensões que ocupam mais espaço têm uma cor cada, as restantes
  são cinzentas. As pastas com espaço para isso têm um **cabeçalho** com o nome e o
  tamanho (como no SpaceSniffer e no TreeSize), e o encaixe lê-se como
  "Portable — 240 GB › scoop › apps". Apontar para um retângulo mostra o caminho e o
  tamanho na barra de estado; um clique num ficheiro, ou no cabeçalho de uma pasta,
  seleciona-o na lista, que abre as pastas acima dele. Selecionar na lista contorna
  o item no treemap.

**Ordenar.** Um clique no cabeçalho de uma coluna ordena por ela; outro clique
inverte a ordem. A seta mostra a coluna e o sentido. Tamanhos e contagens começam
pelos maiores, nomes de A a Z. Na lista, o conteúdo de cada pasta é ordenado dentro
dela. "Parte da pasta" e "Percentagem" seguem o tamanho escolhido em **Ver**
(tamanho dos ficheiros ou tamanho em disco).

Na disposição Mapa, os separadores por cima do gráfico (ou o menu **Ver**, ou F6 a
F8) escolhem entre três gráficos (F7 e F8 passam para a disposição Mapa):

- **Treemap**, como no WinDirStat ou no WizTree.
- **Tarte**, como no TreeSize ou no JDiskReport: uma fatia para cada item da pasta,
  os doze maiores, com os restantes numa fatia "Outros", e uma legenda com os
  tamanhos e as percentagens.
- **Anéis**, como no Baobab ou no Scanner: a pasta ao centro, o seu conteúdo num
  anel à volta, o conteúdo desses no anel seguinte, até seis níveis, cada item dentro
  do ângulo da sua pasta. A cor segue o ângulo e fica mais clara para fora; os
  ficheiros são mais pálidos do que as pastas. Os itens finos demais para se verem
  ficam de fora.

Na tarte e nos anéis, a fatia sob o ponteiro afasta-se do centro e fica mais clara,
com tudo o que tem dentro, e o caminho e o tamanho aparecem na barra de estado. Um
clique seleciona-a na lista.

O gráfico mostra uma pasta de cada vez, e os tipos de ficheiro seguem-na. No treemap,
um duplo clique no cabeçalho de uma pasta mostra essa pasta, e num ficheiro desce uma
pasta em direção a ele; na tarte e nos anéis, um duplo clique numa pasta
mostra essa pasta, e um clique no centro dos anéis sobe uma pasta. **Enter** mostra a
pasta selecionada na lista, **Retrocesso** ou um **clique direito** no
gráfico sobe uma pasta e **Ctrl+Retrocesso** volta ao topo.

**Ver > Tamanho dos ficheiros / Tamanho em disco** escolhe o tamanho mostrado e
usado para ordenar: o comprimento dos ficheiros, ou o espaço que ocupam no disco
(clusters, compressão, ficheiros esparsos e, em Linux, as próprias pastas).

O menu de contexto (botão direito na lista, ou Shift+botão direito no gráfico) abre
o item, mostra-o no gestor de ficheiros, copia o caminho, muda a pasta do gráfico
ou elimina-o.

**Editar > Eliminar** (Delete) manda o ficheiro ou a pasta selecionados para a
Reciclagem, ou para o lixo do ambiente de trabalho em Linux, depois de mostrar o que é
e quanto contém. Onde não há Reciclagem (uma partilha de rede) ou o item é grande
demais para ela, o Windows pergunta antes de o apagar de vez. A lista, os totais e os
gráficos perdem logo o item, sem voltar a ler o disco.

**Ficheiro > Exportar para CSV** (Ctrl+S) grava num ficheiro CSV a pasta mostrada no
gráfico, com tudo o que tem abaixo: uma linha por pasta e ficheiro, com o caminho, o
tipo, o tamanho, o tamanho em disco, os ficheiros e pastas que contém e a última
alteração. O ficheiro abre-se tal como está nas folhas de cálculo (UTF-8, vírgulas).

### Teclado

| Teclas | Ação |
| --- | --- |
| Ctrl+O | Escolher uma pasta para analisar |
| Ctrl+D | Mostrar os discos |
| F5 | Ler a mesma pasta outra vez |
| Esc | Parar a leitura |
| ↑ ↓ PgUp PgDn Home End | Mover na lista |
| ← → Espaço | Fechar ou abrir uma pasta |
| Enter | Mostrar a pasta selecionada no treemap |
| Retrocesso ou clique direito / Ctrl+Retrocesso | Gráfico: uma pasta acima / tudo |
| Shift+clique direito | O menu do item, no gráfico |
| F3 / F4 | Disposição Pastas / disposição Mapa |
| F6 / F7 / F8 | Treemap / tarte / anéis |
| Ctrl+Enter | Abrir o ficheiro ou a pasta selecionados |
| Ctrl+E | Mostrá-lo no gestor de ficheiros |
| Ctrl+C | Copiar o caminho |
| Ctrl+1 / Ctrl+2 | Tamanho dos ficheiros / tamanho em disco |
| Delete | Mandá-lo para a Reciclagem, depois de perguntar |
| Ctrl+S | Exportar para CSV a pasta mostrada |
| F1 | Atalhos de teclado |

## Como lê o disco

O rsize experimenta três formas, a mais rápida primeiro, e o bloco de cima diz qual
deu o resultado.

**Do índice do [Everything](https://www.voidtools.com/)** (Windows), quando o
Everything está a correr: uma só pergunta ao Everything devolve todos os ficheiros e
pastas abaixo da pasta, com tamanhos e datas, sem tocar no disco. A resposta só é
usada quando parece completa: o Everything tem de ter os tamanhos no índice
(Ferramentas > Opções > Índices, "Indexar o tamanho dos ficheiros") e tem de mostrar o
que a pasta contém no primeiro nível. Caso contrário, o rsize lê a pasta de uma das
outras formas. O rsize fala diretamente com a janela do Everything, como faz o SDK do
próprio Everything, por isso não é preciso mais nada.

**Diretamente do NTFS** (Windows, com direitos de administrador). Quando a pasta está
num volume NTFS local, o rsize abre o volume e lê a sua Master File Table: um registo
por ficheiro e pasta, em blocos grandes e seguidos, como fazem o WizTree e o
Everything. A árvore é depois montada a partir dos registos, por isso um disco inteiro
leva segundos em vez de minutos. Também conta:

- uma só vez cada ficheiro com vários nomes (ligações físicas, frequentes em
  `C:\Windows\WinSxS`);
- o que os ficheiros comprimidos e esparsos ocupam de facto, e os fluxos com nome;
- o espaço do índice de cada pasta;
- os ficheiros de sistema na raiz do volume (`$MFT`, `$LogFile`...).

Sem direitos de administrador o Windows não deixa um programa abrir um volume, por
isso o rsize lê pasta a pasta (abaixo) e a barra de estado diz que seria mais rápido
como administrador. **Ficheiro > Reiniciar como administrador** volta a abrir o rsize
com esses direitos (o Windows pergunta primeiro) e lê a mesma pasta. A barra de estado
diz sempre de que forma o resultado foi lido.

A tabela é lida sem a cache do sistema, por um fio enquanto outro a interpreta. As
sequências longas de registos livres (2 MB ou mais, marcadas no mapa de bits da própria
tabela) são saltadas. O
`rsize --timings <pasta>`, corrido como administrador, lê-a duas vezes (sem e com a
cache) e mostra quanto tempo levaram o disco e a interpretação, para perceber porque é
que um disco é lento.

**Pasta a pasta**, através do sistema: para outros sistemas de ficheiros (FAT, exFAT,
ReFS), partilhas de rede, Linux, e quando não há direitos de administrador. Vários fios
leem pastas ao mesmo tempo (até oito), o que conta sobretudo em SSDs e
partilhas de rede. Em Windows cada pasta é lida com
`GetFileInformationByHandleEx(FileIdBothDirectoryInfo)`, que devolve as entradas com
os tamanhos, o espaço em disco e as datas em lotes de 64 KB, sem abrir nada por
ficheiro. Caminhos com mais de 260 caracteres funcionam. As partilhas que não
suportam esse pedido são lidas com `FindFirstFile`, e aí o espaço em disco é igual
ao comprimento. Em Linux as pastas são lidas com `readdir` e `lstat`.

As ligações simbólicas, as junções e os pontos de montagem aparecem na lista mas não
são seguidos, para que nada seja contado duas vezes e a leitura fique no volume. Em
Linux, as pastas noutro sistema de ficheiros não são lidas (como `du -x`), e um
ficheiro com vários nomes (ligações físicas) conta uma vez.

Tempos neste PC:

| | Pasta a pasta | NTFS direto | Everything |
| --- | --- | --- | --- |
| C: (SSD, cerca de 650 000 ficheiros e pastas) | 16 s | 3 s | 2 a 3 s |
| D: (SSD, 1,9 milhões) | 23 s | 5 s | |
| E:\Projects (disco mecânico, 1,6 milhões) | | | 6 s |

Pasta a pasta, a segunda leitura de uma pasta é muito mais rápida, porque o sistema
guarda o que leu. O Everything precisa de um momento depois de arrancar para carregar
o índice; até lá, o rsize lê das outras formas.

## Linux

O pacote para Linux é um `tar.gz` com o binário `rsize`, de 5,5 MB. Funciona em X11
e Wayland e precisa da glibc 2.35 ou mais recente.

- **Bibliotecas.** As mesmas das outras ferramentas: `libwayland-client` e
  `libxkbcommon` em Wayland, `libX11`, `libxcursor`, `libxkbcommon` e
  `libxkbcommon-x11` em X11.
- **Discos.** A página inicial mostra os sistemas de ficheiros montados que guardam
  ficheiros (ext4, btrfs, xfs, FAT, exFAT, NTFS, NFS, SMB e outros), não o `/proc`,
  o `tmpfs` e semelhantes. Um dispositivo montado mais de uma vez aparece uma vez.
- **Diálogos.** Os diálogos de pasta e de gravação e a pergunta antes de eliminar
  usam o `zenity` ou o `kdialog`. Sem eles, a pasta pode ser indicada na linha de
  comandos, mas não se pode eliminar nem exportar.
- **Lixo.** Eliminar usa o `gio trash` quando existe; senão, move o item para o lixo
  do utilizador (`~/.local/share/Trash`), que só aceita itens no mesmo sistema de
  ficheiros da pasta pessoal.
- **Menus.** São desenhados na janela, com os mesmos itens e atalhos. Abrem-se com
  Alt+letra ou F10.
- **Nomes.** Os nomes de ficheiro que não são UTF-8 válido aparecem com caracteres
  de substituição; esses ficheiros contam, mas abri-los a partir do rsize pode falhar.

Verificado no WSL2 em X11: os testes, a leitura de `/usr` (58 000 ficheiros e pastas
em 0,9 s) e o arranque da janela com uma pasta. O diálogo de pasta e o Wayland não
foram experimentados.

## Compilar

Requisitos: Rust 1.88+ com o alvo MSVC e as MSVC Build Tools.

```powershell
cargo build --profile release-small -p rsize
# ou, com as Build Tools portáteis:
./dev.ps1 build --profile release-small -p rsize
./dev.ps1 test -p rsize -p rt-scan
./dev.ps1 clippy -p rsize --all-targets
```

O resultado é `target\release-small\rsize.exe`. O ícone, a informação de versão e o
manifesto (DPI por monitor v2, caminhos longos) são embebidos pelo `build.rs`.

## Testes e verificação

O `./dev.ps1 test -p rsize -p rt-scan` corre 56 testes em Windows (alguns só lá),
sobre:

- a árvore: totais, ordenação pelos dois tamanhos, caminhos, extensões;
- o espaço do volume onde está uma pasta;
- a leitura: uma árvore de pastas com nomes fundos e acentuados, uma raiz que não
  existe, uma leitura interrompida, junções em Windows, ligações simbólicas e físicas
  e pastas sem permissão em Linux;
- o leitor de NTFS, sobre um volume de 8 MB feito pelo `mkntfs`: o setor de arranque,
  as listas de extensões, as sequências de atualização, ficheiros residentes e não
  residentes, um fluxo com nome, um nome acentuado, uma pasta abaixo da raiz, o
  `$BadClus`, a interrupção;
- as respostas do Everything: a leitura, a árvore montada a partir dos caminhos (sem
  distinguir maiúsculas), e a recusa de um índice sem a pasta ou sem os tamanhos; com
  o Everything a correr, a mesma pasta lida das duas formas;
- o treemap: a disposição squarified preenche a área em proporção, cada pixel
  pertence a um ficheiro ou à moldura de uma pasta, os cliques encontram o ficheiro
  certo, as pastas com espaço têm um cabeçalho que as seleciona e o conteúdo por baixo
  dele, as cores seguem as extensões, um tipo escolhido sobressai;
- a tarte e os anéis: as fatias cobrem o círculo em proporção, os itens pequenos vão
  para "Outros", os anéis ficam dentro das suas pastas, os cliques encontram o
  segmento certo;
- a lista: abrir pastas, seleção, teclado, zoom, ordenação por todas as colunas nos
  dois sentidos; as duas disposições guardam as suas barras; a tabela de tipos de ficheiro: ordenação por todas as colunas, os
  nomes dos tipos, seguir a pasta que o gráfico mostra;
- eliminar: o item sai da pasta, os totais e os totais por extensão diminuem, e o
  gráfico sobe quando mostrava o que foi eliminado; a exportação CSV (ordem, aspas,
  colunas); em Linux, a pasta do lixo;
- tamanhos, contagens, percentagens e datas em várias línguas, definições;
- traduções: todos os textos nas 10 línguas, nenhum par de itens do mesmo menu com a
  mesma tecla de acesso, e todas as páginas e gráficos desenhados em alemão sem texto
  por traduzir.

Há mais três testes a pedido: `screenshots` desenha cada página em
`apps/rsize/target/screens/*.png`, `bench` mede a leitura da pasta em
`RSIZE_BENCH_DIR` (da forma mais rápida disponível), e `probe` mostra o que um
Everything a correr responde para ela:

```powershell
./dev.ps1 test -p rsize --release --lib screenshots '--' --ignored
$env:RSIZE_BENCH_DIR = 'C:\'; ./dev.ps1 test -p rsize --release --lib bench '--' --ignored --nocapture
./dev.ps1 test -p rsize --lib probe '--' --ignored --nocapture
```

## Limitações

- Lido pasta a pasta em Windows, um ficheiro com vários nomes (ligações físicas)
  conta uma vez por nome, por isso um disco de sistema pode mostrar mais do que tem,
  e o espaço em disco das próprias pastas não conta. A leitura direta do NTFS não tem
  nenhum destes problemas.
- A leitura direta do NTFS lê a tabela do volume inteiro, mesmo para uma só pasta;
  para uma pasta pequena num disco grande, pasta a pasta pode ser mais rápido.
- A partir do índice do Everything, o espaço em disco é estimado (tamanhos
  arredondados a clusters inteiros), um ficheiro com vários nomes conta uma vez por
  nome, e faltam as pastas que o Everything foi configurado para excluir. O Everything
  também pode estar uns segundos atrasado em relação ao disco.
- **Analisar novamente** lê a pasta inteira outra vez; ainda não se pode ler só uma
  pasta do resultado.
- Os cabeçalhos das pastas no treemap ocupam parte da área, por isso as áreas deixam
  de estar exatamente em proporção entre pastas: o conteúdo de uma pasta reparte o
  que sobra por baixo do seu cabeçalho. As pastas pequenas demais não têm cabeçalho.
- Com centenas de milhares de ficheiros, desenhar o treemap leva uma fração de
  segundo; enquanto a janela é redimensionada, o treemap é esticado e volta a ser
  desenhado quando o redimensionamento para.

## Licença

MIT. Ver [LICENSE](../../LICENSE).
