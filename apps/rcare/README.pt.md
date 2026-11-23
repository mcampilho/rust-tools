# rcare

Manutenção do Windows numa só janela, escrita em Rust (só Windows, em curso).

[English version](README.md)

O rcare faz parte do [rust-tools](../../README.pt.md). Junta o que várias das
ferramentas pequenas fazem, para não ser preciso abrir várias com
administração. O plano, com o que fica de fora de propósito e porquê, está em
[docs/rcare.pt.md](../../docs/rcare.pt.md). Os comandos abaixo correm na raiz
do repositório.

## Uso

```text
rcare
```

Uma barra lateral à esquerda escolhe a página; o botão **Barra lateral** (ou
Ctrl+B, ou Ver › Recolher a barra lateral) mostra só os ícones. À direita está
a página como a ferramenta pequena a mostra, e os menus são os da janela
(Ficheiro, Ver, Ajuda) mais os da página (Editar, Marcar…): cada página traz os menus da sua ferramenta pequena, menos o que a janela tem uma vez
(Sair, tema, língua, ajuda).

Páginas até agora:

| Página | A ferramenta pequena | O que faz |
| --- | --- | --- |
| System Maintenance | – (do próprio rcare) | As verificações de uma vez (F5): lixo, atalhos inválidos, entradas inválidas do registo, programas com problemas, e o que o Windows diz de cada disco; cada linha abre a página que corrige. Ali não se muda nada |
| Custom Clean | – (do próprio rcare) | Lixo, duplicados e ficheiros grandes (100 MB ou mais) de uma pasta de uma vez, com um botão para rever cada um na sua página; F5 analisa. Ali não se apaga nada |
| Limpeza de disco | [rclean](../rclean/README.pt.md) | Temporários, caches dos browsers e dos gestores de pacotes, atalhos inválidos, entradas inválidas do registo |
| Ficheiros duplicados | [rdup](../rdup/README.pt.md) | Ficheiros e pastas com o mesmo conteúdo em vários discos e pastas ao mesmo tempo |
| Ficheiros grandes | [rsize](../rsize/README.pt.md) | O que ocupa espaço num disco ou numa pasta: lista, tipos de ficheiro, treemap, tarte, anéis |
| Speed Up | – (do próprio rcare) | O Modo de Jogo do Windows, a memória (libertar a lista de espera como administrador), o processador, e os programas abertos: os marcados são convidados a fechar, como faz o botão de fechar deles |
| Startup Optimizer | – (do próprio rcare) | O que arranca com o Windows, com um interruptor cada: desligado como o Gestor de Tarefas faz, por isso pode voltar a ligar-se e nada é apagado. Os itens de todos os utilizadores precisam de administrador |
| Program Deactivator | – (do próprio rcare) | Serviços de outras empresas que arrancam com o Windows, com um interruptor cada: desligado, um serviço só arranca quando é preciso (Manual); como arrancava fica numa cópia para o ligar de novo. Alterar precisa de administrador |
| Extensions | – (do próprio rcare) | O que está acrescentado ao Chrome, Edge, Brave, Vivaldi, Opera, Chromium e Firefox, por perfil, ligado ou não, e quem o instalou; abre a página de extensões do próprio browser, onde se desliga ou remove. Não altera nada |
| Registry Cleaner | – (do próprio rcare, com o motor do rclean) | Entradas do registo que indicam o que já não existe, por categoria, guardadas num `.reg` antes de se apagarem; classes COM e serviços só mostrados; o tamanho dos ficheiros do registo |
| Protect System | – (do próprio rcare) | O antivírus e a firewall como o Windows os relata (os de outras empresas incluídos), o Microsoft Defender e os perfis da Firewall do Windows; análise rápida, definições novas e ligar o que está desligado, tudo pelas ferramentas do próprio Windows |
| Internet Booster | – (do próprio rcare) | Os valores TCP do Windows, com os alterados por outras ferramentas repostos aos do Windows, e o DNS de cada ligação: automático ou Cloudflare, Quad9, Google. Cada alteração guarda uma cópia `.cmd` que repõe o que estava |
| Desinstalar aplicações | [rinst](../rinst/README.pt.md) | Os programas de todos os utilizadores e fontes, desinstalados uns a seguir aos outros, e os seus restos |

A seguir: as ferramentas de rede, proteção e sistema (ver o
plano). As ferramentas pequenas ficam como estão: as páginas são código
partilhado, por isso uma correção faz-se uma vez.

As definições (tema, língua, janela, barra lateral, a página mostrada, as
caixas marcadas) ficam em `%APPDATA%\rcare`, ou ao lado do programa quando
existe lá um ficheiro `rcare.portable`.

## Compilar

```powershell
./dev.ps1 build --release -p rcare
./dev.ps1 test -p rcare
./dev.ps1 test -p rcare --test screens '--' --ignored   # apps/rcare/target/screens/*.png
```

Perfil `release`: não é feito para ocupar pouco.

Linha de comandos: `rcare [--tray] [--new-window] [pasta]`. Uma pasta abre a
Custom Clean nela, analisada. `--tray` fica na área de notificação (Ficheiro ›
Integração com o Windows › Iniciar com o Windows arranca-o assim); Ficheiro ›
Integração com o Windows › Menu do Explorador acrescenta "Analisar com o rcare"
às pastas e discos (ou `--register` / `--unregister`).

## Por fazer

"Procurar atualizações…" a sério (ainda não há release) e o bucket do Scoop.
