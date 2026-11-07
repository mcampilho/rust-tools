# rsize

Um analisador de espaço em disco portátil para Windows e [Linux](#linux), escrito
em Rust. Lê um disco, uma pasta ou uma partilha de rede e mostra o que ocupa o
espaço: as pastas e os ficheiros por tamanho, dos maiores para os mais pequenos, e
um **treemap** em que cada ficheiro é um retângulo do tamanho do ficheiro.

[English version](README.md)

O rsize faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo correm na
raiz do repositório.

O `rsize.exe` é um único executável de 1,7 MB: sem instalador, sem runtime, sem DLLs
além das que vêm com o Windows. Não faz ligações de rede por iniciativa própria (lê
as partilhas de rede que escolher) e não tem telemetria. Só lê: nunca altera, move
ou apaga nada.

Esta é a versão 0.1, a primeira fase do programa. Ainda faltam: os gráficos de tarte
e de anéis, a leitura direta do NTFS (muito mais rápida, com direitos de
administrador), o uso do índice do [Everything](https://www.voidtools.com/) quando
está a correr, as estatísticas por tipo de ficheiro, a exportação para CSV e apagar
para a Reciclagem.

## Utilização

```text
rsize [pasta]
```

Sem pasta, o rsize mostra os discos, com o sistema de ficheiros e quão cheios estão;
um clique lê o disco. **Escolher pasta** (Ctrl+O) lê qualquer pasta, também na rede:
no diálogo, ou na linha de comandos como `\\servidor\partilha\pasta`. Uma pasta
largada na janela também é lida.

Durante a leitura, a janela mostra quantos ficheiros e pastas já encontrou e o seu
tamanho. **Parar** (Esc) termina a leitura e mostra o que já foi lido; a barra de
estado indica então que o resultado está incompleto.

O resultado tem duas partes, separadas por uma barra que se pode arrastar:

- **A lista**: cada pasta com a sua parte da pasta acima (uma barra e uma
  percentagem), o tamanho, quantos ficheiros contém e a alteração mais recente. As
  pastas abrem-se com a seta, um duplo clique, a seta para a direita ou o Espaço. As
  pastas que não se conseguiram ler, as ligações e as ligações físicas ficam
  assinaladas depois do nome.
- **O treemap**: um retângulo por ficheiro, dentro dos retângulos das suas pastas,
  sombreados em almofada para que as pastas se distingam sem contornos. Os ficheiros
  têm a cor da extensão: as doze extensões que ocupam mais espaço têm uma cor cada,
  as restantes são cinzentas. Apontar para um retângulo mostra o caminho e o tamanho
  na barra de estado; um clique seleciona-o na lista.

O treemap mostra uma pasta de cada vez: um duplo clique desce uma pasta em direção
ao ficheiro clicado, **Enter** mostra a pasta selecionada na lista, **Retrocesso**
sobe uma pasta e **Ctrl+Retrocesso** volta ao topo.

**Ver > Tamanho dos ficheiros / Tamanho em disco** escolhe o tamanho mostrado e
usado para ordenar: o comprimento dos ficheiros, ou o espaço que ocupam no disco
(clusters, compressão, ficheiros esparsos e, em Linux, as próprias pastas).

O menu de contexto da lista e do treemap (botão direito) abre o item, mostra-o no
gestor de ficheiros, copia o caminho ou muda a pasta do treemap.

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
| Retrocesso / Ctrl+Retrocesso | Treemap: uma pasta acima / tudo |
| Ctrl+Enter | Abrir o ficheiro ou a pasta selecionados |
| Ctrl+E | Mostrá-lo no gestor de ficheiros |
| Ctrl+C | Copiar o caminho |
| Ctrl+1 / Ctrl+2 | Tamanho dos ficheiros / tamanho em disco |
| F1 | Atalhos de teclado |

## Como lê o disco

Vários fios leem pastas ao mesmo tempo (até oito), o que conta sobretudo em SSDs e
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

Tempos neste PC, através do sistema: C: (NTFS, 608 000 ficheiros e pastas) em 16 s,
D: (NTFS, 1,9 milhões) em 23 s. A segunda leitura de uma pasta é muito mais rápida,
porque o sistema guarda o que leu.

## Linux

O pacote para Linux é um `tar.gz` com o binário `rsize`, de 5,4 MB. Funciona em X11
e Wayland e precisa da glibc 2.35 ou mais recente.

- **Bibliotecas.** As mesmas das outras ferramentas: `libwayland-client` e
  `libxkbcommon` em Wayland, `libX11`, `libxcursor`, `libxkbcommon` e
  `libxkbcommon-x11` em X11.
- **Discos.** A página inicial mostra os sistemas de ficheiros montados que guardam
  ficheiros (ext4, btrfs, xfs, FAT, exFAT, NTFS, NFS, SMB e outros), não o `/proc`,
  o `tmpfs` e semelhantes. Um dispositivo montado mais de uma vez aparece uma vez.
- **Diálogo de pasta.** Usa o `zenity` ou o `kdialog`. Sem eles, a pasta pode ser
  indicada na linha de comandos.
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
./dev.ps1 test -p rsize
./dev.ps1 clippy -p rsize --all-targets
```

O resultado é `target\release-small\rsize.exe`. O ícone, a informação de versão e o
manifesto (DPI por monitor v2, caminhos longos) são embebidos pelo `build.rs`.

## Testes e verificação

O `./dev.ps1 test -p rsize` corre 32 testes (31 em Linux), sobre:

- a árvore: totais, ordenação pelos dois tamanhos, caminhos, extensões;
- a leitura: uma árvore de pastas com nomes fundos e acentuados, uma raiz que não
  existe, uma leitura interrompida, junções em Windows, ligações simbólicas e físicas
  e pastas sem permissão em Linux;
- o treemap: a disposição squarified preenche a área em proporção, cada pixel
  pertence a um ficheiro, os cliques encontram o ficheiro certo, as cores seguem as
  extensões;
- a lista: abrir pastas, seleção, teclado, zoom;
- tamanhos, contagens, percentagens e datas em várias línguas, definições;
- traduções: todos os textos nas 10 línguas, e todas as páginas desenhadas em alemão
  sem texto por traduzir.

Há mais dois testes a pedido: `screenshots` desenha cada página em
`apps/rsize/target/screens/*.png`, e `bench` mede a leitura da pasta em
`RSIZE_BENCH_DIR`:

```powershell
./dev.ps1 test -p rsize --release --lib screenshots '--' --ignored
$env:RSIZE_BENCH_DIR = 'C:\'; ./dev.ps1 test -p rsize --release --lib bench '--' --ignored --nocapture
```

## Limitações

- Em Windows, um ficheiro com vários nomes (ligações físicas, frequentes em
  `C:\Windows\WinSxS`) conta uma vez por nome, por isso um disco de sistema pode
  mostrar mais do que tem. A leitura direta do NTFS (prevista) vai contá-lo uma vez.
- O espaço em disco das próprias pastas não conta em Windows.
- **Analisar novamente** lê a pasta inteira outra vez; ainda não se pode ler só uma
  pasta do resultado.
- Com centenas de milhares de ficheiros, desenhar o treemap leva uma fração de
  segundo; enquanto a janela é redimensionada, o treemap é esticado e volta a ser
  desenhado quando o redimensionamento para.

## Licença

MIT. Ver [LICENSE](../../LICENSE).
