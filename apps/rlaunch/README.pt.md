# rlaunch

Um menu portátil de aplicações para Windows e Linux, escrito em Rust. Junta os
programas instalados com o Scoop, os do menu Iniciar e os das tuas pastas num
só menu, por categorias, com uma caixa de procura.

[English version](README.md)

O rlaunch faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo
correm na raiz do repositório. **Está a ser feito por fases**: ver [Fases](#fases).

O `rlaunch.exe` é um único executável com cerca de 1,9 MB: sem instalador nem
runtime. Não faz ligações à rede (salvo **Procurar atualizações**, quando o
escolhes) e não tem telemetria.

## Utilização

```text
rlaunch [--tray]
```

O rlaunch fica na área de notificação. O menu abre pelo ícone, pelo atalho de
teclado (**Ctrl+Alt+Espaço**) ou arrancando o rlaunch outra vez, junto à área de
notificação do ecrã onde está o ponteiro, e fecha quando outra coisa recebe o foco. `--tray`
arranca sem abrir o menu.

- **À esquerda:** as categorias; um clique (ou Enter) abre uma, a seta no topo
  volta atrás.
- **Procura:** é só escrever. Os acentos não contam. Cima, Baixo e Enter
  escolhem; Esc limpa, volta atrás e depois fecha.
- **À direita:** as pastas Documentos, Transferências, Imagens, Música e Vídeos,
  **Explorar**, Opções, Acerca de e Sair.
- **Em baixo:** o espaço livre do disco.
- Os **favoritos** ficam no topo, acima das categorias.
- **Afixar** (ao lado da procura): o menu fica aberto quando outra coisa recebe
  o foco, para se poder arrastar ficheiros para ele.

### Os teus programas

Botão direito num programa (ou a tecla Menu, ou Shift+F10): **Abrir**,
**Executar como administrador** (também Ctrl+Shift+Enter; Windows), **Executar sempre como administrador** (o programa
fica com um escudo), **Abrir
localização do ficheiro**, **Adicionar aos favoritos**, **Mudar o nome** (um nome
vazio repõe o original), **Categoria**, **Mudar o ícone…** (`.ico`, `.png`, um
programa ou um atalho) e **Ocultar**. Os ocultos estão em **Opções > Ocultos**,
onde o mesmo menu os volta a mostrar.

**Opções > Adicionar programa…** acrescenta qualquer ficheiro, um programa
ou um documento; arrastá-lo para o menu afixado faz o mesmo (uma pasta arrastada
junta-se às tuas pastas). O menu dele tem **Remover do menu**. Os atalhos e os
programas nunca são alterados: o que escolhes fica nas definições do rlaunch.

**Modo admin**, na coluna da direita (ou **Reiniciar como administrador** no
menu do ícone), arranca o rlaunch outra vez com direitos de administrador; para
voltar ao normal, **Sair** e arrancar de novo.

Quando o próprio rlaunch corre como administrador, um escudo toma o lugar do
logótipo e uma linha avisa: o que ele abrir também fica com esses direitos.

### Opções

Separador **Programas**:

- **Programas de:** os atalhos que o Scoop cria no menu Iniciar (`Scoop Apps`),
  o resto do menu Iniciar e **As minhas pastas**: qualquer pasta de programas ou
  atalhos, seguida até três níveis (ver [PortableApps](#portableapps)).
- **Iniciar com o Windows** e **Arrancar sempre como administrador** (Windows):
  o Windows pede confirmação em cada arranque, também quando o rlaunch inicia
  com a sessão; se recusares, arranca sem esses direitos.
- **Atalho de teclado:** Ctrl+Alt+Espaço, Ctrl+Shift+Espaço, Alt+Shift+Espaço,
  Win+Shift+Espaço ou desativado. Quando outro programa já tem a combinação, o
  menu avisa: escolhe outra.
- Língua.

Separador **Aspeto**:

- **Tema** (claro, escuro ou como o sistema) e **Cor** da moldura: sete cores ou
  uma feita a partir da cor de realce do sistema.
- **Tamanho:** 100% (350×480), 115% ou 130%. **Linhas:** compactas, ou grandes
  com ícones maiores.
- **Transparência** (Windows): desativada, ou de 10% a 40%.
- **Posição** (Windows): à direita (junto ao relógio), à esquerda, ao centro ou
  junto ao ponteiro.

Para cores à tua medida, põe um ficheiro `theme.ini` ao lado do `settings.ini`.
É lido sempre que o menu abre; cada linha substitui uma cor, e as que faltam
ficam como estão:

```ini
frame=#202040
accent=#FF8800
list=#FFFFFF
text=#101010
```

Os nomes são `frame`, `frame_text`, `frame_dim`, `frame_hover`, `list`, `text`,
`dim`, `border`, `hover`, `accent`, `on_accent` e `warn`.

As categorias são deduzidas do nome dos programas conhecidos; os outros vão para
**Outros**. Os desinstaladores, manuais e atalhos para páginas ficam de fora, e
um programa com o mesmo nome em dois sítios aparece uma vez (primeiro o do
Scoop).

## PortableApps

Uma pasta com aplicações no formato da [PortableApps.com](https://portableapps.com)
é lida como tal, onde quer que esteja nas tuas pastas: cada aplicação diz o seu
nome (sem "Portable"), categoria e programas, e nada mais lá dentro é listado.
Adicionar a pasta (ou a pasta que contém `PortableApps`) traz também os
favoritos e os programas com nome ou categoria mudados e ocultos do menu da
PortableApps; o que já escolheste no rlaunch mantém-se. Os ficheiros da suite só
são lidos.

As outras suites não são lidas no seu próprio formato: adiciona a pasta de
programas delas.

### Numa pen USB

Cria um ficheiro vazio chamado `rlaunch.portable` ao lado do `rlaunch.exe`: as
definições passam a ficar ali, as pastas do mesmo disco são encontradas quando a
pen muda de letra, **Explorar** abre a pen e a barra mostra o seu espaço.
Desliga o Scoop e o menu Iniciar para ver só o que está na pen. Da primeira
vez, uma pasta PortableApps ao lado da do rlaunch, ou nos sítios habituais da
pen, é encontrada e acrescentada.

Sem esse ficheiro, as definições ficam em `%APPDATA%\rlaunch`.

## Linux

O menu lê as aplicações instaladas (ficheiros `.desktop`, com as suas próprias
categorias) e as tuas pastas. Não tem atalho de teclado próprio (associa um a
`rlaunch` nas definições do ambiente de trabalho) e ainda não mostra ícones. Sem
área de notificação, fechar o menu termina o programa.

## Fases

1. **Feito:** o menu na área de notificação com o atalho de teclado, Scoop, menu
   Iniciar e pastas próprias, categorias, procura, abrir programas.
2. **Feito:** favoritos, ocultar, mudar o nome, a categoria e o ícone,
   acrescentar um programa solto, executar como administrador.
3. **Feito:** temas e aspeto.
4. **Feito para a PortableApps:** as aplicações e as escolhas do menu dela. As
   outras suites ainda não são lidas no seu próprio formato.
5. Linux completo (ícones).

## Compilar

```powershell
./dev.ps1 build --profile release-small -p rlaunch
./dev.ps1 test -p rlaunch
```
