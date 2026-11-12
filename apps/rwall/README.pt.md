# rwall

Muda o fundo do ambiente de trabalho no Windows e no [Linux](#linux); portátil,
escrito em Rust. Põe uma imagem nova no ambiente de trabalho todos os dias,
duas ou quatro vezes por dia, todas as semanas, todos os meses ou a cada tantos
minutos. As imagens vêm da imagem do dia do Bing, da Astronomy Picture of the
Day da NASA, do Wallhaven, do Lorem Picsum ou das suas pastas. No Windows pode
também pôr um **fundo dinâmico** atrás dos ícones do ambiente de trabalho: um
vídeo, um GIF ou WebP animado, ou uma cena desenhada pelo rwall.

[English version](README.md)

O rwall faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo correm
na raiz do repositório.

O `rwall.exe` é um único executável de 3,2 MB: sem instalador, sem runtime, sem
DLLs além das que vêm com o Windows. É a única ferramenta do rust-tools que usa
a rede: descarrega imagens dos serviços que ligar, e mais nada. Sem conta, sem
cookies, sem telemetria.

## Utilização

```text
rwall [--tray | --auto]
```

| Comando | O que faz |
| --- | --- |
| `rwall` | Abre as definições: a imagem que está no ambiente de trabalho, **Imagem seguinte**, de onde vêm as imagens, de quanto em quanto tempo e como enchem o ecrã. Fechar a janela termina o rwall. A imagem só muda quando se pede: a frequência escolhida precisa do `--tray`. |
| `rwall --tray` | Fica na área de notificação e muda a imagem com a frequência escolhida. |
| `rwall --auto` | Muda a imagem uma vez com as definições guardadas, sem janela, e termina: para o Agendador de Tarefas, um atalho ou um script. Com o rwall já a correr, é esse rwall que a muda. |
| `rwall --help` | Escreve isto na consola onde foi escrito, ou mostra-o numa caixa de mensagem. |

Só corre um rwall de cada vez: arrancá-lo outra vez mostra as definições do que
está a correr.

**Modo residente** (`--tray`): um clique no ícone da área de notificação abre
as definições; o menu (clique direito) tem **Imagem seguinte**, **Sobre esta
imagem** (a página sobre ela, no navegador), **Definições…**, **Mostrar o
ficheiro** e **Sair**. Fechar as definições só as esconde (o Esc também). Da
primeira vez que arranca assim, o rwall mostra as definições e põe a imagem do
dia do Bing no ambiente de trabalho. **Iniciar com o Windows**, nas definições,
arranca-o desta maneira.

Na janela das definições, o Esc fecha-a e o F5 é **Imagem seguinte**.

### Quando muda a imagem (com `--tray`)

| Escolha | Muda |
| --- | --- |
| Todos os dias (predefinido) | à meia-noite |
| Duas vezes por dia | às 00:00 e às 12:00 |
| Quatro vezes por dia | às 00:00, 06:00, 12:00 e 18:00 |
| Todas as semanas | à segunda-feira, à meia-noite |
| Todos os meses | no dia 1, à meia-noite |
| A cada … min | de 5 a 480 minutos (−/+), contados a partir da meia-noite |

As horas são as do relógio do computador. O rwall vê as horas de meio em meio
minuto: se o computador estava em suspensão, desligado ou sem o rwall à hora de
uma mudança, a imagem muda logo que volta. Escolher outra frequência não muda a
imagem logo; conta a partir daí. **Imagem seguinte** muda-a quando quiser, e a
mudança marcada seguinte continua à sua hora.

Quando uma mudança falha (sem rede, um serviço em baixo), a linha de estado nas
definições diz porquê, e o rwall tenta outra vez dez minutos depois.

### De onde vêm as imagens

Marque quantas fontes quiser. Revezam-se: o Bing, depois a NASA, depois as suas
pastas, e assim por diante. Quando uma falha, passa logo à seguinte. Uma imagem
mostrada há pouco (as últimas 200) não volta a ser escolhida enquanto houver
outras.

| Fonte | O que dá | Definições |
| --- | --- | --- |
| Bing – imagem do dia | A imagem do bing.com em UHD, com o local e os créditos; uma dos últimos oito dias quando a de hoje já foi mostrada | **Região**: o mercado do Bing, como `pt-PT` ou `en-GB` (vazio: a língua do rwall) |
| NASA – imagem astronómica do dia | O dia mais recente ainda não mostrado, ou um dia do arquivo ao acaso (os dias com vídeo são saltados), com a sua página em science.nasa.gov | – (sem chave) |
| Wallhaven – pesquisa | Um wallpaper ao acaso, próprio para o trabalho, horizontal, pelo menos do tamanho do ecrã | **Pesquisar**: palavras como `mountains`; vazio: qualquer tema |
| Picsum – fotografia aleatória | Uma fotografia ao acaso no tamanho exato do ecrã, com o autor | – |
| As minhas pastas | Uma imagem ao acaso das suas pastas (e subpastas, se marcado) | **Acrescentar uma pasta…**, × para tirar uma |

As imagens da NASA vêm da lista do APOD em science.nasa.gov
(`wp-json/wp/v2/apod-basic`), para onde o APOD passou do `api.nasa.gov`. Depois
de um erro do servidor, o rwall pergunta mais duas vezes e depois passa à fonte
seguinte.

As imagens das pastas podem ser JPEG, PNG, BMP, GIF, TIFF ou WebP. As imagens
descarregadas são verificadas: o que não for uma imagem, ou for menor do que
320×320, é recusado.

### Posição da imagem e o resto

- **Posição da imagem**: Preencher (cobre o ecrã, cortando o que sobra), Ajustar
  (toda, com barras), Esticar, Centrar ou Expandir (uma imagem por todos os
  ecrãs). Aplica-se logo à imagem no ambiente de trabalho.
- **Imagens descarregadas a guardar**: as 30 mais recentes por predefinição (de
  1 a 500); as mais antigas são apagadas depois de cada mudança. As imagens das
  suas pastas nunca são mexidas.
- **Iniciar com o Windows** acrescenta `"rwall.exe" --tray` a
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (só o utilizador atual);
  desmarcar tira-o. No Linux é **Iniciar ao entrar na sessão**, um ficheiro em
  `~/.config/autostart`.

## Fundo dinâmico (Windows)

O separador **Fundo dinâmico** das definições põe algo a mexer atrás dos ícones
do ambiente de trabalho, no lugar da imagem parada:

| Escolha | O que passa |
| --- | --- |
| Um vídeo | Um ficheiro de vídeo (MP4, MOV, WMV, AVI, MKV, WebM…: o que o Windows conseguir reproduzir), em ciclo, sem som |
| Vídeos de uma pasta | Um vídeo ao acaso de uma pasta (e das subpastas, se marcado); com `--tray`, outro a cada mudança escolhida no separador Fundo estático. **Vídeo seguinte** muda-o quando quiser |
| Uma imagem animada | Um GIF ou um WebP animado, à sua velocidade |
| Uma cena desenhada pelo rwall | **Aurora boreal**, **Céu estrelado** (roda devagar, com uma estrela cadente de vez em quando), **Mar ao pôr do sol** ou **Fluxo de cores**: sem ficheiro |

Ícones, ficheiros, notificações, widgets e a barra de tarefas continuam por
cima, como sempre: só se mexe a imagem por trás deles. **Ligar** e **Desligar**
começam e param o fundo dinâmico; quando para, a imagem parada volta. Enquanto
passa, a imagem parada não muda sozinha.

- **Posição em cada ecrã**: Preencher (cobre cada ecrã, cortando o que sobra)
  ou Ajustar (toda, com barras pretas). Com vários ecrãs, cada um mostra o
  vídeo inteiro.
- **Pausar quando** (tudo marcado por predefinição): um programa ou jogo está
  em ecrã inteiro; janelas maximizadas tapam todos os ecrãs; o computador está
  bloqueado ou o ecrã desligado; o computador está na bateria. Em pausa, fica a
  última imagem. O menu do ícone tem também **Pausar o fundo dinâmico**.
- Passa enquanto o rwall corre: com `--tray` sempre (e logo ao entrar, com
  **Iniciar com o Windows**), sem ele enquanto a janela das definições estiver
  aberta. O `--auto` não lhe mexe.

Gasta pouco: um vídeo é descodificado pela placa gráfica (cerca de 1–2 % de um
núcleo para um vídeo 1080p), e as imagens de uma cena ou de uma animação são
feitas pequenas e esticadas pela placa gráfica (menos de 10 % de um núcleo).

## Como funciona

- **Transferências**: WinHTTP, o HTTPS do sistema com as definições de proxy
  (o programa `curl` no Linux), com 60 segundos de limite e limites de tamanho
  (4 MB para uma resposta, 64 MB para uma imagem). As respostas dos serviços
  são lidas com um pequeno leitor de JSON do próprio rwall.
- **O ambiente de trabalho**: `SystemParametersInfoW(SPI_SETDESKWALLPAPER)` com
  a posição em `HKCU\Control Panel\Desktop` (`WallpaperStyle`), como as
  definições do próprio Windows. A mesma imagem vai para todos os ecrãs. As
  imagens WebP, que o Windows pode não ler sem um codec extra, são primeiro
  convertidas para PNG.
- **Atrás dos ícones**: o Windows não tem uma maneira pública de desenhar aí.
  Como todos os programas de fundos animados, o rwall manda à janela do ambiente
  de trabalho (`Progman`) a mensagem não documentada `0x052C`, que faz o
  Explorador pôr uma janela `WorkerW` atrás da camada dos ícones
  (`SHELLDLL_DefView`); as janelas do rwall (uma por ecrã) passam a ser filhas
  dela. Desde o Windows 11 24H2 a camada dos ícones fica dentro do `Progman`:
  aí as janelas do rwall ficam logo abaixo dela. Quando o Explorador reinicia
  ou os ecrãs mudam, o rwall volta a pô-las no sítio. Se o rwall for terminado
  à força, a última imagem fica até o fundo voltar a ser pintado (pelo rwall
  ou pelo Windows).
- **Reprodução**: o motor de multimédia do Media Foundation reproduz o vídeo
  numa swap chain do Direct3D 11 por ecrã. As cenas e as animações são
  desenhadas no processador a 960×540 (ou menos) e esticadas pela swap chain.
- **Ficheiros**: as definições em `settings.ini` e as imagens descarregadas em
  `images\`, ambas em `%APPDATA%\rwall` (Windows) ou `~/.config/rwall` (Linux),
  ou ao lado do `rwall.exe` quando lá existe um ficheiro `rwall.portable` (modo
  portátil).

## Linux

A mesma janela de definições (sem o fundo dinâmico, por agora só no Windows)
e, com `--tray`, o mesmo menu num ícone
StatusNotifierItem. Sem um anfitrião para ele (GNOME simples), o rwall
residente mantém a janela, minimizada quando se fecha. A imagem é posta com o
`gsettings` (GNOME, Cinnamon e afins), o `plasma-apply-wallpaperimage` (KDE
Plasma), o `xfconf-query` (XFCE) ou o `feh` (outros gestores de janelas), e as
transferências usam o programa `curl`, que tem de estar instalado. Compilado e
testado no WSL2, ainda não corrido num ambiente de trabalho Linux.

## Compilar e testar

```powershell
./dev.ps1 build --release -p rwall          # target\release\rwall.exe
./dev.ps1 test -p rwall
./dev.ps1 clippy -p rwall --all-targets
./dev.ps1 test -p rwall --lib screenshots '--' --ignored        # apps\rwall\target\screens\*.png
./dev.ps1 test -p rwall --lib live_services '--' --ignored --nocapture   # precisa de rede
./dev.ps1 test -p rwall --lib scene_pictures '--' --ignored      # uma imagem de cada cena, flow.gif
./dev.ps1 test -p rwall --release --lib scene_speed '--' --ignored --nocapture
```

O rwall usa o perfil `release` (não o `release-small`): uma imagem malformada
vinda da internet não o pode fechar, por isso os pânicos são desenrolados e
apanhados.

## Licença

MIT. Ver [LICENSE](../../LICENSE). As imagens pertencem aos seus autores: veja
os créditos mostrados para cada uma e os termos de cada serviço.
