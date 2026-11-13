# runinst

Um desinstalador portátil para Windows, escrito em Rust. Lista os programas
instalados no computador para todos os utilizadores (do registo, do Windows
Installer, da Loja e do winget, numa só lista; o Scoop e o Chocolatey em
separadores próprios), com o tamanho, a versão, o editor, o comando de
desinstalação, a entrada do registo, a versão mais recente e os atalhos que lhes
pertencem no menu Iniciar e no ambiente de trabalho; e desinstala-os, um ou
vários seguidos, e depois encontra o que deixaram para trás.

[English version](README.md)

O runinst faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo
correm na raiz do repositório.

O `runinst.exe` é um único executável de 2,1 MB: sem instalador, sem runtime,
sem DLLs além das que vêm com o Windows. Não faz ligações à rede e não tem
telemetria. Está a ser feito por fases: lista, desinstala e remove os restos,
também de programas removidos antes; as funcionalidades opcionais do Windows e
os pacotes das ferramentas de desenvolvimento vêm a seguir (ver
[A seguir](#a-seguir)).

## Utilização

```text
runinst
```

### O que é listado

O runinst junta numa só lista, sem repetidos, o que o Windows e as suas
ferramentas sabem dos programas instalados; a coluna **Origem** diz onde cada
um foi encontrado:

- **Registo**: as entradas `Uninstall` ("Aplicações e funcionalidades"), o
  mesmo que `REG QUERY …\Uninstall`: as do sistema, nas vistas de 64 e de 32
  bits; as suas (`HKEY_CURRENT_USER`, e o seu `WOW6432Node`); e as de outros
  utilizadores (abaixo).
- **Instalador**: os produtos do Windows Installer, a partir dos registos dele
  (`MsiEnumProductsEx`): o que o `Get-WmiObject Win32_Product` e o
  `wmic product` listam, sem o efeito secundário deles (essa classe WMI manda o
  Windows Installer verificar, e às vezes reparar, cada produto). Um produto com
  entrada `Uninstall` é o mesmo programa; um sem ela aparece como componente do
  sistema.
- **Loja**: os pacotes da Loja e MSIX, como o `Get-AppxPackage` os lista, a
  partir do gestor de pacotes do Windows: o ChatGPT, o Terminal do Windows, o
  Outlook, as apps do Windows. Não têm entrada `Uninstall`. As frameworks
  (VCLibs, Windows App Runtime…), os pacotes assinados como parte do Windows e
  os que não têm uma app para abrir são componentes do sistema.
- **winget**: o `winget list`, que demora segundos, por isso é perguntado
  depois de o resto aparecer (a barra de estado di-lo entretanto). As linhas
  dele juntam-se pela chave do registo ou pelo pacote que indicam, ou senão pelo
  nome; trazem o id do winget e a **versão mais recente** que as fontes dele
  têm (a azul na coluna Versão, `9.11 → 9.12`, e contadas na barra de estado).

Para quem:

- **Para todos**: as entradas e produtos do sistema; qualquer utilizador os
  pode ler.
- **Para si**: as suas entradas, produtos e pacotes (o Chrome, o VS Code, o
  ChatGPT e afins muitas vezes são).
- **Para outros utilizadores**: as entradas deles em `HKEY_USERS` quando têm
  uma sessão aberta, e senão a partir do `NTUSER.DAT` do perfil, carregado à
  parte e só para ler; os produtos e pacotes deles. Isto precisa de direitos de
  administrador: sem eles, uma linha por cima da lista diz que utilizadores não
  se puderam ler, com **Reiniciar como administrador**.

Quando o Windows pede a palavra-passe de um administrador (outra conta), o
runinst corre como essa conta mas continua a saber que o utilizador do ambiente
de trabalho é você: os seus programas, atalhos e Scoop continuam seus, e não
aparecem repetidos.

O **Scoop** e o **Chocolatey**, quando estão instalados, têm um separador cada:
as apps nas pastas do Scoop (`SCOOP`, ou `~\scoop`, e a global) com a versão,
o bucket, a descrição e a página; os pacotes na pasta `lib` do Chocolatey com a
versão e os autores. São lidos das pastas, sem correr o `scoop` nem o `choco`.

Os atalhos são lidos do menu Iniciar e do ambiente de trabalho de todos
(`C:\ProgramData\…\Start Menu`, `C:\Users\Public\Desktop`), seus e, com
direitos de administrador, dos outros utilizadores. O runinst lê cada ficheiro
`.lnk` por si, e liga cada atalho ao seu programa pela pasta do que ele abre (a
pasta de instalação do programa, ou a pasta do ícone ou do desinstalador), ou
senão pelo nome. Um atalho cujo destino já não existe aparece a laranja.

Não listados: as funcionalidades opcionais do Windows (`dism /online
/get-features`, que precisa de direitos de administrador) e os pacotes das
ferramentas de programação (npm, pip, Cargo, vcpkg): mais tarde.

### Desinstalar

Selecione um programa, ou vários (Ctrl+clique, Shift+clique, Ctrl+A), ou marque
as caixas deles (um clique na caixa, ou Espaço para as linhas selecionadas; a
caixa do cabeçalho marca todos os mostrados ou desmarca todos). As marcas ficam
quando a pesquisa muda, para encontrar os programas um a um e marcar cada um;
enquanto houver marcados, os botões agem sobre eles. Depois escolha
**Desinstalar** (Delete) ou **Desinstalar sem perguntas** (Shift+Delete), nos
botões por cima da lista, no menu **Programa** ou no do botão direito. O runinst
diz quais vão ser desinstalados e quais não podem, e pergunta antes. Depois corre
um desinstalador de cada vez e espera que termine, junto com os processos que
ele lança (muitos desinstaladores copiam-se para a pasta temporária e lançam
essa cópia); a linha por cima da lista diz qual está a correr, com uma barra do
avanço da fila e **Parar depois deste**, e cada programa da fila fica marcado na
lista (Em espera, A desinstalar…, Feito, Não funcionou). No fim lê os programas de novo e diz o que foi desinstalado, o
que continua instalado (um desinstalador cancelado), o que não funcionou e o que
não chegou a começar; e se o Windows tem de reiniciar.

O que corre:

- o desinstalador do próprio programa (`UninstallString`); sem perguntas, o
  `QuietUninstallString`, ou `/VERYSILENT` nos do Inno Setup; sem nenhum, o
  normal;
- `msiexec /x {código}` no Windows Installer (`/qn` sem perguntas);
- nas apps da Loja, o gestor de pacotes do Windows;
- `scoop uninstall` e `choco uninstall` nos separadores deles;
- `winget uninstall --id` no que só o winget conhece.

Um desinstalador que precisa de direitos de administrador recebe a pergunta do
Windows. Os programas instalados para outro utilizador desinstalam-se na conta
dele; as suas apps da Loja também, quando o runinst corre como outra conta.
**Modificar ou reparar** corre o comando de alteração do programa
(`ModifyPath`, ou a manutenção do Windows Installer); numa app do Scoop,
`scoop install` de novo quando a instalação falhou, senão `scoop reset` (de
novo os shims, os atalhos e a versão atual). Poucos programas
o têm (o Microsoft 365, o Edge, o Visual C++ e os pacotes do Windows Installer
têm); nos outros, como nas apps do Chocolatey e da Loja, e quando a entrada diz
`NoModify` e `NoRepair`, explica porque não pode.

Os programas com um problema têm uma etiqueta na lista, e a barra de estado
conta-os: **Instalação falhou** (uma app do Scoop cuja instalação não acabou:
sem `install.json`), **Manifesto removido** (nenhum bucket tem já o manifesto
de uma app do Scoop), **Desinstalador em falta** (uma entrada cujo
desinstalador e pasta já não existem: deixada por um programa removido de outra
forma).

### Restos

Depois de desinstalar, o runinst procura o que os programas deixaram para trás
e mostra-o num separador **Restos** (fica no separador onde estava; fechar a
lista volta a ele):

- **Pasta do programa**: a pasta de instalação, se ainda existir; pastas com o
  nome do programa em Program Files, Common Files, ProgramData e
  `AppData\Local\Programs`; a pasta do editor quando não tem mais nada e nenhum
  outro programa dele está instalado. Numa app do Scoop, a pasta dela.
- **Atalho**: pastas do menu Iniciar com o nome dele, e atalhos que abrem algo
  que era dele.
- **Arranca com o Windows**: entradas `Run` que arrancam algo que era dele.
- **Registo**: as chaves dele em `HKEY_LOCAL_MACHINE\SOFTWARE` (e
  `WOW6432Node`), e a entrada `Uninstall` quando o desinstalador já não existe.
- **Serviço** e **Tarefa agendada**: os que correm algo na pasta dele, ou algo
  que desapareceu com o nome dele. As tarefas são lidas de
  `Windows\System32\Tasks`, que precisa de direitos de administrador para a
  maioria.
- **Definições do utilizador**: as pastas dele em AppData (Local, Roaming,
  LocalLow), a chave dele no `Software` do utilizador, e a pasta `persist` de
  uma app do Scoop. Não vêm marcadas: muitas vezes vale a pena manter as
  definições.
- **Encontrado pelo Everything**: com o [Everything](https://www.voidtools.com)
  a correr, pastas com o nome dele noutros sítios do disco do sistema ou do
  programa. Só contam nomes com mais de uma palavra, ou dentro da pasta do
  editor; nunca em perfis de outros utilizadores, gestores de pacotes, projetos,
  caches ou cópias de segurança. Não vêm marcadas.

Nada que um programa ainda instalado use é oferecido: nem a pasta dele, nem o
que está lá dentro, nem o que a contém; nunca as pastas do Windows, um perfil ou
um disco. O que precisa de direitos de administrador que o runinst não tem
aparece a cinzento e não se pode marcar.

Marque o que deve sair (um clique na caixa, ou Espaço) e escolha **Remover os
marcados** (Delete). O runinst pergunta antes. As chaves e valores do registo,
e as chaves dos serviços, são guardados num ficheiro `.reg` na pasta das
definições (`backups\leftovers <data>.reg`): faça duplo clique nele para os
repor. Um serviço é depois parado e apagado (`sc delete`); o ficheiro de uma
tarefa é copiado para `backups\leftovers <data>.tasks\` (`schtasks /Create
/XML` repõe-na) e a tarefa apagada. Os ficheiros e pastas vão para a
Reciclagem.

**Procurar restos** (no menu Ficheiro, ou o botão) procura o que programas
removidos antes deixaram, sem desinstalar nada:

- entradas cujo desinstalador já não existe, com tudo o que está acima para
  cada uma, como se tivessem acabado de ser desinstaladas;
- atalhos, entradas de arranque, serviços e tarefas agendadas que correm algo
  que desapareceu (de um disco que está ligado; as tarefas do próprio Windows
  ficam de fora), e a pasta em Program Files, ProgramData ou AppData para onde
  apontavam (desmarcada: é só um indício);
- pastas vazias logo abaixo de Program Files, ProgramData, AppData e dos menus
  Iniciar (não as que o Windows mantém vazias).

Os serviços encontrados só porque o que correm desapareceu também vêm
desmarcados.

### A janela

- **Escrever** em qualquer lado procura programas pelo nome, editor ou pasta
  (várias palavras: todas elas); Esc limpa a pesquisa. Ctrl+V cola nela a
  partir de qualquer lado. Na caixa, um clique põe o cursor, arrastar ou Shift
  seleciona, um duplo clique seleciona uma palavra; as setas, Home e End movem
  (com Ctrl: por palavras), Delete e Backspace apagam, Ctrl+A, Ctrl+C e Ctrl+X
  selecionam, copiam e cortam; o menu do botão direito tem Cortar, Copiar,
  Colar, Eliminar e Selecionar tudo.
- **Todos os programas / Para todos os utilizadores / Só para mim / Para outros
  utilizadores** escolhe de quem são os programas mostrados.
- **Componentes do sistema** e **Atualizações** mostram o que o Windows esconde
  da sua lista (`SystemComponent`, atualizações e KBs), a cinzento.
- Colunas: nome com o ícone do programa, editor, versão, tamanho, data de
  instalação, para quem e origem; um clique num título ordena por ele (as versões pelos
  números: 9.11 antes de 10.2).
- **Detalhes**, à direita: para quem, 64 ou 32 bits, Windows Installer (com o
  código do produto), tamanho, data, pasta de instalação, comando de
  desinstalação (e os de desinstalação silenciosa e de modificação), a chave do
  registo, os atalhos, e todos os valores da entrada.
- Os botões por cima da lista, o menu **Programa** e o do botão direito:
  Desinstalar, Desinstalar sem perguntas, Modificar ou reparar, Abrir a pasta
  de instalação (Ctrl+E), Copiar o nome (Ctrl+C), o comando de desinstalação, a
  chave do registo, todos os detalhes (Ctrl+Shift+C), Selecionar tudo (Ctrl+A).
- Com vários programas selecionados, os detalhes mostram quantos são, o tamanho
  e cada um; Copiar todos os detalhes copia-os todos.
- **Exportar para CSV** (Ctrl+S) guarda a lista mostrada. F5 lê tudo de novo.

A barra de estado conta os programas mostrados, o tamanho e os atalhos.

## Teclado

| Tecla | Faz |
| --- | --- |
| Letras | Procurar um programa pelo nome, editor ou pasta |
| Esc | Limpar a pesquisa |
| ↑ ↓ PgUp PgDn Home End | Mover na lista (com Shift: selecionar) |
| Ctrl+clique, Shift+clique | Selecionar vários programas |
| Espaço | Marcar os programas selecionados (quando não se está a escrever na pesquisa) |
| Ctrl+V | Colar na pesquisa |
| ← → Home End, Delete, Ctrl+A/C/X | Editar a pesquisa, quando está em uso |
| Ctrl+A | Selecionar todos os programas mostrados |
| Delete | Desinstalar os programas selecionados (no separador Restos: remover os marcados) |
| Shift+Delete | Desinstalá-los sem perguntas |
| F5 | Ler os programas de novo |
| Ctrl+E | Abrir a pasta de instalação |
| Ctrl+C | Copiar o nome |
| Ctrl+Shift+C | Copiar todos os detalhes |
| Ctrl+S | Guardar a lista como CSV |
| F1 | Atalhos de teclado |

## Definições

As definições (tema, língua, tamanho da janela, de quem são os programas
mostrados, componentes do sistema e atualizações) ficam em
`%APPDATA%\runinst\settings.ini`. Com um ficheiro chamado `runinst.portable` ao
lado do `runinst.exe`, ficam aí (modo portátil).

## Compilar

```powershell
./dev.ps1 build --profile release-small -p runinst
./dev.ps1 test -p runinst
./dev.ps1 clippy -p runinst --all-targets
./dev.ps1 test -p runinst --lib report '--' --ignored --nocapture   # o que este computador tem
```

O resultado é `target\release-small\runinst.exe`.

## Linux

Compila e corre (5,7 MB), mas ainda não lista nada: os pacotes do Linux (dpkg,
rpm, flatpak, snap) vêm na fase 5.

## A seguir

1. Feito: os programas de todos os utilizadores, do registo, do Windows
   Installer, da Loja e do winget, juntos; separadores do Scoop e do Chocolatey;
   atalhos, ícones, detalhes, pesquisa e CSV.
2. Feito: desinstalar, normal ou sem perguntas, um ou vários seguidos,
   esperando pelos processos que cada desinstalador lança; modificar e reparar.
3. Feito: restos depois de desinstalar (pastas, atalhos, entradas de arranque,
   chaves do registo, serviços, tarefas agendadas, definições do utilizador,
   Everything), ficheiros para a Reciclagem e chaves guardadas num `.reg`;
   Procurar restos de programas removidos antes e de entradas cujo
   desinstalador já não existe.
4. As funcionalidades opcionais do Windows (DISM); os pacotes das ferramentas de
   programação (npm, pip, Cargo, vcpkg).
5. Linux: pacotes dpkg, rpm, flatpak e snap, removidos com `pkexec`.
