# Um serviço com privilégios para as rust-tools (desenho)

Estado: construído, instalado e a responder como SYSTEM num PC real (`install`, `status`, a leitura de volumes e as fotografias). Ainda não correram a sério: o `uninstall` e as tarefas. As ferramentas ainda não usam tarefas; as tarefas não são guardadas em disco, por isso não sobrevivem a um reinício do serviço. [English version](service.md)

## 1. Objetivo

Algumas coisas precisam de direitos de administrador: ler a tabela NTFS de um disco, listar os programas de
todos os utilizadores, medir os temporários do sistema, remover o que sobrou de um programa. Hoje o utilizador
tem de abrir a aplicação inteira como administrador. Este desenho acrescenta um pequeno serviço do Windows,
como o do Everything, a que as ferramentas recorrem quando está a correr:

- **Todos veem o sistema.** Um utilizador sem privilégios tem a visão de todo o sistema (todos os discos,
  todos os programas instalados, o tamanho do que se pode limpar) sem abrir nada como administrador.
- **Só um cliente elevado altera.** Alterar, remover ou desinstalar continua a exigir a ferramenta aberta
  como administrador, como antes. O serviço faz então o trabalho em segundo plano e a janela fica livre.
- **Trabalho adiantado.** Enquanto o computador está parado, o serviço reúne o que o `rclean` e o `rinst`
  demoram a descobrir, e a ferramenta abre com a maior parte pronta.

Fora do âmbito: atualizações (cada ferramenta mantém a sua, ver o `CLAUDE.md`), rede de qualquer espécie,
Linux (não há equivalente previsto) e tudo o que deixe um utilizador fazer o que o Windows não deixaria.

## 2. Princípios

1. **Nenhum poder novo para ninguém.** O que um utilizador normal pode fazer é igual com ou sem serviço,
   exceto *ler* informação do sistema inteiro. Escrever exige um cliente elevado e administrador, verificado
   pelo serviço em cada pedido. Não substitui nada do que o UAC protege.
2. **Verbos fixos, sem comandos.** O protocolo tem uma lista de pedidos, cada um com as suas verificações.
   Não há "executa isto" nem "apaga este caminho": o limpador indica um item de um catálogo que o serviço conhece.
3. **As ferramentas funcionam sem ele.** Cada ferramenta tenta primeiro o serviço e, se não o encontra, faz o
   que faz hoje (elevação própria, ou a vista limitada). O serviço é opcional e nada depende dele.
4. **Pequeno e discreto.** Poucas dependências, sem rede, prioridade baixa, trabalho pesado só com o
   computador parado.

## 3. Peças

| Peça | O que é |
| --- | --- |
| `crates/rt-svc` | O protocolo (pedidos, respostas, codificação das tramas) e o cliente que todas as ferramentas usam: `connect()`, os verbos como funções e as verificações do §5. |
| `apps/rtsvc` | O executável do serviço. `rtsvc install`, `uninstall`, `status`, `run` (como serviço) e `console` (em primeiro plano, para desenvolver). Só Windows. |

As ferramentas que ganham: rsize, rdup (e rsize-cli, rdup-cli), rinst e rclean. O código que correm para
o NTFS e para as listas fica nos seus crates (`rt-scan`, `rt-dups`, …); o serviço chama as mesmas funções,
por isso há uma só implementação.

A ordem de quem lê um disco passa a ser: **Everything** (se estiver a correr) → **o serviço** → leitura
própria do NTFS (se a ferramenta estiver elevada) → pasta a pasta. O Everything fica no cliente: um serviço
corre na sessão 0 e não chega a um programa da sessão do utilizador pelas mensagens de janela que o
Everything usa. Um cliente já elevado salta o serviço e lê a tabela NTFS ele próprio.

## 4. Transporte e protocolo

- **Pipe nomeado** `\\.\pipe\rust-tools-svc`, modo de mensagens, `PIPE_REJECT_REMOTE_CLIENTS`, criado com
  `FILE_FLAG_FIRST_PIPE_INSTANCE` (ninguém consegue criar o nome antes do serviço).
- **DACL do pipe:** SYSTEM, Administradores e o dono (a conta com que o serviço corre) com controlo total;
  os Utilizadores autenticados só leem e escrevem dados, leem atributos, leem o dono e sincronizam
  (`0x120083`). Não `FILE_APPEND_DATA`: é o direito de criar uma instância do pipe, e deixaria um utilizador
  responder no lugar do serviço. Os inícios de sessão de rede são recusados; um anónimo não é um utilizador autenticado.
- **Tramas:** comprimento `u32` (little endian), versão do protocolo `u8`, pedido `u16`, depois os dados.
  Uma trama tem no máximo 1 MiB, exceto os lotes de entradas de uma leitura (8 MiB). Um cliente pode ter
  8 pedidos abertos; o serviço atende no máximo 32 clientes ao mesmo tempo.
- **Dados:** um só codificador binário simples, escrito à mão (números little endian, textos com comprimento),
  estrito a descodificar: cada leitura é verificada, as contagens são comparadas com os bytes que restam e
  bytes a mais são um erro. Um pedido desconhecido dá uma resposta de erro, nunca uma falha.
- **Versões:** o cliente envia `Hello {protocolo, aplicação, versão}`; o serviço responde com o seu
  protocolo, versão e capacidades. Fala os protocolos N e N-1. Um cliente mais novo que o serviço usa o que
  está nas capacidades e recorre ao resto de outra forma.
- **Tempos limite:** o cliente liga-se em ~200 ms e desiste (recorre ao que fazia); cada pedido tem um
  prazo. O serviço larga um cliente calado durante 60 s.
- **Cancelamento:** um pedido cancela-se pelo seu número; fechar o pipe cancela todos os do cliente.

## 5. Quem chama e quem responde

**O serviço verifica o cliente** em cada ligação:

1. `GetNamedPipeClientProcessId`, depois `ImpersonateNamedPipeClient` e ler o token (`OpenThreadToken`):
   o SID do utilizador, a sessão e `TokenElevation`. Se o token tem um token ligado (UAC), o serviço vê
   se o *ligado* é de administrador, mas só trata o cliente como elevado quando o token que recebeu é ele
   próprio elevado e está em Administradores.
2. Três níveis, só pelo token:
   - **User**: qualquer utilizador autenticado. Pode ler informação ao nível do sistema.
   - **Elevated**: membro de Administradores com token elevado. Pode também alterar (§7) e ver os dados
     dos outros utilizadores (§6).
   - **Denied**: anónimo, convidado, ou um token que não se consegue ler.
3. O caminho ou o nome do programa do cliente **não** conta para a segurança (pode ser forjado). Só é
   escrito no registo.

**O cliente verifica o serviço** antes de enviar seja o que for: `GetNamedPipeServerProcessId`, abrir o
processo e exigir que o token seja de SYSTEM e que a imagem seja o `rtsvc.exe` que o Gestor de Serviços tem registado (§9).
Um servidor falso poderia, de outra forma, recolher o que a ferramenta envia.

## 6. O que é visível a quem

| Dados | User | Elevated |
| --- | --- | --- |
| Discos e a sua tabela NTFS (nomes, tamanhos, datas) | sim | sim |
| Programas instalados para todos, funcionalidades do Windows, serviços | sim | sim |
| Programas, pastas e caches **do próprio utilizador** | sim | sim |
| Programas, pastas e caches **de outros utilizadores** (perfis, caches dos browsers) | não | sim |
| Temporários do sistema, registos, caches de atualizações (tamanhos) | sim | sim |
| Conteúdo dos ficheiros | não (o serviço nunca lê nem devolve conteúdos) | não |

A tabela NTFS mostra nomes e tamanhos de pastas que o utilizador não poderia abrir. O Everything faz a mesma
troca e o rsize/rdup já funcionam assim com ele. Por isso o serviço é **opt-in**: o `rtsvc install` di-lo
e pede confirmação, e uma definição (`rtsvc config visibility = user | elevated`) pode limitar a tabela aos
clientes elevados. **Questão em aberto:** o valor por omissão da linha dos dados de outros utilizadores
(o proposto acima).

O rdup nunca pede conteúdos ao serviço: o hash é feito pela ferramenta, com os direitos do próprio
utilizador, nos ficheiros que consegue abrir.

## 7. Pedidos

**Leitura (User)**

| Pedido | Resposta |
| --- | --- |
| `Ping`, `Status` | versão, tempo ativo, o que está em cache e há quanto tempo, o que está a correr |
| `ScanVolume {raiz}` | as entradas de um disco, em lotes (as `rt_scan::model::Entry`), o mesmo resultado da leitura NTFS que as ferramentas fazem |
| `Programs {desde}` | os programas instalados, todas as origens (registo, MSI, Store…), com a idade da fotografia |
| `Features` | funcionalidades do Windows (DISM) e outras listas que a ferramenta lê ao arrancar |
| `CleanScan {desde}` | os itens do catálogo do limpador com tamanhos e idades |
| `Jobs` | a lista e o estado das tarefas (§8): as do próprio, ou todas para Elevated |

**Escrita (só Elevated)**

| Pedido | O que faz |
| --- | --- |
| `Clean {itens}` | remove itens do catálogo por id (nunca por caminho, §8) |
| `Uninstall {programas}` | corre a desinstalação de programas de `HKLM` (por código de produto MSI, ou o comando registado, em silêncio) |
| `RemoveLeftovers {ids}` | remove restos que o próprio serviço achou para um programa |
| `CancelJob {id}` | para uma tarefa em curso |

Todo o pedido de escrita torna-se uma **tarefa** (§8). Os pedidos que precisam do contexto do próprio
utilizador (programas instalados só para um utilizador) o serviço não faz: a ferramenta fá-los como hoje.

## 8. Trabalho em segundo plano e tarefas

**Agendador.** O serviço guarda fotografias (programas, funcionalidades, análise do limpador) e atualiza-as:

- **Com o computador parado:** sem entrada há alguns minutos, CPU abaixo de um limiar, fora da bateria,
  sem aplicação em ecrã inteiro. O trabalho corre com `PROCESS_MODE_BACKGROUND_BEGIN` (prioridade baixa de
  CPU e de disco) e pára logo que o computador volta a ser usado.
- **Por eventos** onde o Windows os dá: uma notificação de alteração nas chaves de desinstalação
  (`RegNotifyChangeKeyValue`) marca a fotografia dos programas como suja, e só se volta a ler o que mudou;
  o diário USN de um volume faz o mesmo para as pastas do limpador.
- **No máximo:** o catálogo do limpador de poucas em poucas horas e depois de uma alteração; os programas
  depois de uma alteração ou todos os dias.
- **Nunca** enquanto um cliente tem um pedido que precisa dos mesmos dados: o pedido do cliente tem prioridade.

**Cache.** `%ProgramData%\rust-tools\` com ACL de escrita para SYSTEM e Administradores e leitura para
Utilizadores. Os ficheiros escrevem-se com um nome temporário e mudam de nome, têm versão e tamanho
limitado. Cada fotografia leva a hora a que foi feita; a ferramenta mostra a idade e pode pedir uma
atualização. As entradas sobre outros utilizadores ficam numa parte que os Utilizadores não leem (§6).

**Tarefas.** Um pedido de escrita devolve logo um número. Uma tarefa:

- sobrevive à ferramenta que a pediu (fechar a janela não a pára), é guardada em disco e tem um registo
  do resultado (o que foi removido, o que falhou e porquê) que a ferramenta lê depois;
- pode ser cancelada, indica o progresso e corre uma de cada tipo de cada vez;
- está limitada pelo **catálogo**: os itens do limpador são ids com significado fixo (uma pasta ou uma
  regra) que o serviço resolve. Um id nunca é um caminho vindo do cliente.

**Apagar com segurança** (ficheiros e pastas simples das tarefas de limpeza). Os ficheiros abrem-se por *handle* com `FILE_FLAG_OPEN_REPARSE_POINT`, o caminho
final verifica-se contra o catálogo (`GetFinalPathNameByHandle`) depois de abrir, e apagam-se por esse
*handle* (`FILE_DISPOSITION_INFO_EX`). Junções e ligações simbólicas removem-se como ligações, nunca se
seguem. Isto fecha a falha entre verificar e usar, em que um utilizador troca uma pasta por uma junção.

**Desinstalar.** Um serviço está na sessão 0 e não mostra janelas. As desinstalações silenciosas de `HKLM`
e o `msiexec /x` por código de produto correm no serviço. Um desinstalador com janelas arranca na sessão do
cliente, com um token primário feito a partir do token *elevado* do cliente (`DuplicateTokenEx`, com o id
de sessão do cliente), e comporta-se como se o utilizador o tivesse aberto elevado. O que está instalado
por utilizador (`HKCU`) é corrido como esse utilizador pela ferramenta, não pelo serviço.

## 9. Instalação

- `rtsvc install` (numa linha de comandos elevada; o Scoop não eleva): copia o `rtsvc.exe` por omissão
  para `%ProgramFiles%\rust-tools\`, regista o serviço `RustToolsSvc` como automático (arranque atrasado)
  e arranca-o. `rtsvc install --dir <pasta>` usa outra pasta (por exemplo `C:\Programs`, como se faz com
  o Everything), **se a pasta for segura** (abaixo). O serviço **nunca corre de uma pasta onde um
  utilizador escreve**: senão trocar o exe seria uma escalada de privilégios.
- **Uma pasta é segura quando**, verificado pelo `install` e outra vez pelo serviço em cada arranque (se
  não for, não arranca e diz porquê no registo):
  - está num volume local fixo (não numa partilha de rede, nem amovível, nem numa unidade `subst` ou mapeada);
  - nem a pasta nem nenhuma pasta acima dela até à raiz é uma junção ou ligação simbólica (o caminho é o
    mesmo depois de resolvido);
  - na ACL da própria pasta, e de cada ficheiro e pasta lá dentro (quatro níveis), só SYSTEM,
    Administradores e TrustedInstaller têm algum direito que altere coisas (escrever, acrescentar,
    atributos, apagar, mudar permissões, tomar posse, escrita ou tudo genéricos); `Users`,
    `Authenticated Users`, `Everyone` e qualquer outra conta só podem ler e executar. Em cada pasta
    **acima**, mais ninguém pode apagá-la a ela ou às de baixo, nem mudar permissões ou dono (acrescentar
    coisas ao lado é inofensivo). Não contam as entradas só para o que se cria lá dentro (herança); as
    entradas de recusa não se leem, o que só pode tornar a verificação mais estrita; uma ACL em falta
    (todos com controlo total) falha;
  - o dono é SYSTEM, Administradores ou TrustedInstaller;
  - os ficheiros lá dentro (o `rtsvc.exe` e o que carrega) cumprem a mesma regra de ACL.
  Uma pasta com as permissões de um `C:\Program Files` normal passa. A raiz do disco conta também: um PC
  cujo `C:\` dá ao `Everyone` controlo total da própria raiz (como mostra o `icacls C:\` no PC do autor)
  deixa qualquer um mudar o nome do que está por baixo, por isso todas as pastas desse disco, a
  `Program Files` incluída, são recusadas até essa entrada ser removida. O `install` diz a entrada que falha e
  não tem opção para ignorar a verificação.
- **A pasta de dados** (`%ProgramData%\rust-tools`) também é verificada: os utilizadores podem criar pastas em
  `%ProgramData%`, por isso o `install` e cada arranque recusam uma que tenha uma ligação ou junção, ou algo que
  não seja de SYSTEM, Administradores ou TrustedInstaller, e depois entregam-na aos Administradores com as
  permissões do próprio serviço.
- O cliente descobre onde está o serviço perguntando ao Gestor de Serviços pelo caminho registado da
  imagem (`QueryServiceConfig`), sem presumir Program Files, e compara-o com a imagem do processo
  que está no pipe (§5).
- O serviço corre como LocalSystem, com ações de recuperação para o reiniciar depois de uma falha.
  Restringir-lhe os privilégios (`SERVICE_CONFIG_REQUIRED_PRIVILEGES_INFO`) fica para o passo de
  endurecimento, com medições numa instalação real: tirar um de que precisa pararia a leitura do volume.
- O `install` mostra o que o serviço deixa ler (e a quem) e pede confirmação (`--yes` dispensa-a).
- `rtsvc uninstall` pára e remove o serviço e os ficheiros, e oferece-se para apagar a cache.
- Para atualizar o serviço, instala-se de novo numa linha de comandos elevada (parar, substituir,
  arrancar). As ferramentas avisam quando o serviço é mais antigo do que precisam.
- No bucket do Scoop o serviço é um pacote à parte (`rtsvc`) com uma mensagem a apontar para
  `rtsvc install`; o Scoop nunca instala nada como serviço.

## 10. Ameaças e as respostas

Os atacantes considerados: um utilizador local normal que quer mais direitos, um processo da sessão de um
administrador que *não* está elevado, e dados malformados.

| Ameaça | Resposta |
| --- | --- |
| Um utilizador troca o exe do serviço | instalado em Program Files ou noutra pasta que passe a verificação do §9 (feita na instalação e em cada arranque, por isso uma mudança posterior de permissões pára o serviço); o cliente compara o caminho da imagem do servidor com o que o Gestor de Serviços registou (§5) |
| Um processo cria primeiro o nome do pipe | o serviço cria-o com `FIRST_PIPE_INSTANCE` no arranque, antes de qualquer processo de utilizador; o cliente verifica o token do servidor (§5); depois disso o nome nunca desaparece (uma instância nova antes de fechar uma estragada) |
| Um utilizador cria a pasta de dados antes de o serviço ser instalado | não é usada: não se aceita nela nenhuma ligação nem outro dono (§9) |
| Um processo não elevado de um administrador pede uma escrita | o token recebido pelo pipe não é elevado, por isso o pedido é recusado (§5); o UAC não é contornado |
| Troca de caminhos, junções ou ligações simbólicas | os pedidos levam ids do catálogo, abertura por *handle*, verificação do caminho final (§8) |
| Ler o que um utilizador não pode (conteúdos) | nenhum pedido devolve conteúdos de ficheiros; só nomes e tamanhos, e conforme a definição de visibilidade (§6) |
| Informação sobre outros utilizadores | numa parte da cache que os Utilizadores não leem; só respondida a Elevated (§6) |
| Recusa de serviço (muitos clientes, pedidos enormes, leituras sem fim) | os limites do §4, prioridade baixa, uma tarefa de cada tipo de cada vez, prazos |
| Tramas malformadas ou hostis | limites de comprimento (um pedido nunca é longo), descodificação estrita, sem `unsafe` no codificador, testes de *fuzz* do descodificador |
| Cache velha ou forjada | os ficheiros da cache só são escritos por SYSTEM e Administradores; cada um leva versão e hora |
| Segredos nos registos | os registos dizem o utilizador, o verbo e as contagens, nunca conteúdos; rodam, em `%ProgramData%\rust-tools\logs` |

O `unsafe` fica em módulos Win32 pequenos com uma nota `SAFETY`, como no resto do workspace.

## 11. O que as ferramentas fazem com ele

| Ferramenta | Com o serviço | Sem ele |
| --- | --- | --- |
| rsize, rsize-cli | Everything → `ScanVolume` → NTFS próprio (se elevada) → pasta a pasta | como hoje |
| rdup, rdup-cli | o mesmo para a listagem; o hash fica na ferramenta | como hoje |
| rinst | `Programs` e `Features` logo; desinstalar e restos como tarefas quando elevada | lê tudo ao arrancar, como hoje |
| rclean | `CleanScan` logo; `Clean` como tarefa quando elevada | analisa ao arrancar, como hoje |

Cada uma destas ferramentas mostra quando usou o serviço ("via serviço, há 3 minutos") e tem maneira de
atualizar ou de o ignorar.

## 12. Plano

1. **Este documento**, revisto.
2. **Esqueleto:** `rt-svc` (tramas, codificador, cliente com as verificações do §5), `rtsvc` com
   `install`, `uninstall`, `status`, `console`, `Ping` e `Status`, testes do codificador e dos níveis de acesso.
3. **`ScanVolume`** para o rsize/rdup (e os `-cli`), a partir do código do `rt-scan`.
4. **Fotografias:** programas e funcionalidades para o rinst, catálogo e tamanhos para o rclean, o
   agendador e a cache.
5. **Tarefas para clientes elevados:** limpar (catálogo, apagar com segurança), desinstalar e restos.
6. **Endurecimento:** *fuzz* do descodificador, a revisão do §10, documentação no README de cada ferramenta.

Cada passo termina com testes, clippy e as ferramentas a funcionar com e sem o serviço.

## 13. Questões em aberto

- O valor por omissão dos dados de outros utilizadores (§6): visíveis só a Elevated (o proposto) ou a todos.
- Se a verificação do §9 deve aceitar também uma pasta cujos únicos escritores extra são nomeados pelo
  utilizador (proposto: não, para a regra continuar simples).
- O nome do serviço e do executável (propostos: `RustToolsSvc` e `rtsvc`).
- Quanto tempo se guarda a cache e os limiares de "computador parado", a acertar com medições.
- Se uma ferramenta `-cli` deve ter `--no-service` (proposto: sim, com `--service` para o exigir).
