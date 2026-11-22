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
(Ficheiro, Ver, Ajuda) mais os da página (Limpeza, Editar).

Páginas até agora:

| Página | A ferramenta pequena | O que faz |
| --- | --- | --- |
| Limpeza de disco | [rclean](../rclean/README.pt.md) | Temporários, caches dos browsers e dos gestores de pacotes, atalhos inválidos, entradas inválidas do registo |

A seguir: ficheiros duplicados (rdup), ficheiros grandes (rsize), desinstalador
(rinst), e depois as ferramentas de velocidade, registo, rede e proteção (ver o
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
