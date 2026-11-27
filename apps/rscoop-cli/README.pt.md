# rscoop-cli

*Ler em [inglês](README.md).*

O [Scoop](https://scoop.sh), mais rápido. Trabalha sobre a mesma instalação do Scoop: o rscoop-cli
faz ele próprio o que lê e o que mantém a instalação, e **entrega ao Scoop tudo o que instala ou
remove aplicações**, por isso pode usar-se em vez do `scoop`. O plano do resto está em
[docs/rscoop.pt.md](../../docs/rscoop.pt.md).

Sem comando faz o que já fazia: **valida a instalação**. Um só executável de 3,4 MB; textos em
inglês, português e espanhol.

## Comandos

```bat
rscoop-cli list
rscoop-cli search ripgrep
rscoop-cli update
rscoop-cli status
rscoop-cli install ripgrep      (entregue ao Scoop)
```

| Comando | O que faz |
|---|---|
| `list [query]`, `search [query]` | As aplicações instaladas; as dos buckets locais (pelo nome e pelo programa). |
| `info <app> [-v]`, `cat <app>`, `home <app>` | O que se sabe de uma aplicação, o manifesto, a página. |
| `status [--local]` | Se o Scoop e os buckets estão atrasados (consultados ao mesmo tempo) e depois as aplicações desatualizadas, estragadas, descontinuadas e removidas. `--local` não usa a rede. |
| `which`, `prefix`, `depends` | O programa de um comando, a pasta de uma aplicação, o que ela precisa. |
| `export [-c]`, `checkup` | A instalação em JSON; os testes do `scoop checkup` e depois as verificações abaixo (valem as opções delas). |
| `update` | Atualiza o Scoop e os buckets, estes ao mesmo tempo. O `update <app>` é do Scoop. |
| `bucket list\|known\|add\|rm`, `config [nome [valor]]`, `alias add\|rm\|list` | Buckets, definições e aliases. |
| `download <app…> [-f] [-s] [-a arq]` | Os ficheiros das aplicações para a cache, com as somas conferidas (aria2 quando o Scoop o usaria). |
| `cache show\|rm`, `cleanup <app…>\|* [-g] [-k]` | O que fica guardado: descargas e versões antigas. |
| `hold`, `unhold` | Fixar a versão de aplicações (ou do próprio `scoop`). |
| `shim add\|rm\|list\|info [-g]` | Shims como o Scoop os faz (`.exe` e `.shim`, `.cmd`, `.ps1`, `sh`); o de outra aplicação fica de lado e volta. O `shim alter` é do Scoop. |
| `reset <app…>\|*`, `uninstall <app…> [-g] [-p]` | Feitos aqui só com `RSCOOP_NATIVE=1`, enquanto se comparam com o Scoop; sem isso, do Scoop. Os scripts dos manifestos correm no código do próprio Scoop. |
| `help [comando]`, `scoop <comando…>` | Ajuda; executar qualquer comando com o próprio Scoop. |

**Onde é mais cauteloso do que o Scoop:** o `bucket rm` só aceita o nome de um bucket (`bucket rm ..`
apagaria a instalação); o `cleanup` não remove nada quando não sabe qual é a versão atual, e nunca
segue uma ligação para dentro do `persist`. **O que deixa para o Scoop:** uma atualização que
reporia o repositório do próprio Scoop (outro repositório ou ramo, sem git), a cache SQLite,
`app@versão`, manifestos de um endereço da web e descargas por proxy, FTP ou FossHub.

`--json` dá os dados de todos os comandos feitos aqui, `--root CAMINHO` escolhe a instalação e
`--lang en|pt|es` a língua. A instalação é a que o Scoop usaria: `--root`, `SCOOP`, `root_path`, a
pasta do `scoop` que está no `PATH` e por fim `~\scoop`; um `config.json` portátil na raiz conta. O
`compare.ps1` compara as respostas com as do Scoop oficial.

Num PC com duas instalações (uma leitura de cada, não é um benchmark):

| | Scoop, 361 aplicações | rscoop-cli, 361 | rscoop-cli, 1500 |
|---|---:|---:|---:|
| `list` | 10,7 s | 0,4 s | 1,5 s |
| `status --local` | 24,8 s | 0,5 s | 2,0 s |
| `search` | 4,6 s | 0,5 s | 1,2 s |

## Verificar uma instalação

Sem comando, o rscoop-cli valida uma instalação do Scoop, esteja onde estiver, e indica o que está
estragado ou ainda aponta para uma raiz antiga do Scoop — por exemplo depois de mudar o Scoop de
`D:\Portable\scoop` para `E:\Portable\scoop`. Por omissão **não altera nada**: só lê ficheiros e o
registo. Com `--fix` corrige o que conseguir (ver [Correções](#correções)).

É a versão em Rust do `scoop-check` em Python, com as mesmas verificações, opções e
relatório, e duas utilizações do SQLite para ser mais rápido (ver [Rapidez](#rapidez)).

## Como correr

```bat
rscoop-cli
rscoop-cli --lang pt --skip configs
rscoop-cli --json > relatorio.json
```

O código de saída é 0 quando não se encontra nada, 1 quando há erros ou avisos (ou, com
`--fix`, quando uma correção falhou) e 2 para opções erradas.

### Opções

| Opção | O que faz |
|---|---|
| `--root CAMINHO` | Raiz do Scoop a verificar. Por omissão: a variável `SCOOP` (processo, utilizador, sistema), depois `root_path` em `~\.config\scoop\config.json`, depois `~\scoop`. |
| `--old-root CAMINHO` | Uma raiz anterior a procurar (pode repetir-se). Sem ela, as outras raízes são encontradas na mesma pelos caminhos com a forma `X:\…\scoop\apps\…` (ou `shims`, `persist`, `buckets`, `cache`, `modules`). |
| `--json` | Relatório em JSON em vez de texto. |
| `--checks LISTA` / `--skip LISTA` | Fazer só, ou saltar, algumas verificações (identificadores abaixo, separados por vírgulas). |
| `--max N` | Problemas mostrados por verificação no relatório de texto (predefinição 50, `0` = todos). O JSON tem sempre todos. |
| `--config-depth N` | Profundidade das pastas onde se procuram ficheiros de configuração em `apps\*\current` (predefinição 3). |
| `--persist-depth N` | Profundidade das pastas onde se procuram ficheiros de configuração em `persist` (predefinição 5). |
| `--lang en\|pt\|es` | Língua do relatório (predefinição: `RSCOOP_LANG`, depois a língua do Windows; inglês para as outras). |
| `--fix [LISTA]` | Corrige o que for possível; opcionalmente só algumas verificações (`current`, `persist`, `shims`, `regfiles`, `configs`, `shortcuts`, `registry`, `env`). |
| `--dry-run` | Mostra as correções que o `--fix` faria, sem alterar nada. |
| `--backup-dir CAMINHO` | Onde o `--fix` guarda as cópias de segurança (predefinição `%LOCALAPPDATA%\rscoop\backup\<data-hora>`). |
| `--no-cache` | Volta a ler todos os ficheiros e não mexe na cache. |
| `-q`, `--quiet` | Sem linhas de progresso no stderr. |

## Verificações

| Id | O que se verifica |
|---|---|
| `roots` | De onde vem a raiz; se `SCOOP` e `SCOOP_GLOBAL` (processo, utilizador, sistema) e `root_path`/`global_path` do `config.json` apontam para pastas que existem; se `SCOOP` e `root_path` coincidem. |
| `shims` | Cada `<raiz>\shims\*.shim`, `*.cmd`, `*.ps1` e shim bash sem extensão: o destino existe e não está noutra raiz; os `.exe` e `.shim` andam aos pares. Os shims alternativos (`x.shim.app`) dão avisos. |
| `current` | `apps\<app>\current` é uma junção para uma pasta de versão que existe. Uma **pasta `current` vazia** (o que uma cópia simples da árvore deixa em vez da junção) é um erro, com a pasta de versão para onde devia apontar. |
| `persist` | Para cada entrada `persist` do manifesto instalado: a pasta é uma junção (ou o ficheiro uma ligação física) para `<raiz>\persist\<app>\…`. |
| `install` | Aplicações sem `scoop-install.json`/`scoop-manifest.json` (ou os antigos `install.json`/`manifest.json`) — o "Install failed" do `scoop list`; manifestos que já não estão no bucket; buckets que faltam ou não são repositórios git. |
| `shortcuts` | Ficheiros `.lnk` nas pastas `Scoop Apps` do menu Iniciar (do utilizador e de todos): destino, pasta de trabalho e ícone. |
| `regfiles` | Ficheiros `.reg` em `apps\*\current` (como o `install-context.reg`), que têm caminhos absolutos escritos na instalação. |
| `registry` | `Software\Classes`, `Run`, `RunOnce`, `App Paths` e `Uninstall` em HKCU: valores que apontam para dentro de outra raiz do Scoop ou para ficheiros que não existem (em `Classes` e `Uninstall` só contam os ficheiros em falta dentro da raiz do Scoop). |
| `env` | Variáveis do utilizador e do sistema (lidas do registo): cada entrada do `PATH` e cada variável com ar de caminho existe e não está noutra raiz; a pasta dos shims está no `PATH`. |
| `configs` | Ficheiros de configuração de texto (`.ini`, `.json`, `.cfg`, `.conf`, `.xml`, `.toml`, `.yaml`, `gitconfig`, …, até 512 KB) em `apps\*\current` e `persist` com um caminho dentro de outra raiz ou um caminho em falta dentro da atual. |

A raiz global (`SCOOP_GLOBAL`, `global_path` ou `%ProgramData%\scoop`) também conta como
raiz atual; as suas aplicações, shims e pasta persist também são verificados quando existe.

## Rapidez

Na primeira corrida quase todo o tempo vai para abrir uns 30 000 ficheiros pequenos — e o
antivírus lê cada um na primeira abertura. O rscoop-cli guarda uma cache em
`%LOCALAPPDATA%\rscoop\cache.db` (SQLite): para cada shim, atalho, JSON de instalação ou
manifesto, `.reg` e ficheiro de configuração, o tamanho, a data de modificação e o que se
leu dele (nos ficheiros de configuração, só os caminhos encontrados). Um ficheiro com o
mesmo tamanho e a mesma data não volta a ser aberto; se os caminhos existem verifica-se
sempre de novo. As linhas sem uso há 30 dias são apagadas. `--no-cache` desliga-a.

Com o `use_sqlite_cache` do Scoop ligado, a verificação `install` lê também o `scoop.db`
do próprio Scoop (os manifestos de todos os buckets) numa só consulta; uma aplicação que
lá não esteja é procurada na pasta do bucket, porque essa base de dados pode estar
atrasada.

Na instalação para que foi escrito (1500 aplicações, 28 000 ficheiros de configuração,
disco rígido):

| | Primeira corrida | Seguintes |
|---|---|---|
| `scoop-check` em Python | 264 s | 264 s |
| rscoop-cli | 261 s | 17 s |

Os ficheiros de configuração são lidos em várias threads. A primeira corrida depende do
disco e do antivírus, como a versão em Python.

## Correções

```bat
rscoop-cli --fix --dry-run
rscoop-cli --fix
```

Corra primeiro com `--dry-run` para ver a lista. Depois o `--fix` corrige, por esta ordem:

| Verificação | Correção |
|---|---|
| `current` | Uma pasta `current` vazia ou uma junção estragada passa a ser uma junção para a pasta da versão mais recente. |
| `persist` | Um item persistente em falta ou mal ligado passa a ser uma junção (pasta) ou ligação física (ficheiro) para `<raiz>\persist\<app>\…`, quando esse existe. Ficheiros ou pastas normais no caminho ficam como estão. |
| `shims`, `regfiles`, `configs` | A raiz antiga é substituída pela atual no texto, em todas as grafias (`\`, `/` e o `\\` dos ficheiros JSON e `.reg`); a codificação (UTF-8, UTF-16, BOM, Windows-1252) e o atributo só de leitura mantêm-se. |
| `shortcuts` | O atalho é recriado através do `WScript.Shell`, com a raiz antiga substituída no destino, argumentos, pasta de trabalho e ícone. (Mudar só o destino deixa o caminho antigo dentro do ficheiro, na secção LinkInfo — é o que o `scoop reset` faz.) Atalhos que o WScript não consegue ler ficam como estão. |
| `registry`, `env` | Valores de HKCU e variáveis do utilizador/sistema: a raiz antiga é substituída, mantendo o tipo do valor. As variáveis do sistema precisam de uma linha de comandos de administrador. Os programas abertos são avisados de que o ambiente mudou. |

Cada ficheiro alterado é primeiro copiado para a pasta de cópias de segurança (dentro de
uma pasta com o nome do disco), e os valores do registo alterados ficam em `registry.json`,
com o valor antigo e o novo. Ficheiros em falta que não estão noutra raiz não se podem
corrigir automaticamente e continuam no relatório. Volte a correr o rscoop-cli no fim para ver
o que falta.

## Testes

```powershell
./dev.ps1 test -p rscoop-cli
```

Os testes criam uma árvore Scoop falsa (junções, ligações físicas, shims, atalhos,
ficheiros `.reg` e de configuração) em `target\rscoop-tests` e usam um registo em memória;
o registo verdadeiro nunca é lido nem alterado.

## Línguas

Os textos estão em `src/strings.txt`: uma linha por chave com os textos em inglês,
português europeu e espanhol, separados por tabulações.

## Linux

O Scoop só existe no Windows: em Linux o rscoop-cli só diz isso.
