# rsize-cli

*Leia isto em [inglês](README.md).*

Quanto espaço em disco ocupam os ficheiros e as pastas de um caminho? O gémeo de linha de
comandos do [rsize](../rsize/README.pt.md), com as opções do `du`. Lê o disco como o rsize:
a tabela de ficheiros do NTFS diretamente (como administrador), o índice do
[Everything](https://www.voidtools.com) quando está a correr, ou pasta a pasta. Um único
executável de 0,5 MB para Windows e Linux; mensagens em inglês, português e espanhol; sem
rede nem telemetria.

## Utilização

```bat
rsize-cli                       :: a pasta atual, em KiB, só as pastas
rsize-cli -h -d 1 C:\Users      :: as pastas de C:\Users com tamanhos como 1.5K, 23M, 4.0G
rsize-cli -sh C:\Windows D:\    :: só o total de cada caminho
rsize-cli -ah --top 20 D:\      :: os 20 maiores ficheiros e pastas de D:
rsize-cli --json -d 2 D:\ > d.json
```

A saída é a do `du`: o tamanho, um tab, o caminho como o escreveu, e cada pasta depois do que
tem dentro. Os tamanhos são o que os ficheiros ocupam **no disco** (clusters, compressão,
ficheiros esparsos); `--apparent-size` ou `-b` dão o comprimento. Um ficheiro com vários nomes
(ligações físicas) conta uma vez.

### Opções

| Opção | O que faz |
|---|---|
| `-h`, `--human-readable` | Tamanhos como `1.5K`, `23M`, `4.0G` (passos de 1024, arredondados para cima). |
| `--si` | Como `-h`, com passos de 1000 (`1.5k`). |
| `-b`, `--bytes` | Tamanhos em bytes, e o comprimento dos ficheiros (`--apparent-size`). |
| `-k`, `-m` | Tamanhos em KiB (a predefinição) ou em MiB, arredondados para cima. |
| `-s`, `--summarize` | Só o total de cada caminho (`-d 0`). |
| `-a`, `--all` | Também os ficheiros, não só as pastas. |
| `-c`, `--total` | Um total geral depois de todos os caminhos. |
| `-d N`, `--max-depth=N` | As pastas até N níveis abaixo de cada caminho. |
| `--apparent-size` | O comprimento dos ficheiros, não o espaço que ocupam no disco. |
| `--sort=size\|name` | Ordem dentro de cada pasta: a maior primeiro (a predefinição) ou por nome. |
| `--top N` | Só as N maiores entradas, da maior para a menor. |
| `--json`, `--csv` | Uma lista JSON ou CSV (`path`, `kind`, `size`, `allocated`, `files`, `folders`, `modified` em segundos desde 1970) em vez de texto. |
| `--no-index` | Ler pasta a pasta, sem o índice do Everything nem a tabela NTFS. |
| `--lang=en\|pt\|es` | Língua das mensagens (predefinição: `RSIZE_LANG`, depois a do sistema). |
| `--version`, `--help` | A versão, e a ajuda. |

As opções curtas juntam-se (`-sh`, `-ahd2`), `--` termina as opções e `/?` mostra a ajuda.

O código de saída é 0 quando tudo foi lido, 1 quando não foi possível ler um caminho ou algumas
pastas (os outros aparecem na mesma, e uma linha na saída de erros diz quantas) e 2 para opções
erradas.

## Diferenças para o `du`

- Um caminho que é um **ficheiro** mostra o seu comprimento (o espaço que ocupa não se sabe sem ler a sua pasta).
- Ligações simbólicas, junções e pontos de montagem são contados, não seguidos, e não se entra noutros sistemas de ficheiros.
- Com o índice do Everything ou a tabela NTFS as pastas não são lidas uma a uma: um disco leva segundos em vez de
  minutos. Só se usam quando conseguem responder pelo caminho; senão lê pasta a pasta.
- Extras: `--top`, `--sort`, `--json`, `--csv`, `--no-index`.
- Ainda sem `-x`, `-L`, `--exclude`, `--time`, `--block-size` nem `--threshold`.

## Compilar e testar

```powershell
./dev.ps1 build --profile release-small -p rsize-cli    # target\release-small\rsize-cli.exe
./dev.ps1 test -p rsize-cli -p rt-cli
```

Os testes criam pastas pequenas em `target\rsize-cli-tests`, dentro do projeto.
