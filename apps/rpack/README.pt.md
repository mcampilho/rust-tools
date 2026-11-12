# rpack

Um arquivador portátil para Windows e [Linux](#linux), escrito em Rust. Navega
pelos discos e pastas como um gestor de ficheiros; cria arquivos 7z, zip e tar
(sozinhos ou comprimidos com gzip, xz, zstd, bzip2, lz4 ou lzip); abre-os, e os
ficheiros comprimidos inteiros, pasta a pasta; testa-os e extrai tudo ou parte;
e calcula os hashes dos ficheiros (CRC, MD5, SHA-1, SHA-2, SHA-3, BLAKE2sp,
BLAKE3...).

[English version](README.md)

O rpack faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo
correm na raiz do repositório.

O `rpack.exe` é um único executável de 4,8 MB: sem instalador, sem runtime, sem
DLLs além das que vêm com o Windows. Não faz ligações à rede e não tem
telemetria. Está a ser feito por fases: palavras-passe, rar, iso, cab, zpaq e o
menu do Explorador vêm a seguir (ver [A seguir](#a-seguir)).

## Utilização

```text
rpack [arquivo | pasta]
```

Um arquivo na linha de comandos abre logo; uma pasta é mostrada. `rpack --help`
escreve isto na consola onde foi escrito; aberto sem consola, o rpack mostra-o
na sua janela.

O rpack abre como um navegador de ficheiros, na última pasta mostrada (da
primeira vez, a pasta do utilizador). O caminho por cima da lista volta a
qualquer pasta dele, até **Este PC** com os discos (tamanho, espaço livre,
sistema de ficheiros); Retrocesso ou a seta à esquerda sobe um nível, F5 lê a
pasta de novo. Primeiro as pastas, depois os ficheiros, ordenados por qualquer
coluna (um clique no título; outro para inverter): nome, tamanho, data e tipo.
Os arquivos têm um ícone próprio.

Enter (ou um duplo clique) abre uma pasta, um arquivo, ou qualquer outro
ficheiro com o seu programa. Um arquivo é reconhecido pelos primeiros bytes,
seja qual for o nome: um zip com o nome mudado para `.dat`, ou um `.tar.gz` sem
extensão, abrem na mesma; e também um zip dentro de um programa autoextraível.
Só o lzma e o brotli, que não têm marca própria, precisam da extensão (`.lzma`,
`.br`). **Abrir arquivo** (Ctrl+O) abre um a partir de um diálogo, e um arquivo
largado na janela também abre.

Dentro de um arquivo a navegação continua: as suas pastas (feitas a partir dos
caminhos quando o arquivo não as guarda), com tamanho, tamanho comprimido,
data e método; uma pasta mostra o tamanho de tudo o que tem dentro. Subir a
partir da raiz do arquivo volta à pasta onde ele está (também **Fechar
arquivo**, Ctrl+W). Enter num ficheiro abre-o com o seu programa, depois de o
extrair para uma pasta temporária que é esvaziada quando o rpack fecha. Uma
entrada encriptada tem um `*`.

Clique, Ctrl+clique e Shift+clique selecionam linhas; Ctrl+A seleciona a pasta
toda. A barra de estado mostra a linha apontada ou selecionada (caminho,
tamanhos, CRC), e os totais da pasta ou do arquivo (formato, ficheiros, quanto
foi comprimido).

**O que se pode fazer com as linhas selecionadas** está na barra por baixo da
lista (**Adicionar**, **Hashes**, **Testar**, **Extrair**), no menu Arquivo e
no menu do botão direito, como no 7-Zip ou no PeaZip:

- no disco: Abrir; para arquivos, Extrair (cada um ao lado de si), Extrair
  aqui, Extrair para "*nome*" (uma pasta com o nome dele), Extrair para uma
  pasta, Testar; Adicionar a um arquivo, Adicionar a "*nome*.7z", Adicionar a
  "*nome*.zip", Adicionar cada um a um arquivo individual; Calcular hashes;
  Mostrar no gestor de ficheiros, Copiar caminho, Mover para a Reciclagem
  (Delete);
- dentro de um arquivo: Abrir, Extrair, Extrair para uma pasta, Calcular
  hashes, Copiar caminho.

### Adicionar a um arquivo

**Adicionar** (Ctrl+Shift+A) abre uma janela para os ficheiros e pastas
selecionados:

- **Nome** do arquivo, escrito, e **Pasta** para onde vai (a que está mostrada,
  ou outra com Alterar). O nome nunca é o de um ficheiro que já lá está:
  `fotos (2).7z`.
- **Formato**: 7z, zip, tar, tar.gz, tar.xz, tar.zst, tar.bz2, tar.lz4, tar.lz,
  e, para um só ficheiro, gz, xz, zst, bz2 ou lz4. A extensão do nome
  acompanha.
- **Nível**: Armazenar, Mais rápido, Rápido, Normal, Máximo ou Ultra (no 7z e no
  xz, os níveis 0 a 9 do 7-Zip e do xz; no zstd, os seus níveis 1 a 19).
- **Método**, no zip: Deflate (todos os programas o abrem), Zstandard, XZ ou
  BZip2.
- **Sólido** (7z, ligado por omissão): os ficheiros são comprimidos juntos em
  blocos de até 256 MB, o que dá arquivos mais pequenos; desligado, cada
  ficheiro sozinho.
- **Cada item num arquivo próprio**, com vários selecionados: cada um com o seu
  nome, ao lado dele.

As escolhas ficam para a próxima vez. Adicionar a "*nome*.7z" e "*nome*.zip"
criam o arquivo logo com elas, e Adicionar cada um a um arquivo individual
também.

Uma pasta entra com tudo o que tem dentro, com o nome a partir da pasta onde
está (`fotos/...`), com as datas dos ficheiros (e, no Linux, as permissões). As
ligações ficam de fora. Um ficheiro que não se consegue ler fica de fora e é
indicado no relatório. O arquivo é escrito com um nome temporário e só fica com
o seu nome quando está completo: parar, ou um erro, não deixa nada para trás. O
7z e o xz usam todos os núcleos do processador.

### Extrair

**Extrair** (Ctrl+E) extrai as linhas selecionadas, com tudo o que está dentro
das pastas selecionadas, ou o arquivo todo quando nada está selecionado. Os
ficheiros selecionados dentro de uma pasta do arquivo são escritos sem o
caminho dessa pasta: extrair `img` de dentro de `docs` escreve `img`, não
`docs/img`.

Vão para a pasta mostrada ao lado de **Extrair para**: por omissão, uma pasta
com o nome do arquivo (`fotos.tar.gz` → `fotos`) ao lado dele, ou a própria
pasta do arquivo quando está tudo dentro de uma pasta ou o arquivo tem um só
ficheiro, para não haver uma pasta dentro de outra com o mesmo nome. **Alterar**
escolhe outra pasta; **Extrair para uma pasta** (Ctrl+Shift+E) escolhe uma e
extrai logo. A pasta é criada se não existir.

**Numa pasta**, os arquivos selecionados podem ser extraídos ou testados todos
de uma vez, cada um à sua parte: ao lado de si, como acima (Extrair), na pasta
onde está (Extrair aqui), numa pasta com o nome dele (Extrair cada um para uma
pasta nova), ou numa pasta escolhida (Extrair para uma pasta); um relatório diz
como correu cada um.

Arquivo › Se o ficheiro já existir escolhe o que acontece a um ficheiro que já
lá está: **Manter ambos** (por omissão: o novo fica `nome (2).ext`),
**Substituir** ou **Ignorar**. As datas mantêm-se, e no Linux também as
permissões. Com Arquivo › Abrir a pasta depois de extrair (ligado por omissão),
a pasta abre no gestor de ficheiros no fim.

Nada é escrito fora da pasta escolhida, diga o arquivo o que disser: `..` e
letras de disco saem dos caminhos, e os nomes que o Windows não aceita
(`con.txt`, `o quê?.txt`, nomes acabados em ponto) são mudados. As ligações
(simbólicas e físicas, nos arquivos tar) não são extraídas, porque uma ligação
podia levar os ficheiros seguintes para fora da pasta; o relatório conta-as.
Um ficheiro que não se consegue ler (danificado ou encriptado) é indicado e
deixado de fora, nunca escrito a meio.

### Hashes

**Hashes** (ou Arquivo › Calcular hashes) calcula CRC-32, CRC-64, XXH64, MD5,
SHA-1, SHA-256, SHA-384, SHA-512, SHA3-256, BLAKE2sp ou BLAKE3, ou todos de uma
vez, dos ficheiros e pastas selecionados no disco (todos os ficheiros dentro de
uma pasta), ou dos ficheiros selecionados dentro de um arquivo (todos, quando
nada está selecionado). Cada ficheiro é lido uma só vez, seja qual for o número
de hashes. A janela mostra os primeiros ficheiros; Ctrl+C copia-os todos, e
**Guardar hashes** escreve-os como o `sha256sum` e afins os escrevem (`hash
*nome`, um ficheiro por linha), para os poder verificar com essas ferramentas.
O CRC-64 é o do xz e do 7-Zip; os CRC e o XXH64 aparecem em maiúsculas, como o
7-Zip os mostra.

### Testar

**Testar** (Ctrl+T) lê todos os ficheiros até ao fim e verifica o que o formato
verifica: o CRC de cada entrada zip e 7z, as somas de verificação dos fluxos xz,
gzip, bzip2, lzip, lz4 e zstd, e que cada ficheiro tem o tamanho que o arquivo
diz. Depois mostra os erros, se houver.

Abrir, adicionar, extrair, testar e calcular hashes correm em segundo plano: a
janela mostra até onde chegou, o ficheiro que está a ler e o tempo que passou.
**Parar** (Esc) termina; um ficheiro a meio é apagado.

## Formatos

| Formato | Lê | Escreve | Métodos |
| --- | --- | --- | --- |
| 7z | sim | sim | Lê LZMA, LZMA2, BZip2, PPMd, os filtros BCJ, blocos sólidos; escreve LZMA2 (sólido ou não) ou Store |
| zip, jar, apk, docx… | sim, também zip64 | sim | Lê Store, Deflate, Deflate64, BZip2, LZMA, XZ, Zstandard, PPMd, e os antigos Shrink, Reduce e Implode; escreve Store, Deflate, Zstandard, XZ, BZip2 |
| tar | sim | sim | ustar, GNU (nomes longos, esparsos), pax |
| gz / tgz | sim | sim | gzip (vários membros como um) |
| bz2 / tbz2 | sim | sim | bzip2 |
| xz / txz | sim | sim | xz (todas as verificações, vários fluxos) |
| lzma / tlzma | sim | não | lzma (o antigo formato `.lzma`) |
| lz / tlz | sim | tar.lz | lzip |
| zst / tzst | sim | sim | Zstandard, janelas até 2 GB (`--long`) |
| lz4 / tlz4 | sim | sim | tramas LZ4 |
| br / tbr | sim | não | Brotli (escrevê-lo tornaria o rpack 1 MB maior) |

Um ficheiro comprimido com um tar dentro abre como tar. Um tar comprimido só se
lê desde o início, por isso listar um `.tar.xz` lê-o todo, e extrair parte dele
lê-o até ao último ficheiro pedido. Um bloco sólido de 7z é igual: extrair um
ficheiro lê os que estão antes dele no bloco.

Os nomes nos zip não marcados como UTF-8 são lidos na antiga página de código
do DOS (437), como o formato zip diz; arquivos feitos por programas antigos do
Windows noutras línguas podem mostrar mal algumas letras acentuadas. Os nomes
escritos pelo rpack são sempre UTF-8.

As entradas zip e 7z com palavra-passe aparecem na lista mas ainda não são
extraídas, e ainda não se podem criar arquivos com palavra-passe (fase 2).

## Teclado

| Tecla | Faz |
| --- | --- |
| Enter | Abrir a pasta, o arquivo ou o ficheiro selecionado |
| Retrocesso | Voltar à pasta acima |
| F5 | Ler a pasta de novo |
| Ctrl+O | Abrir um arquivo |
| Ctrl+W | Fechar o arquivo |
| Ctrl+A | Selecionar tudo na pasta |
| Ctrl+Shift+A | Adicionar a seleção a um arquivo |
| Ctrl+E | Extrair a seleção, ou tudo |
| Ctrl+Shift+E | Extrair para uma pasta à escolha |
| Ctrl+T | Testar o arquivo |
| Delete | Mover a seleção para a Reciclagem (depois de perguntar) |
| Esc | Parar |
| ↑ ↓ PgUp PgDn Home End | Mover na lista (Shift seleciona) |
| F1 | Atalhos de teclado |

Na janela Adicionar: escrever muda o nome, Enter cria o arquivo, Esc fecha a
janela.

## Definições

As definições (tema, língua, tamanho da janela, o que fazer aos ficheiros que
já existem, a última pasta mostrada, como se criam os arquivos) ficam em
`%APPDATA%\rpack\settings.ini` no Windows e em `~/.config/rpack/settings.ini`
no Linux. Com um ficheiro chamado `rpack.portable` ao lado do `rpack.exe`,
ficam aí (modo portátil).

## Compilar

```powershell
./dev.ps1 build --release -p rpack
./dev.ps1 test -p rpack
./dev.ps1 clippy -p rpack --all-targets
```

O resultado é `target\release\rpack.exe`. O rpack usa o perfil `release`, que
desenrola os pânicos: um descodificador que entre em pânico com um arquivo
danificado termina essa tarefa, não o programa.

## Linux

As mesmas funções, com os menus desenhados e os diálogos de ficheiros do zenity
ou do kdialog; Mover para o lixo em vez da Reciclagem. Verificado no WSL2, não
num ambiente de trabalho Linux real.

## A seguir

1. Feito: um navegador de ficheiros; ler, testar e extrair zip, 7z, tar e os
   ficheiros comprimidos, um arquivo ou vários; hashes.
2. Em parte feito: criar arquivos 7z, zip e tar e ficheiros comprimidos, com a
   janela Adicionar e o menu do botão direito. Por fazer: palavras-passe (AES no
   zip e no 7z, para ler e escrever), tamanho do dicionário e número de fios,
   dividir em volumes, acrescentar a um arquivo que já existe.
3. Só leitura: rar (RAR4 e RAR5, volumes, palavras-passe) com o código do
   UnRAR, imagens iso (ISO 9660, Joliet e UDF), cab.
4. zpaq (com as suas versões), Lizard, lzop, e Zstandard, LZ4, Brotli e Lizard
   dentro de arquivos 7z como o 7-Zip ZS os escreve.
5. Integração com o Windows: menu do Explorador (extrair aqui, extrair para uma
   pasta, adicionar ao arquivo, testar), tipos de ficheiro, arrastar ficheiros
   para fora da janela, comandos na linha de comandos.
