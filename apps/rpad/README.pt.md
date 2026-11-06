# rpad

Um editor de texto e código para Windows, portátil e sem rede, escrito em Rust.
O `rpad.exe` é um único executável de 4 MB, sem instalador, sem runtime e sem DLL
além das que vêm com o Windows. Não faz ligações de rede e não tem telemetria.

O rpad inspira-se nos editores de texto clássicos do Windows (separadores,
codificações e fins de linha, procura e substituição, ferramentas de texto,
ajudas para XML), mas o código, o ícone e os gráficos são próprios.

[English version](README.md)

O rpad faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo correm
na raiz do repositório.

## Compilar

Requisitos: Rust 1.88+ com o alvo MSVC (`x86_64-pc-windows-msvc`) e as MSVC Build
Tools (linker e `rc.exe`).

```powershell
# Com as Build Tools no PATH (uma "Developer PowerShell"):
cargo build --profile release-small -p rpad

# Ou com Build Tools portáteis: o dev.ps1 carrega primeiro o ambiente de
# $env:RUST_TOOLS_MSVC_ENV, ou de \Portable\Tools\dev\msvc nos discos E: ou D:.
./dev.ps1 build --profile release-small -p rpad
./dev.ps1 test -p rpad
./dev.ps1 clippy -p rpad --all-targets
```

O resultado é `target\release-small\rpad.exe`. O perfil `release-small` (ver o
`Cargo.toml` da raiz) aborta em pânicos, o que torna o executável cerca de 20% mais
pequeno; a recuperação após falha não depende disso. A biblioteca C é ligada
estaticamente (`.cargo/config.toml`), e o ícone, a informação de versão e o
manifesto da aplicação são embebidos pelo `build.rs`. O manifesto ativa os Common
Controls 6, a consciência de DPI por monitor v2 e os caminhos longos.

## Executar

```
rpad [--tray] [--new-window] [ficheiros...]
```

- Os ficheiros indicados na linha de comandos abrem em separadores. Um ficheiro
  que ainda não exista abre como separador vazio e só é criado ao guardar.
- Por omissão, uma segunda chamada ao `rpad` passa os ficheiros à janela que já
  está aberta (Ferramentas › Preferências › Abrir ficheiros na janela existente).
  `--new-window` arranca uma janela separada.
- Arrasta ficheiros para a janela para os abrir.

### Modo residente (`--tray`)

`rpad --tray` arranca o rpad escondido na área de notificação, para acesso
imediato:

- Clicar no ícone, ou voltar a arrancar o `rpad` (com ou sem ficheiros), mostra a
  janela de imediato; os ficheiros abrem nela.
- Fechar a janela (ou o último separador, quando está configurado para fechar a
  janela) apenas a esconde. Os documentos abertos, mesmo os não guardados, ficam
  como estão, e os dados de recuperação e as definições são escritos ao esconder.
- Para sair, usa o menu do ícone (botão direito › Sair) ou Ficheiro › Sair. Aplicam-se
  as regras habituais: com a recuperação ligada, o texto não guardado fica para o
  próximo arranque; com a recuperação desligada, o rpad pergunta o que fazer aos
  documentos não guardados.
- Escondido, o rpad usa cerca de 20-30 MB e nenhum CPU.

Para o arrancar com o Windows, coloca um atalho para `rpad.exe --tray` na pasta
Arranque (`shell:startup`); o rpad não se acrescenta sozinho.

### Idioma da interface

A interface existe em alemão (de-DE), inglês (en-GB, en-US), espanhol (es-ES),
francês (fr-FR), italiano (it-IT), japonês (ja-JP), português (pt-BR, pt-PT) e
chinês simplificado (zh-CN). Por omissão segue o idioma de apresentação do Windows
(os restantes idiomas usam inglês); Ver › Idioma da interface muda-o de imediato,
incluindo os menus.

### Modo portátil e dados

As definições, a sessão, os ficheiros de recuperação após falha, o histórico local
e os excertos ficam em `%APPDATA%\rpad`. Para manter tudo ao lado do executável
(numa pen USB, por exemplo), cria um ficheiro vazio chamado `rpad.portable` ao lado
do `rpad.exe`. Não são precisos direitos de administrador nem escritas no registo.
O registo só é lido, para a lista de tipos de letra, o tema e a cor de destaque.

Se o `settings.ini` estiver danificado, os valores inválidos voltam aos valores por
omissão. Se a pasta de dados não puder ser escrita, o rpad funciona na mesma e
mostra um aviso.

## Funcionalidades

**Documentos**
- Separadores com marca de alteração, clique do meio para fechar e menu de contexto;
  a faixa de separadores desloca-se quando há muitos.
- Novo, Abrir (seleção múltipla), Guardar, Guardar como, Guardar tudo, Fechar,
  Fechar os outros, Fechar tudo, Reabrir separador fechado (Ctrl+Shift+T),
  Recarregar, Abrir recente (com Limpar), Copiar caminho, Mostrar no Explorador.
- Fechar o último separador pode manter um documento vazio ou fechar a janela
  (Ferramentas › Preferências).
- Pergunta sobre alterações não guardadas (Guardar / Não guardar / Cancelar) ao
  fechar um separador, e ao sair com a recuperação desligada.
- **Recuperação após falha / saída rápida** (ligada por omissão, pode ser desligada):
  o texto não guardado é escrito em ficheiros de recuperação 2 segundos depois de
  parar de editar e ao perder o foco, e restaurado no arranque seguinte. A
  recuperação nunca toca nos ficheiros originais.
- Restauro da sessão (separadores abertos, cursor, posição de deslocamento).
- **Histórico local** (desligado por omissão): antes de cada gravação é guardada uma
  cópia da versão anterior; Ficheiro › Histórico local abre as versões antigas só
  para leitura.
- Gravação automática opcional quando a janela perde o foco (desligada por omissão).
- Gravação atómica: um ficheiro temporário e `ReplaceFileW`, com escrita direta
  quando a pasta não permite ficheiros temporários.
- Deteta ficheiros alterados ou apagados por outros programas (quando a janela ganha
  o foco), oferece Recarregar / Manter a minha versão, e pergunta antes de sobrepor
  um ficheiro mais recente no disco.
- Os ficheiros só de leitura abrem só para leitura; os ficheiros com conteúdo binário
  (bytes NUL) abrem só para visualização, para não poderem ser danificados. Guardar
  como funciona em ambos.

**Codificações e fins de linha**
- UTF-8, UTF-8 com BOM, UTF-16 LE/BE (com ou sem BOM) e mais 22 páginas de código
  antigas (Windows-125x, ISO-8859-x, KOI8, IBM866, Shift_JIS, EUC-JP, GBK, Big5,
  EUC-KR).
- Deteção: BOM → heurísticas de UTF-16 → UTF-8 válido → página de código do sistema.
  É possível reabrir com outra codificação a qualquer momento.
- O texto nunca é corrompido em silêncio. Se houver bytes que não se conseguem
  descodificar, o documento avisa e pergunta antes de guardar caracteres de
  substituição. Guardar numa codificação que não consegue representar um carácter é
  recusado, com a linha e a coluna do primeiro desses caracteres, e o ficheiro fica
  inalterado.
- CRLF / LF / CR são detetados e preservados; os fins de linha mistos são
  assinalados. A conversão faz-se no menu Formatar ou na barra de estado.

**Edição**
- Anular/refazer com a escrita agrupada em palavras; cada ferramenta ou substituição
  é um só passo de anulação.
- Vários cursores: Ctrl+clique, seleção em coluna com Alt+arrastar, Ctrl+Alt+Cima/Baixo,
  acrescentar a ocorrência seguinte (Ctrl+Alt+D), selecionar todas as ocorrências
  (Ctrl+Shift+L), dividir a seleção em linhas.
- Duplicar, apagar, mover e juntar linhas; aumentar/diminuir indentação; alternar
  comentário; indentação automática atenta aos parênteses; Home inteligente; modo de
  sobreposição.
- Tabulações ou espaços, largura de tabulação 2/4/8 e conversão da indentação nos
  dois sentidos.
- Números de linha, realce da linha atual, marcas de espaços e de fim de linha,
  caracteres de controlo desenhados como símbolos, translineação, régua de linhas
  longas, zoom (Ctrl+roda), escolha do tipo de letra, ecrã inteiro.
- Os caracteres CJK largos e os emoji ocupam duas colunas; as marcas combinatórias
  ligam-se ao carácter anterior.

**Procura**
- Barra de procura e substituição: maiúsculas/minúsculas, palavra inteira, expressão
  regular, na seleção, recomeçar do início, contagem em tempo real ("3 de 12"),
  resultados realçados com marcas na barra de deslocamento e histórico de procuras e
  substituições.
- Substituir tudo mostra uma pré-visualização (contagem, linhas, primeiras alterações)
  e pede confirmação. As substituições com regex aceitam `$1` / `${name}` e `\n`, `\t`.
- Procura nos documentos abertos e em ficheiros (pasta, filtro como `*.rs;*.txt`; ignora
  ficheiros binários, ficheiros acima de 32 MB e pastas como `.git` / `target` /
  `node_modules`; corre em segundo plano e pode ser interrompida). Os resultados
  aparecem num painel; Enter ou um clique salta para lá.
- Ir para a linha (`linha` ou `linha:coluna`, com pré-visualização ao escrever), ir
  para o parêntese correspondente (Ctrl+B), ir para o símbolo e um painel de estrutura.
- As expressões regulares usam a biblioteca `regex` (tempo linear, sem retrocesso
  catastrófico), com limites de tamanho.

**Sintaxe**
- Realce para Rust, C, C++, C#, Java, JavaScript, TypeScript, Go, Python, HTML, CSS,
  JSON, XML, Markdown, SQL, Shell, PowerShell, Batch, INI, TOML, YAML, Properties e
  Diff. A linguagem é detetada pela extensão e pela primeira linha (shebang, `<?xml`),
  e pode ser mudada na barra de estado. Os ficheiros desconhecidos são texto simples.
- Dobragem de código (chavetas, etiquetas, indentação, títulos de Markdown) e realce do
  parêntese correspondente.

**Ferramentas de texto** (menu Ferramentas; seleção ou documento inteiro; tudo anulável)
- Maiúsculas e minúsculas: MAIÚSCULAS, minúsculas, Título, Frase, iNVERTER.
- Linhas: ordenar de forma ascendente/descendente/sem distinção de maiúsculas/natural,
  inverter, remover duplicados, remover linhas vazias, cortar espaços no fim/início,
  acrescentar prefixo/sufixo.
- Espaços: tabulações → espaços, espaços iniciais → tabulações.
- Codificar/descodificar: cadeia JSON, entidades XML, codificação percentual de URL,
  Base64.
- As alterações a muitas linhas do documento inteiro perguntam primeiro.

**Markdown e XML**
- Painel de pré-visualização de Markdown (desligado por omissão, Ctrl+Shift+M),
  desenhado sem rede. As ligações são mostradas mas nunca abertas; as imagens aparecem
  como o seu texto alternativo e nada é descarregado.
- XML: verificação de boa formação, com a linha e a coluna do primeiro erro; Formatar
  XML reindenta o conteúdo só com elementos e mantém o texto, CDATA, comentários e
  `xml:space="preserve"` exatamente como estão. Recusa tocar em documentos mal
  formados.

**Outros**
- Paleta de comandos (Ctrl+Shift+P) com procura aproximada em todos os comandos.
- Excertos: inserir a partir de uma lista, guardar a seleção como excerto ou editar o
  `snippets.txt`.
- Tema claro, escuro ou do sistema (segue o Windows), cor de destaque do sistema e DPI
  por monitor. No tema escuro, a barra de menus, os menus, os menus de contexto e o
  menu da área de notificação também ficam escuros.
- Dez idiomas de interface (ver acima), que se podem trocar durante a execução.
- Menus nativos com atalhos e teclas de acesso, dicas na barra de ferramentas e na
  barra de estado, e uma barra de estado com posição, seleção, número de linhas, modo
  de inserção/sobreposição, zoom, indentação, codificação, fim de linha e linguagem.
  Clicar num elemento da barra de estado muda-o.
- Leitores de ecrã: a janela expõe os controlos (separadores, botões, campos, listas e
  a linha atual do editor) à UI Automation através do AccessKit.

## Atalhos de teclado

| Ação | Atalho |
| --- | --- |
| Novo / Abrir / Guardar / Guardar como | Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S |
| Guardar tudo | Ctrl+Alt+S |
| Fechar separador | Ctrl+W, Ctrl+F4 |
| Reabrir separador fechado | Ctrl+Shift+T |
| Separador seguinte / anterior | Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+PgDn / Ctrl+PgUp |
| Anular / Refazer | Ctrl+Z / Ctrl+Y, Ctrl+Shift+Z |
| Cortar / Copiar / Colar | Ctrl+X / Ctrl+C / Ctrl+V (Shift+Del, Ctrl+Ins, Shift+Ins) |
| Selecionar tudo | Ctrl+A |
| Duplicar linha ou seleção | Ctrl+D |
| Apagar linha | Ctrl+Shift+K |
| Mover linha para cima / baixo | Alt+Up / Alt+Down (Ctrl+Shift+Up / Down) |
| Juntar linhas | Ctrl+J |
| Aumentar / diminuir indentação | Tab / Shift+Tab, Ctrl+] / Ctrl+[ |
| Alternar comentário | Ctrl+/ |
| Cursor acima / abaixo | Ctrl+Alt+Up / Ctrl+Alt+Down |
| Acrescentar ocorrência seguinte / selecionar todas | Ctrl+Alt+D / Ctrl+Shift+L |
| Dividir a seleção em linhas | Alt+Shift+I |
| Seleção em coluna | Alt+arrastar |
| Acrescentar cursor | Ctrl+clique |
| Inserir data/hora | F5 |
| Inserir excerto | Ctrl+Shift+J |
| Modo de sobreposição | Insert |
| Procurar / Substituir | Ctrl+F / Ctrl+H |
| Procurar seguinte / anterior | F3 / Shift+F3 (Enter / Shift+Enter na caixa de procura) |
| Opções de procura (na barra) | Alt+C maiúsculas, Alt+W palavra, Alt+R regex, Alt+L na seleção |
| Substituir tudo (na barra) | Alt+A, Ctrl+Alt+Enter |
| Procurar em ficheiros | Ctrl+Shift+F |
| Ir para linha / parêntese / símbolo | Ctrl+G / Ctrl+B / Ctrl+Shift+O |
| Translineação | Alt+Z |
| Dobrar / desdobrar atual | Ctrl+Shift+[ / Ctrl+Shift+] |
| Dobrar tudo / desdobrar tudo | Alt+0 / Alt+Shift+0 |
| Aumentar / reduzir / repor zoom | Ctrl+= / Ctrl+- / Ctrl+0, Ctrl+roda |
| Pré-visualização de Markdown | Ctrl+Shift+M |
| Ecrã inteiro | F11 |
| Paleta de comandos | Ctrl+Shift+P |
| MAIÚSCULAS / minúsculas | Ctrl+Shift+U / Ctrl+U |
| Lista de atalhos de teclado | F1 |

## Arquitetura

| Módulo | Conteúdo |
| --- | --- |
| `doc` | Buffer rope (`ropey`), seleção múltipla, anular/refazer transacional, operações de edição |
| `io` | Deteção, descodificação e codificação estrita; leitura de ficheiros e gravação atómica |
| `syntax` | Definições de linguagens, lexer incremental por linha, dobragem, estrutura |
| `search` | Planeamento de procura/substituição e procura em ficheiros |
| `tools` | Transformações de texto e verificador/formatador de XML |
| `app` | Estado da aplicação, comandos, documentos, sessão, recuperação, definições, input |
| `ui` | Renderer de CPU (tiny-skia + ab_glyph com os tipos de letra do sistema), widgets, vista do editor, pré-visualização de Markdown |
| `i18n` | Idiomas da interface: a tabela de traduções (`strings.txt`), procura com alternativas |
| `win` | Integração Win32: menus (com modo escuro), diálogos comuns, Clipboard, ícone na área de notificação, passagem para a instância única |

A lógica da aplicação fala com o sistema através de um pequeno trait `Host` (diálogos e
Clipboard). Isto permite que os testes de fluxo conduzam a aplicação inteira sem janela.
O `unsafe` limita-se a chamadas Win32: o módulo `win` e chamadas isoladas a `ReplaceFileW`
(gravação atómica), `GetACP` (página de código do sistema), hora local e posição do
cursor. Cada uma tem um comentário `SAFETY`.

As traduções estão em `src/i18n/strings.txt`: uma linha por texto, com o texto em inglês
dos EUA como chave e uma coluna por idioma, separadas por tabulações. Os textos dos menus
levam a sua própria tecla de acesso `&`; o japonês e o chinês usam a convenção do Windows
`ファイル(&F)`. As células en-GB ou pt-PT vazias usam en-US ou pt-BR.

Os menus escuros usam as mesmas técnicas não documentadas de outros editores Win32: as
funções de modo preferido da aplicação do uxtheme para os menus pendentes, e as mensagens
`WM_UAHDRAWMENU`/`WM_UAHDRAWMENUITEM` para desenhar a barra de menus
(`src/win/darkmode.rs`). As funções são procuradas durante a execução, por isso, nas
versões do Windows que não as têm, os menus ficam simplesmente claros.

## Testes

`cargo test -p rpad` corre 115 testes. Cobrem o buffer e as operações de edição, as
codificações (ida e volta, deteção, caracteres não representáveis), os fins de linha, o
lexer, a procura, as ferramentas de texto, XML, definições, ficheiros de sessão e de
recuperação, layout e fluxos completos através de um host simulado:

- ida e volta exata ao byte para UTF-8, UTF-8 com BOM, UTF-16 LE/BE, CR/LF/CRLF e
  Windows-1252;
- recusa de guardar caracteres não representáveis, deixando o ficheiro intacto;
- perguntas ao fechar (Cancelar / Guardar com diálogo cancelado / Guardar / Não guardar)
  e fecho do último separador;
- deteção de alterações e remoções externas, e confirmação antes de sobrepor um ficheiro
  mais recente;
- substituir tudo com pré-visualização e uma só anulação, grupos de regex, regex inválida;
- ida e volta da recuperação após falha, recuperação desligada, restauro da sessão e
  histórico local;
- ferramentas de texto e respetiva anulação, operações de linhas, escrita com vários
  cursores, Clipboard e paleta de comandos;
- abertura de um ficheiro com mais de 16 MB em modo de ficheiro grande;
- traduções: todas as linhas têm todos os idiomas, os marcadores `{}`, as quebras de
  linha e exatamente uma tecla de acesso nos menus; todos os textos que o código traduz
  (`tr`, `tf`, avisos, nomes de ferramentas, comandos, menus e paleta) têm linha; e a
  janela inteira é desenhada em alemão com todos os painéis abertos, registando qualquer
  procura sem tradução.

## Verificado à mão (Windows 10, escala 100%)

Versão de distribuição: sem avisos, clippy limpo, 115/115 testes a passar. O
`dumpbin /dependents` mostra apenas DLL do sistema (sem runtime VC).

Na aplicação a correr:
- separadores, realce de Rust e Markdown, marcas de dobragem, barra de estado;
- a barra de procura e substituição com contagem em tempo real e marcas na barra de
  deslocamento;
- Alt+A → diálogo de pré-visualização de Substituir tudo → substituído e marcado como
  alterado;
- a paleta de comandos, a pré-visualização de Markdown, o painel de estrutura, a barra de
  ferramentas, os temas claro e escuro;
- a recuperação depois de o processo ter sido terminado à força (edições não guardadas
  restauradas);
- a passagem para a instância única;
- um registo de 106 MB / 1 000 001 linhas a abrir em cerca de 0,5 s com cerca de 145 MB de
  RAM, com Ctrl+End e Ir para linha instantâneos e uma contagem em segundo plano de
  1 000 000 de resultados;
- mudança de idioma durante a execução (alemão) e arranque em japonês: barra de menus,
  menus, barra de procura, barra de estado, separadores e avisos traduzidos, e texto CJK
  desenhado na janela e nos menus;
- barra de menus escura e menus Ficheiro/Editar/Formatar, menu de contexto e menu da área
  de notificação escuros no tema escuro;
- `--tray`: arranca escondido (cerca de 20 MB), um segundo arranque mostra a janela,
  Alt+F4 esconde-a mantendo as edições não guardadas, arrancar com um ficheiro mostra-a e
  abre o ficheiro, e menu do ícone › Sair termina (saída rápida com recuperação).

Não exercitado à mão (o código existe; parte tem testes unitários): composição com um
método de introdução asiático real, utilização com leitores de ecrã, os idiomas além do
alemão e do japonês no ecrã (partilham o mesmo código e as verificações da tabela),
reiniciar o Explorador em modo residente (o ícone volta a ser acrescentado com
`TaskbarCreated`), monitores de alta densidade e mudanças de DPI, arrastar a partir do
Explorador, procura em ficheiros na interface (o motor de procura tem testes unitários),
diálogos de excertos e de histórico local na interface, e tipos de letra além do
predefinido.

## Limitações

- Só Windows. O código é maioritariamente portátil, mas não há compilação testada para
  outros sistemas.
- Os ficheiros acima de 256 MB são recusados. Os ficheiros acima de 16 MB abrem em modo de
  ficheiro grande: sem realce de sintaxe, dobragem, estrutura nem translineação.
- O texto é desenhado numa grelha de células monoespaçadas, sem modelação de texto
  complexo: as escritas da direita para a esquerda (árabe, hebraico) e as que precisam de
  modelação (devanágari, tailandês) aparecem como glifos separados pela ordem lógica, os
  emoji são monocromáticos e não há ligaduras.
- O realce usa um lexer por linha para cada linguagem, não um parser completo, por isso
  algumas construções raras podem ficar com a cor errada. A dobragem baseia-se em
  parênteses, etiquetas, indentação ou títulos.
- O realce de resultados no editor tem um limite para contagens muito grandes; a contagem
  em si é exata.
- As expressões regulares funcionam no texto do documento, mas não suportam look-around nem
  referências para trás (uma limitação do motor de tempo linear).
- O histórico de anulação não se mantém entre arranques, embora o texto não guardado se
  mantenha.
- Sem impressão, verificação ortográfica, vistas divididas, macros ou plugins.
- As caixas de mensagem e os diálogos Abrir/Guardar/Tipo de letra são desenhados pelo
  Windows; ficam claros no tema escuro, e os botões seguem o idioma do Windows, não o do
  rpad.
- Os nomes das teclas dos atalhos (Ctrl, Shift…) aparecem em inglês nos menus, em todos os
  idiomas. A ajuda da linha de comandos (`--help`) está em inglês.
- As traduções foram escritas para este projeto; as correções de falantes nativos são
  bem-vindas em `src/i18n/strings.txt`.

## Licença

MIT. Ver [LICENSE](../../LICENSE).
