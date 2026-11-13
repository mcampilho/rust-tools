# rclean

Um limpador portátil para Windows e Linux, escrito em Rust. Mostra quanto
ocupam os ficheiros temporários do sistema e as caches dos browsers, e apaga o
que marcar, depois de perguntar.

[English version](README.md)

O rclean faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo
correm na raiz do repositório.

O `rclean.exe` é um único executável de 1,8 MB: sem instalador, sem runtime,
sem DLLs além das que vêm com o Windows. Não faz ligações à rede e não tem
telemetria. Está a ser feito por fases: limpa os ficheiros temporários, as
caches dos browsers e as dos gestores de pacotes; os atalhos inválidos e as
entradas inválidas do registo vêm a seguir (ver [Fases](#fases)).

## Utilização

```text
rclean
```

Ao arrancar, o rclean vê logo o que cada item tem (**Analisar**, F5, volta a
fazê-lo) e mostra, em cada um, quantos ficheiros e quanto espaço; o título de
cada grupo (Windows ou Linux, e depois cada browser encontrado) soma os seus
itens. Marque o que quer ver sair (um clique na caixa, Espaço na linha
selecionada, a caixa do grupo para o grupo inteiro) e carregue em **Limpar**
(Ctrl+L): o rclean diz o que vai apagar e quanto, e pergunta antes de começar.
Depois volta a analisar. **Parar** (Esc) para depois do item em curso.

As marcas ficam guardadas. **Marcar os habituais** volta às que vêm marcadas à
partida; as outras (miniaturas, caches de shaders, a Reciclagem) ficam à sua
escolha.

### O que é limpo

| Item | Onde | Marcado à partida |
| --- | --- | --- |
| Ficheiros temporários | `%TEMP%`: ficheiros sem alterações há um dia | sim |
| Ficheiros temporários do Windows | `C:\Windows\Temp`: ficheiros sem alterações há um dia (administrador) | sim |
| Transferências do Windows Update | `C:\Windows\SoftwareDistribution\Download` (administrador) | sim |
| Relatórios de erros | `%LOCALAPPDATA%\Microsoft\Windows\WER`, `%LOCALAPPDATA%\CrashDumps` | sim |
| Relatórios de erros do sistema | `C:\ProgramData\Microsoft\Windows\WER\ReportArchive` e `ReportQueue` (administrador) | sim |
| Despejos de memória | `C:\Windows\Minidump`, `C:\Windows\MEMORY.DMP` (administrador) | sim |
| Cache de miniaturas | `thumbcache_*` em `%LOCALAPPDATA%\Microsoft\Windows\Explorer` | não |
| Caches de shaders gráficos | DirectX (`D3DSCache`), NVIDIA e AMD | não |
| Reciclagem | a de todas as unidades, através do Windows | não |
| Cache de um browser | `Cache`, `Code Cache`, `GPUCache` e caches do Service Worker de cada perfil, e as caches de shaders: Chrome, Edge, Brave, Vivaldi, Opera, Chromium; `cache2` e `startupCache` do Firefox, também LibreWolf, Waterfox e Floorp | sim |

Só o que existe para deitar fora: o histórico, os cookies, as palavras-passe,
as sessões e as definições dos browsers nunca são tocados. Um browser aparece
quando tem cache.

### Gestores de pacotes

Cada gestor de pacotes encontrado (no `PATH`, ou pela sua pasta) tem um grupo
próprio. A cache é medida pelas pastas e limpa pelo comando do próprio gestor,
para ele manter os seus registos certos; o Cargo e o vcpkg não têm esse
comando e perdem as pastas.

| Gestor | Item | Limpo com | Marcado à partida |
| --- | --- | --- | --- |
| Scoop | Transferências (`<scoop>\cache`) | `scoop cache rm *` | sim |
| Scoop | Versões antigas (as que ficam depois de atualizar) | `scoop cleanup *` | não |
| Chocolatey | Cache | `choco cache remove` (administrador) | sim |
| pip | Cache | `python -m pip cache purge` | sim |
| npm | Cache (onde o `npm config get cache` diz) | `npm cache clean --force` | sim |
| Yarn | Cache | `yarn cache clean` | sim |
| pnpm | Repositório: pacotes que nenhum projeto usa | `pnpm store prune` | não |
| Go | Cache de compilação | `go clean -cache` | sim |
| NuGet | Cache HTTP (os pacotes que os projetos usam ficam) | `dotnet nuget locals http-cache --clear` | sim |
| Cargo | `registry/cache`, `registry/src`, `git/checkouts` | apagados | sim |
| vcpkg | `downloads`, `buildtrees`, `packages` | apagados | sim |

Em Linux também o apt (`apt-get clean`), o dnf (`dnf clean all`), o zypper e o
pacman, através do `pkexec` (pede a palavra-passe); os runtimes do Flatpak que
já nada usa (`flatpak uninstall --unused`, tamanho só conhecido depois) e as
revisões antigas do Snap (`snap remove <nome> --revision=<n>`), ambos
desmarcados à partida. Quando um comando não funciona, o resumo final diz o
que ele escreveu.

Os itens marcados "administrador" precisam de direitos de administrador: sem
eles aparecem a cinzento, com **Reiniciar como administrador** por cima da
lista.

### Como apaga

- Os temporários e as caches são apagados de vez, não vão para a Reciclagem:
  são feitos para deitar fora, e só a encheriam.
- Pastas temporárias: só os ficheiros sem alterações há um dia, para nada que
  um programa aberto esteja a usar sair.
- Um ficheiro em uso não se pode apagar: fica, e é contado ("em uso ficaram").
  Quando um browser está aberto, a linha dele di-lo; feche-o primeiro para o
  limpar todo.
- Ligações e junções lá dentro não são seguidas nem apagadas. As próprias
  pastas ficam (sai só o que têm); as pastas lá dentro que ficam vazias são
  removidas.
- Os ficheiros só de leitura também são apagados.

## Teclado

| Tecla | Ação |
| --- | --- |
| F5 | Analisar de novo |
| Ctrl+L | Limpar o que está marcado |
| Esc | Parar |
| ↑ ↓ PgUp PgDn Home End | Mover na lista |
| Espaço | Marcar ou desmarcar a linha selecionada |
| Ctrl+A | Marcar tudo |
| Ctrl+E | Abrir a pasta |
| Ctrl+Shift+C | Copiar os detalhes |
| F1 | Atalhos de teclado |

## Definições

O tema, a língua, o tamanho da janela e as marcas ficam em `settings.ini`, em
`%APPDATA%\rclean` (Windows) ou `~/.config/rclean` (Linux), ou ao lado do
programa quando lá existe um ficheiro `rclean.portable`.

## Compilar

```powershell
./dev.ps1 build --profile release-small -p rclean
./dev.ps1 test -p rclean
```

O resultado é `target\release-small\rclean.exe`.

## Linux

Um ficheiro de 5,5 MB. Os itens são do utilizador: ficheiros temporários em
`/tmp` e `/var/tmp` (só os seus, sem alterações há um dia), miniaturas
(`~/.cache/thumbnails`), caches de shaders do Mesa e da NVIDIA, o lixo
(`~/.local/share/Trash`) e as caches dos browsers (em `~/.config` e `~/.cache`;
o Firefox também do Snap e do Flatpak), e os gestores de pacotes acima.

## Fases

1. Feito: ficheiros temporários do Windows e do Linux, relatórios de erros,
   despejos de memória, miniaturas, caches de shaders, a Reciclagem e as caches
   dos browsers; analisar, marcar e limpar.
2. Feito: as caches dos gestores de pacotes, pelos comandos de cada um: Scoop,
   Chocolatey, pip, npm, Yarn, pnpm, Go, NuGet, Cargo e vcpkg; apt, dnf,
   zypper, pacman, Flatpak e Snap em Linux.
3. Atalhos inválidos (menu Iniciar, ambiente de trabalho, ficheiros `.desktop`
   do Linux).
4. Entradas inválidas do registo, guardadas num `.reg` antes de saírem.
