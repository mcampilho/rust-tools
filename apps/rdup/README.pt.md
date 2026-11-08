# rdup

Um procurador de ficheiros duplicados portátil para Windows e [Linux](#linux),
escrito em Rust. Procura em discos, pastas e partilhas de rede, todos em
conjunto, ficheiros com o mesmo conteúdo, e mostra-os em grupos, primeiro os que
desperdiçam mais espaço; e pastas que são cópias umas das outras, no todo ou em
parte.

[English version](README.md)

O rdup faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo correm na
raiz do repositório.

O `rdup.exe` é um único executável de 2,2 MB: sem instalador, sem runtime, sem DLLs além das
que vêm com o Windows. Não faz ligações de rede próprias (lê as partilhas de rede
que escolher) e não tem telemetria. Nada é movido nem apagado sem o marcar e
confirmar, e mesmo assim só para a Reciclagem ou para uma pasta que escolher;
fica sempre uma cópia de cada grupo.

## Utilização

```text
rdup [pasta...]
```

Sem pastas, o rdup mostra os discos e as pastas acrescentadas antes, cada um com
uma caixa para marcar: as pastas ficam na lista, marcadas ou não, até o seu ×
as tirar, por isso pastas favoritas podem ser procuradas só de vez em quando.
**Adicionar pasta** (Ctrl+O) acrescenta qualquer pasta, já marcada, incluindo
uma na rede (`\\servidor\partilha\pasta`); uma pasta largada na janela também é
acrescentada. **Procurar** marca ficheiros duplicados, pastas
duplicadas ou ambos (sem nenhum, não há nada para procurar e a pesquisa não
começa). **Procurar duplicados** (F5) procura em tudo o que
está marcado ao mesmo tempo, por isso um ficheiro num disco e a sua cópia noutro
são encontrados. Uma pasta dentro de outra escolhida só é lida uma vez. A escolha
fica guardada para a próxima vez.

Durante a procura, a janela mostra o passo em que está, o que já fez e o ficheiro
que está a ler; **Parar** (Esc) termina a procura.

O resultado mostra os grupos de cópias, primeiro os que desperdiçam mais espaço.
Cada grupo tem uma linha de título (o nome, quantas cópias, o seu tamanho e o
espaço que se libertaria guardando só uma) e uma linha por cópia, com a pasta, o
tamanho e a data. **Abrir** (Ctrl+Enter), **Mostrar no gestor de ficheiros**
(Ctrl+E) e **Copiar caminho** (Ctrl+C) atuam sobre a cópia selecionada, também a
partir do menu de contexto. A barra de estado resume tudo: grupos, cópias, espaço
que se pode libertar, ficheiros comparados, tempo e a forma como os discos foram
lidos. **Nova pesquisa** (Ctrl+N) volta aos discos.

### Marcar e mover cópias

Cada cópia tem uma caixa no início da linha: um clique nela, ou Espaço, marca-a
ou desmarca-a. **Marcar cópias...**, por baixo da lista (e o menu **Marcar**),
marca todas as cópias de cada grupo menos uma: **Manter a mais recente**,
**Manter a mais antiga**, **Manter o caminho mais curto** ou **Manter as de uma
pasta...** (as cópias dessa pasta ficam e as outras são marcadas; um grupo sem
nenhuma cópia lá fica como está). **Desmarcar tudo** tira as marcas.

**Mover para a Reciclagem** (Delete) e **Mover para...** (Ctrl+M) tiram as
cópias marcadas do sítio, depois de perguntar, com o número e o tamanho; saem
então da lista, e também os grupos que ficam com uma só cópia. Movidas para uma
pasta, mantêm os nomes (com um número quando o nome já existe); noutro disco
são primeiro copiadas, e o original só é apagado quando a cópia está completa.
A última cópia de um grupo nunca pode ser marcada, seja qual for a regra.
**Exportar para CSV...** (Ctrl+S) grava a lista mostrada, com as marcas.

Nas pastas, só as idênticas podem ser marcadas: uma pasta semelhante tem
ficheiros que a outra não tem. Mover uma pasta tira também as suas cópias dos
ficheiros, e vice-versa.

### Pastas duplicadas

**Pastas** (Ctrl+2), por cima da lista, mostra as pastas que são cópias umas das
outras, e **Ficheiros** (Ctrl+1) volta aos ficheiros. As pastas são comparadas
pelo conteúdo de todos os ficheiros que têm dentro, sejam quais forem os nomes:
são **idênticas** quando cada ficheiro de uma tem a sua cópia na outra e
vice-versa, e semelhantes quando os ficheiros que partilham fazem uma parte da
maior (em bytes). Os botões ao lado escolhem o que é mostrado: **Idênticas**,
**90 % ou mais** (da primeira vez), **75 % ou mais** ou **50 % ou mais**.

As pastas idênticas aparecem juntas, quantas houver, com o espaço que se
libertaria guardando só uma; duas pastas semelhantes, com o que têm em comum. Um
par dentro de outro com tanto ou mais em comum fica de fora: quando `C:\Fotos` e
`D:\Cópia\Fotos` são iguais, `C:\Fotos\2024` e `D:\Cópia\Fotos\2024` também o
são. As pastas idênticas dentro de duas só semelhantes aparecem, por isso cada
percentagem mais baixa mostra tudo o que a mais alta mostra, e mais. **Abrir**, **Mostrar no gestor de ficheiros** e **Copiar caminho** atuam
sobre a pasta selecionada.

Os filtros escolhem os ficheiros que contam numa pasta, mas não o tamanho
mínimo, que só serve para poupar leituras. Por isso, os ficheiros abaixo dele
também são comparados, só nas pastas que podem ser cópias (passo 5, abaixo), e
uma pasta só é idêntica quando todos os seus ficheiros o são.

### Filtros

Os filtros por baixo dos discos escolhem os ficheiros que são comparados; ficam
guardados para a próxima vez, e **Limpar filtros** esvazia-os todos menos o
tamanho mínimo.

- **Tamanho**: o ficheiro mais pequeno e o maior, como `2000` (bytes),
  `500 KB`, `1,5 GB` ou `2 TB`; os botões ao lado escolhem o mais pequeno com
  um clique (qualquer, 4 KB, 64 KB, 1 MB, 10 MB, 100 MB) e esvaziam o máximo, e
  nenhum fica aceso enquanto houver um máximo escrito. O mínimo é 1 MB se não
  o mudar: abaixo de algumas centenas de kilobytes quase todos os ficheiros
  partilham o tamanho com outro, por isso teriam de ser todos abertos. Em três
  discos com 5,3 milhões de ficheiros, 97 % partilham o tamanho com outro
  ficheiro; a partir de 1 MB ficam 30 000 (0,6 %), com 78 % do espaço.
- **Modificado**: de um dia a outro, como `2024-01-31` ou `31/01/2024`, ou um
  mês ou ano inteiro (`2024-06`, `2024`). Os dois dias contam. O botão em cada
  caixa abre um calendário: as setas (ou PgUp e PgDn, ou a roda do rato) mudam
  o mês, um clique num dia escreve-o na caixa, e **Hoje** e **Sem data** fazem
  o que dizem.
- **Tipos**: caixas para imagens, vídeos, áudio, comprimidos, Office,
  programação e sistema; apontar para uma mostra as suas extensões. Marcar uma
  escreve as suas extensões em **Só estas**, e desmarcá-la tira-as; um tipo fica
  marcado enquanto todas as suas extensões lá estiverem, por isso tirar uma à
  mão desmarca-o.
- **Extensões**: só estas (`msi png mp3`), ou todas menos estas
  (`tmp log obj lnk`), com ou sem o ponto. `doc*` vale `doc`, `docx`, `docm`...
- **Nomes**: só os ficheiros cujo nome, ou o nome de uma pasta onde estão,
  corresponde (`Setup*  *Installer*  *video*`), ou todos menos esses
  (`tmp*  *cache*  log*`); uma pasta excluída não é percorrida. `*` vale
  qualquer texto e `?` um carácter; maiúsculas e minúsculas são iguais.

Um filtro que não é percebido fica contornado a vermelho, e a linha ao lado de
**Procurar duplicados** diz porquê; o mesmo acontece a uma extensão ou nome que
esteja ao mesmo tempo para incluir e para excluir, até ser tirado de uma das
caixas. Os ficheiros vazios ficam sempre de fora, e
também, na raiz de um disco, a Reciclagem, a `System Volume Information` e os
ficheiros de sistema do NTFS.

As caixas dos filtros editam texto como é costume: arrastar o ponteiro, duplo
clique numa palavra ou Shift+clique para selecionar; Shift com as setas, Home e
End para selecionar com o teclado, e Ctrl com as setas, Backspace e Delete para
mover ou apagar uma palavra de cada vez; Ctrl+A, Ctrl+C, Ctrl+X e Ctrl+V
selecionam tudo, copiam, cortam e colam. Tab passa à caixa seguinte, Enter
começa a procura, e Esc sai da caixa.

## Como encontra os duplicados

Ler todos os ficheiros para os comparar levaria horas, por isso o rdup compara por
passos, e cada um deixa menos ficheiros para ler:

1. **Ler os discos e as pastas**: só nomes, tamanhos e datas, da forma mais
   rápida que houver (ver abaixo).
2. **Tamanho.** Um ficheiro com um tamanho que nenhum outro tem não pode ter
   cópia. Isto deixa de fora a maioria dos ficheiros sem abrir nenhum.
3. **Primeiros e últimos bytes.** Os ficheiros que restam são abertos, e os seus
   primeiros e últimos 4 KB são resumidos num hash (os ficheiros até 64 KB,
   inteiros). Os ficheiros que só partilham o tamanho separam-se aqui.
4. **Conteúdo.** Os ficheiros que continuam juntos são lidos por inteiro e
   resumidos com BLAKE3, um hash rápido que não dá o mesmo resultado para
   conteúdos diferentes. Os ficheiros com o mesmo hash são cópias.
5. **Pastas.** Para duas cópias de um ficheiro, as suas pastas, e as que estão
   acima delas nível a nível, fazem pares, até onde uma contém a outra. Os pares
   que não podem chegar a 50 % em comum, mesmo com todos os ficheiros mais
   pequenos, ficam de fora; os ficheiros mais pequenos dos outros são comparados
   como nos passos 2 a 4, e depois cada par ficheiro a ficheiro. Um ficheiro com
   mais de 32 cópias não faz pares: diz pouco sobre as pastas onde está.

Os hashes ficam guardados em `hashes.bin`, ao lado das definições, com o
tamanho e a data de cada ficheiro: um ficheiro que não mudou desde então não é
lido outra vez, por isso uma segunda pesquisa dos mesmos discos só lê o que
mudou. As entradas sem uso há 120 dias são apagadas.

Dois nomes do mesmo ficheiro (ligações físicas) não são cópias: o rdup vê que são
o mesmo ficheiro no mesmo volume e conta-o uma vez.

Cada disco é lido à parte, pela ordem das suas pastas. Os primeiros e últimos
bytes são lidos por quatro fios ao mesmo tempo: são leituras pequenas que passam
quase todo o tempo à espera de abrir o ficheiro e de a cabeça chegar, e um disco
mecânico serve várias por uma ordem melhor (numa pasta com 60 000 ficheiros
pequenos de compilação, cinco vezes mais rápido do que um de cada vez). Os
ficheiros inteiros são lidos um de cada vez, para a cabeça não saltar entre eles.

### Leitura dos discos

As mesmas três formas do [rsize](../rsize/README.pt.md), partilhadas pelo crate
`rt-scan`, tentadas por esta ordem:

- **O índice do [Everything](https://www.voidtools.com/)** (Windows), quando o
  Everything está a correr e indexa a pasta com os tamanhos: uma só pergunta
  devolve todos os ficheiros abaixo dela, sem tocar no disco. É também isto que
  torna rápidas as partilhas de rede e os discos USB.
- **A tabela do NTFS lida diretamente** (Windows, como administrador): um disco
  inteiro em segundos. **Ficheiro > Reiniciar como administrador** volta a abrir
  o rdup com esses direitos e a mesma escolha.
- **Pasta a pasta**, através do sistema: qualquer sistema de ficheiros, partilhas
  de rede, Linux.

A barra de estado diz que formas foram usadas, e quando os direitos de
administrador teriam sido mais rápidos.

## Teclado

| Tecla | Ação |
| --- | --- |
| Ctrl+O | Adicionar uma pasta à pesquisa |
| F5 | Procurar os duplicados |
| Ctrl+N | Alterar os discos e as pastas |
| Esc | Parar a pesquisa |
| Tab | Ir para o filtro seguinte |
| Ctrl+1, Ctrl+2 | Mostrar os ficheiros ou as pastas |
| Espaço | Marcar ou desmarcar a cópia selecionada |
| Delete | Mover as cópias marcadas para a Reciclagem |
| Ctrl+M | Mover as cópias marcadas para uma pasta |
| Ctrl+S | Gravar a lista em CSV |
| ↑ ↓ PgUp PgDn Home End | Mover na lista |
| Ctrl+Enter | Abrir o ficheiro selecionado |
| Ctrl+E | Mostrá-lo no gestor de ficheiros |
| Ctrl+C | Copiar o seu caminho |
| F1 | Atalhos de teclado |

## Idiomas e definições

A interface está em dez idiomas (inglês, inglês britânico, alemão, espanhol,
francês, italiano, japonês, português do Brasil e de Portugal, chinês), seguindo o
sistema ou escolhido em **Ver > Idioma da interface**, e num tema claro ou escuro.
As definições ficam em `%APPDATA%\rdup\settings.ini` (`~/.config/rdup` em Linux),
ou ao lado do programa quando existe aí um ficheiro chamado `rdup.portable`.

## Compilar

```powershell
./dev.ps1 build --profile release-small -p rdup   # target\release-small\rdup.exe
./dev.ps1 test -p rdup
```

## Linux

O mesmo programa, com os menus desenhados das outras aplicações do rust-tools. Os
discos são os sistemas de ficheiros montados de facto; a leitura é pasta a pasta,
sem passar para outros sistemas de ficheiros, e as ligações físicas contam uma
vez. Foi verificado no WSL2, não num ambiente de trabalho Linux real.

## A seguir

Primeiras pesquisas mais rápidas em muitos discos grandes, e filtros de pastas
(datas, número de ficheiros).

## Licença

MIT. Ver [LICENSE](../../LICENSE).
