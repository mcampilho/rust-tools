# rwall

Muda o fundo do ambiente de trabalho no Windows e no [Linux](#linux); portátil,
escrito em Rust. Põe uma imagem nova no ambiente de trabalho todos os dias,
duas ou quatro vezes por dia, todas as semanas, todos os meses ou a cada tantos
minutos. As imagens vêm da imagem do dia do Bing, da Astronomy Picture of the
Day da NASA, do Wallhaven, do Lorem Picsum ou das suas pastas. No Windows pode
também pôr um **fundo dinâmico** atrás dos ícones do ambiente de trabalho (um
vídeo, um GIF ou WebP animado, ou uma cena desenhada pelo rwall) e **widgets**
no ambiente de trabalho: relógios, calendário, notas adesivas, anotações,
meteorologia, fences e fotos.

[English version](README.md)

O rwall faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo correm
na raiz do repositório.

O `rwall.exe` é um único executável de 3,7 MB: sem instalador, sem runtime, sem
DLLs além das que vêm com o Windows. É a única ferramenta do rust-tools que usa
a rede: descarrega imagens dos serviços que ligar, e a meteorologia para um
widget de meteorologia, e mais nada. Sem conta, sem cookies, sem telemetria.

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
dia do Bing no ambiente de trabalho. **Iniciar com o Windows**, no canto
superior direito das definições, ao lado dos separadores, arranca-o desta maneira.

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
- **Iniciar com o Windows** (ao lado dos separadores, para todo o rwall) acrescenta `"rwall.exe" --tray` a
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

## Widgets (Windows)

O separador **Widgets** das definições acrescenta widgets ao ambiente de
trabalho e mostra-os numa lista. Ficam por cima do fundo e dos ícones e por
baixo de todas as janelas, como se fizessem parte do ambiente de trabalho. Cada
um pode, em vez disso, ficar **Sempre visível**, por cima de todas as janelas.

| Widget | O que mostra |
| --- | --- |
| Relógio analógico | Um mostrador redondo, com o dia; o ponteiro dos segundos pode ser desligado |
| Relógio digital | As horas em grande (24 horas ou AM/PM, com ou sem segundos) e a data |
| Calendário | O mês, com o dia de hoje marcado e os feriados nacionais do país das definições regionais do Windows a vermelho (o próximo, ou o que está debaixo do ponteiro, com o nome por baixo do mês); ‹ › para os meses antes e depois, um clique no título volta a hoje |
| Relógio e calendário | Os dois lado a lado |
| Nota adesiva | Uma nota colorida para escrever (seis cores); o texto fica guardado tal como é escrito |
| Anotação | Texto diretamente sobre o fundo, sem caixa, em três tamanhos e quatro cores: etiquetas e lembretes |
| Meteorologia | O tempo agora e nos três dias seguintes, para uma cidade escrita no separador (`Porto` ou `Porto, Portugal`; ao início, a cidade do fuso horário do computador), em °C ou °F |
| Fence | Uma caixa com os ficheiros e pastas de uma pasta, com os ícones e miniaturas do Explorador |
| Fotos | As fotos de uma pasta (e das subpastas), umas atrás das outras, de 1 a 240 minutos |

- **Mover** um widget: arrastá-lo (uma nota pela barra de cima); encosta-se
  às margens do ecrã. **Mudar o tamanho**: arrastar o canto inferior direito.
  **Bloquear posição** (no menu dele) evita movê-lo sem querer.
- **Clique direito** num widget para as opções: cores e tamanhos, segundos e
  relógio de 24 horas, Atualizar e °F na meteorologia, Foto seguinte, Sempre
  visível, Bloquear posição, Definições do widget… e Remover o widget.
- **Notas e anotações**: um clique numa nota para escrever (duplo clique numa
  anotação); setas, Home/End, Ctrl+A, Ctrl+C/X/V e Ctrl+Backspace funcionam
  como de costume; o Esc ou um clique noutro sítio termina a escrita. O texto
  é guardado à medida que se escreve.
- **Fences**: um duplo clique abre um ficheiro ou pasta; os ficheiros podem ser
  arrastados para fora de uma fence (para o ambiente de trabalho, uma pasta, um
  programa) e largados numa. Um ficheiro largado a partir do mesmo disco é
  movido para lá, de outro disco é copiado (Ctrl copia, Shift move), com o
  progresso, as perguntas e o anular do próprio Explorador. Os ficheiros
  ocultos e de sistema continuam escondidos.
- **Meteorologia**: dados de [Open-Meteo.com](https://open-meteo.com/) (grátis,
  sem chave, sem conta), pedidos outra vez de meia em meia hora. A cidade é
  procurada uma vez; fica o lugar mais populoso com esse nome, a não ser que
  se escreva uma região ou um país depois de uma vírgula.
- **Definições regionais**: um relógio novo usa 24 horas ou AM/PM, e uma
  meteorologia nova °C ou °F, conforme o formato regional do Windows; os
  calendários começam a semana no primeiro dia dele (sábado, domingo ou
  segunda). A meteorologia começa com a cidade do fuso horário (`Lisbon, PT`
  para Europe/Lisbon), que se pode mudar.
- **Feriados**: os feriados nacionais de Portugal, Brasil, Espanha, México,
  Estados Unidos, Reino Unido (Inglaterra e País de Gales), França, Alemanha
  e Itália, calculados para qualquer ano (sem os regionais e municipais).
  **Feriados**, no separador, desliga-os num calendário.
- **Quantos**: um relógio, calendário e meteorologia de cada tipo (o botão de
  acrescentar fica cinzento enquanto houver um), notas, anotações, fences e
  fotos à vontade, até 16 widgets ao todo. Um widget novo vai para o primeiro
  sítio livre a partir do canto superior direito do ecrã principal.
- Os widgets aparecem enquanto o rwall corre: com `--tray` sempre, sem ele
  enquanto a janela das definições estiver aberta. **Mostrar os widgets no
  ambiente de trabalho** esconde-os ou mostra-os todos.

Gastam pouco: nove widgets, três deles com segundos, usam menos de 1 % de um
núcleo e cerca de 36 MB.

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
- **Widgets**: cada um é uma janela em camadas (`WS_EX_LAYERED`, transparência
  por píxel com `UpdateLayeredWindow`), desenhada pelo próprio rwall. Um widget
  preso ao ambiente de trabalho é *propriedade* da janela que tem os ícones, por
  isso o Windows mantém-no logo acima deles; o rwall segura o seu lugar entre as
  janelas, mesmo enquanto se escreve numa nota. As fences usam o arrastar e
  largar do OLE e as operações de ficheiros do Explorador (`SHFileOperationW`).
  Quando o Explorador reinicia ou os ecrãs mudam, os widgets voltam ao sítio.
- **Ficheiros**: as definições em `settings.ini` e as imagens descarregadas em
  `images\`, ambas em `%APPDATA%\rwall` (Windows) ou `~/.config/rwall` (Linux),
  ou ao lado do `rwall.exe` quando lá existe um ficheiro `rwall.portable` (modo
  portátil).

## Linux

A mesma janela de definições (sem o fundo dinâmico e os widgets, por agora só
no Windows) e, com `--tray`, o mesmo menu num ícone
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
./dev.ps1 test -p rwall --lib widget_pictures '--' --ignored     # todos os widgets, claros, escuros e a 150 %
./dev.ps1 test -p rwall --lib weather_live '--' --ignored --nocapture   # Open-Meteo, precisa de rede
./dev.ps1 test -p rwall --lib pinned_window_order '--' --ignored --nocapture
```

O rwall usa o perfil `release` (não o `release-small`): uma imagem malformada
vinda da internet não o pode fechar, por isso os pânicos são desenrolados e
apanhados.

## Licença

MIT. Ver [LICENSE](../../LICENSE). As imagens pertencem aos seus autores: veja
os créditos mostrados para cada uma e os termos de cada serviço.
