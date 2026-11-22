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
| Custom Clean | – (do próprio rcare) | Lixo, duplicados e ficheiros grandes (100 MB ou mais) de uma pasta de uma vez, com um botão para rever cada um na sua página; F5 analisa. Ali não se apaga nada |
| Limpeza de disco | [rclean](../rclean/README.pt.md) | Temporários, caches dos browsers e dos gestores de pacotes, atalhos inválidos, entradas inválidas do registo |
| Ficheiros duplicados | [rdup](../rdup/README.pt.md) | Ficheiros e pastas com o mesmo conteúdo em vários discos e pastas ao mesmo tempo |
| Ficheiros grandes | [rsize](../rsize/README.pt.md) | O que ocupa espaço num disco ou numa pasta: lista, tipos de ficheiro, treemap, tarte, anéis |
| Desinstalar aplicações | [rinst](../rinst/README.pt.md) | Os programas de todos os utilizadores e fontes, desinstalados uns a seguir aos outros, e os seus restos |

A seguir: as ferramentas de velocidade, registo, rede e proteção (ver o
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

## Por fazer

Modo residente (`--tray`), "Integração com o Windows", "Procurar atualizações…"
a sério (ainda não há release), o bucket do Scoop e o workflow de release.
