# rdup-cli

*Leia isto em [inglês](README.md).*

Que ficheiros têm o mesmo conteúdo? O gémeo de linha de comandos do [rdup](../rdup/README.pt.md),
com as opções do `fdupes`. Pesquisa em conjunto todas as pastas e discos que lhe indicar, com as
subpastas, e lista as cópias em grupos, primeiro os que desperdiçam mais espaço. Lê o disco como o
rdup (a tabela de ficheiros do NTFS como administrador ou através do serviço opcional [rtsvc](../rtsvc/README.pt.md), o índice do [Everything](https://www.voidtools.com)
quando está a correr, ou pasta a pasta) e compara os ficheiros pelo tamanho, depois pelos primeiros e
últimos bytes e por fim por hashes BLAKE3 do conteúdo inteiro. Um único executável de 0,8 MB para
Windows e Linux; mensagens em inglês, português e espanhol; sem rede nem telemetria. **Só lista: nunca
apaga, move nem liga nada.**

## Utilização

```bat
rdup-cli D:\Fotos E:\Copia             :: as cópias, um ficheiro por linha, uma linha em branco entre grupos
rdup-cli -S --min-size 1MB D:\         :: com o tamanho de cada grupo, ficheiros de 1 MB ou mais
rdup-cli -m C:\Users                   :: só quantas cópias há e o espaço que ocupam
rdup-cli -f D:\Fotos > extras.txt      :: sem o primeiro de cada grupo: os ficheiros que podia remover
rdup-cli --ext jpg,png --json D:\ > copias.json
```

Os caminhos aparecem como escreveu a pasta, mais o que está dentro dela. Os ficheiros vazios nunca são cópias,
nem os nomes de um mesmo ficheiro (ligações físicas), a não ser com `-H`.

### Opções

| Opção | O que faz |
|---|---|
| `-r`, `-R`, `-n` | Aceites, como no fdupes, e não mudam nada: as subpastas são sempre pesquisadas e os ficheiros vazios nunca são comparados. |
| `-S`, `--size` | O tamanho dos ficheiros de cada grupo, antes deles. |
| `-h`, `--human-readable` | Tamanhos como `1.5K`, `23M`, `4.0G`. |
| `-m`, `--summarize` | Só quantas cópias há, em quantos grupos, e o espaço que ocupam. |
| `-1`, `--sameline` | Um grupo por linha, com os ficheiros separados por espaços (entre aspas se um caminho tiver espaços). |
| `-f`, `--omitfirst` | Omite o primeiro ficheiro de cada grupo. |
| `-q`, `--quiet` | Sem progresso. Só aparece na saída de erros quando esta é um terminal. |
| `-A`, `--nohidden` | Deixa de fora ficheiros e pastas cujo nome começa por ponto. |
| `-H`, `--hardlinks` | Os nomes de um ficheiro com ligações físicas contam como cópias uns dos outros. |
| `-D`, `--folders` | Pastas duplicadas em vez de ficheiros: aquelas cujos ficheiros são os mesmos, qualquer que seja o nome. Com `-S`, `-m`, `-1`, `-f`, `--json` e `--csv` como nos ficheiros. |
| `--share=PERCENT` | Com `-D`, também pastas com pelo menos PERCENT de semelhança (50 a 100) pelo conteúdo, não só as idênticas. |
| `--min-size TAMANHO`, `--max-size TAMANHO` | Só ficheiros de pelo menos ou no máximo TAMANHO (`100KB`, `1 MB`, `1.5GB`). |
| `--after DATA`, `--before DATA` | Só ficheiros alterados a partir de DATA, ou até ao fim de DATA (`2024-01-31`, `31/01/2024`, `2024-01`, `2024`). |
| `--ext LISTA`, `--skip-ext LISTA` | Só ficheiros com estas extensões (`jpg,png`), ou deixando-os de fora. |
| `--include NOMES`, `--exclude NOMES` | Só ficheiros ou pastas cujo nome corresponde, ou deixando-os de fora (`*` e `?`, sem maiúsculas; uma pasta deixada de fora não é percorrida). |
| `--json`, `--csv` | Os grupos como lista JSON (`size`, `wasted`, `hash`, `files` com `path` e `modified` em segundos desde 1970) ou CSV (uma linha por ficheiro) em vez de texto. |
| `--no-index` | Ler pasta a pasta, sem o índice do Everything nem a tabela NTFS. |
| `--no-cache`, `--cache FICHEIRO` | Não usar a cache de hashes, ou guardá-la em FICHEIRO. |
| `--lang=en\|pt\|es` | Língua das mensagens (predefinição: `RDUP_LANG`, depois a do sistema). |
| `--version`, `--help` | A versão, e a ajuda. |

As opções curtas juntam-se (`-rSm`), `--` termina as opções e `/?` mostra a ajuda.

O código de saída é 0 quando a pesquisa foi feita (com duplicados ou sem eles), 1 quando não foi possível
ler uma pasta ou alguns ficheiros (os outros são pesquisados na mesma, e uma linha na saída de erros diz
quantos) e 2 para opções erradas.

## A cache de hashes

A primeira pesquisa de um disco grande lê todos os ficheiros que podem ter cópia, e os antivírus tornam isso
lento. Os hashes ficam guardados (por caminho, tamanho e data) em `hashes.bin`, no mesmo sítio que os do rdup,
para a pesquisa seguinte, aqui ou no rdup, só ler o que mudou. `--no-cache` ignora-a.

## Diferenças para o `fdupes`

- Sempre recursivo, e todos os caminhos em conjunto: uma cópia num deles é achada nos outros.
- As ligações físicas de um ficheiro não são cópias, a não ser com `-H`, como no fdupes.
- Os passos são sempre os mesmos, por isso não há `-p`/`-P` (permissões), `-o` (ordem) nem `-t` (tempo).
- Extras: `--min-size`, `--max-size`, `--after`, `--before`, `--ext`, `--skip-ext`, `--include`,
  `--exclude`, `--json`, `--csv`, `--no-index`, a cache de hashes.
- Nada é apagado nem ligado: ainda sem `-d`, `-N`, `-L` nem `-B`. Para remover cópias, use o rdup ou a lista do `-f`.
- As pastas duplicadas (`-D`) vêm da mesma pesquisa da página Pastas do rdup: uma pasta dentro de outra com tanto em comum não se repete.

## Compilar e testar

```powershell
./dev.ps1 build --profile release-small -p rdup-cli    # target\release-small\rdup-cli.exe
./dev.ps1 test -p rdup-cli -p rt-dups
```

Os testes criam pastas pequenas em `target`, dentro do projeto.
