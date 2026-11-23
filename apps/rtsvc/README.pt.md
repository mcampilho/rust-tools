# rtsvc

O serviço das rust-tools para Windows. Instalado uma vez, deixa as ferramentas mostrar o que precisa de direitos de
administrador (a tabela NTFS dos discos, os programas de todos os utilizadores, quanto ocupam os ficheiros do próprio
sistema) sem as abrir como administrador, e mantém isso pronto em segundo plano. Alterar coisas continua a ser só para
ferramentas abertas como administrador. Desenho, protocolo e modelo de ameaças: [docs/service.pt.md](../../docs/service.pt.md).

[English version](README.md)

## Transferir

Um zip dos [releases](https://github.com/mcampilho/rust-tools/releases) (etiquetas `rtsvc-v…`), Windows 10/11 de 64
bits, ou `scoop install rtsvc`. Extrai-o e, numa linha de comandos de administrador, corre `rtsvc install`: copia-se
para uma pasta que só os administradores podem alterar e regista o serviço. É opcional: as ferramentas funcionam sem
ele.

## Comandos

| Comando | O que faz |
| --- | --- |
| `rtsvc install [--dir PASTA] [--visibility user\|elevated] [--yes]` | Copia-se para `%ProgramFiles%\rust-tools` (ou PASTA), regista o serviço `RustToolsSvc` (automático, arranque atrasado, reiniciado depois de uma falha) e arranca-o. Precisa de uma linha de comandos elevada. Diz o que o serviço deixa os utilizadores ler e pergunta primeiro (`--yes` dispensa). |
| `rtsvc uninstall [--purge]` | Pára e remove o serviço e o programa; `--purge` apaga também os dados (`%ProgramData%\rust-tools`). Elevado. |
| `rtsvc status` | Se está instalado e a correr, e o que responde: versão, o seu nível, há quanto tempo corre, clientes, leituras e a idade de cada fotografia. |
| `rtsvc console` | Corre nesta janela, como você, num pipe próprio (`rust-tools-svc-dev`), para desenvolver. Não toca no serviço instalado, e as ferramentas não falam com ele. |

`--lang en|pt|es` (ou `RTSVC_LANG`) escolhe a língua das mensagens.

## O que faz

- **Lê a tabela NTFS** de um disco local para o rsize, o rdup, o rsize-cli e o rdup-cli, que lha pedem depois do Everything e
  antes de a lerem eles. `--visibility elevated` limita isto às ferramentas abertas como administrador.
- **Guarda fotografias**, feitas quando o computador está parado (o processador ocupado menos de um terço do tempo, na
  corrente), quando a lista de programas instalados muda, ou quando uma ferramenta pede: os programas (o rinst lê-os ao
  arrancar; um utilizador vê os da máquina e os seus, um administrador os de todos) e o tamanho do que o limpador
  encontra e precisa de administrador (o rclean mostra-o nas linhas que ainda não pode limpar). Ficam em
  `%ProgramData%\rust-tools\cache`, só legível por administradores.
- **Faz tarefas** para uma ferramenta aberta como administrador, uma de cada tipo de cada vez, que continuam se a
  ferramenta fechar: limpar os alvos do limpador que precisam de direitos (por id, só ficheiros e a Reciclagem) e
  desinstalar programas da máquina pelo código de produto do Windows Installer. As ferramentas ainda não usam tarefas.

## Seguro por desenho

- Tudo é um pedido com as suas verificações; não há "corre isto" nem "apaga este caminho".
- Quem pede lê-se do token no pipe: qualquer um lê, só um administrador elevado altera. Abrir a porta do UAC não é
  tarefa do serviço e ele não a abre.
- O serviço só corre de uma pasta que só SYSTEM, Administradores e TrustedInstaller podem alterar (verificado ao instalar e
  em cada arranque), e as ferramentas verificam que o servidor do pipe é o que está registado.
- Num PC em que o `Everyone` tem controlo total da raiz do disco (`icacls C:\`), todas as pastas desse disco são recusadas.

## Limitações

- O `install`, o `uninstall` e o serviço a sério foram testados só por partes (o pipe, os níveis, a verificação de pasta, as
  fotografias e as tarefas, com testes num só processo); ainda não correram com o serviço instalado.
- As ferramentas não mostram que uma lista veio do serviço, nem deixam ignorá-lo (o rinst lê os programas ao vivo depois
  da primeira leitura).
- Restringir os privilégios do serviço e as tarefas nas janelas das ferramentas ficam por fazer.
- Só Windows.

## Compilar

```powershell
./dev.ps1 build --profile release-small -p rtsvc
./dev.ps1 test -p rtsvc -p rt-svc
```
