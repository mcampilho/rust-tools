# rlaunch

Um menu portátil de aplicações para Windows e Linux, escrito em Rust. Junta os
programas instalados com o Scoop, os do menu Iniciar e os das tuas pastas num
só menu, por categorias, com uma caixa de procura.

[English version](README.md)

O rlaunch faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo
correm na raiz do repositório. **É uma primeira fase**: ver [Fases](#fases).

O `rlaunch.exe` é um único executável com cerca de 1,7 MB: sem instalador nem
runtime. Não faz ligações à rede (salvo **Procurar atualizações**, quando o
escolhes) e não tem telemetria.

## Utilização

```text
rlaunch [--tray]
```

O rlaunch fica na área de notificação. O menu abre pelo ícone, pelo atalho de
teclado (**Ctrl+Alt+Espaço**) ou arrancando o rlaunch outra vez, junto ao canto
do ecrã onde está o ponteiro, e fecha quando outra coisa recebe o foco. `--tray`
arranca sem abrir o menu.

- **À esquerda:** as categorias; um clique (ou Enter) abre uma, a seta no topo
  volta atrás.
- **Procura:** é só escrever. Os acentos não contam. Cima, Baixo e Enter
  escolhem; Esc limpa, volta atrás e depois fecha.
- **À direita:** as pastas Documentos, Transferências, Imagens, Música e Vídeos,
  **Explorar**, Opções, Acerca de e Sair.
- **Em baixo:** o espaço livre do disco.

### Opções

- **Programas de:** os atalhos que o Scoop cria no menu Iniciar (`Scoop Apps`),
  o resto do menu Iniciar e **As minhas pastas**: qualquer pasta de programas ou
  atalhos, seguida até três níveis.
- **Tamanho:** 100% (350×480), 115% ou 130%.
- **Transparência** (Windows): desativada, ou de 10% a 40%.
- **Atalho de teclado:** Ctrl+Alt+Espaço, Ctrl+Shift+Espaço, Alt+Shift+Espaço,
  Win+Shift+Espaço ou desativado. Quando outro programa já tem a combinação, o
  menu avisa: escolhe outra.
- Iniciar com o Windows, tema, língua.

As categorias são deduzidas do nome dos programas conhecidos; os outros vão para
**Outros**. Os desinstaladores, manuais e atalhos para páginas ficam de fora, e
um programa com o mesmo nome em dois sítios aparece uma vez (primeiro o do
Scoop).

### Numa pen USB

Cria um ficheiro vazio chamado `rlaunch.portable` ao lado do `rlaunch.exe`: as
definições passam a ficar ali, as pastas do mesmo disco são encontradas quando a
pen muda de letra, **Explorar** abre a pen e a barra mostra o seu espaço.
Desliga o Scoop e o menu Iniciar para ver só o que está na pen.

Sem esse ficheiro, as definições ficam em `%APPDATA%\rlaunch`.

## Linux

O menu lê as aplicações instaladas (ficheiros `.desktop`, com as suas próprias
categorias) e as tuas pastas. Não tem atalho de teclado próprio (associa um a
`rlaunch` nas definições do ambiente de trabalho) e ainda não mostra ícones. Sem
área de notificação, fechar o menu termina o programa.

## Fases

1. **Feito:** o menu na área de notificação com o atalho de teclado, Scoop, menu
   Iniciar e pastas próprias, categorias, procura, abrir programas.
2. Favoritos, ocultar, mudar o nome, a categoria e o ícone, acrescentar um
   programa solto, executar como administrador.
3. Temas e aspeto.
4. PortableApps (nomes e categorias de cada programa) e importação de outras
   suites portáteis.
5. Linux completo (ícones).

## Compilar

```powershell
./dev.ps1 build --profile release-small -p rlaunch
./dev.ps1 test -p rlaunch
```
