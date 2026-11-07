# rview

Um visualizador de documentos pequeno, rápido e portátil para Windows e
[Linux](#linux), escrito em Rust. Mostra ficheiros **PDF, Markdown, HTML, texto e CSV**, com
zoom, e permite selecionar e copiar o que se vê.

[English version](README.md)

O rview faz parte do [rust-tools](../../README.pt.md). Os comandos abaixo correm
na raiz do repositório.

O `rview.exe` é um único executável de 4,4 MB: sem instalador, sem runtime,
sem DLL além das que vêm com o Windows e sem escrever no registo, a não ser
que peça a integração com o Explorador. Não faz ligações de rede, não tem
telemetria e nunca executa scripts.

## Linux

O pacote para Linux é um `tar.gz` com o binário `rview`. Funciona em X11 e Wayland e
precisa da glibc 2.34 ou mais recente (Ubuntu 22.04, Debian 12, Fedora 35). Compilá-lo
precisa do Rust 1.92 ou mais recente.

- **Desenho.** As páginas PDF são desenhadas pelo
  [hayro](https://github.com/LaurenzV/hayro) e as imagens são descodificadas pela
  biblioteca `image`, ambas dentro do binário, por isso não é preciso mais nada, mas
  ele tem 16,6 MB, quase quatro vezes o de Windows. O hayro é recente (versão 0.9): PDF
  complexos podem sair pior do que em Windows. Os tipos de letra que um PDF não embebe
  usam os tipos de letra padrão do PDF, por isso esse texto em chinês, japonês ou
  coreano não aparece. Os PDF com palavra-passe não abrem.
- **Bibliotecas.** Carrega as bibliotecas gráficas ao arrancar, e qualquer ambiente de
  trabalho as tem: `libwayland-client` e `libxkbcommon` em Wayland, `libX11`,
  `libxcursor`, `libxkbcommon` e `libxkbcommon-x11` em X11. Se em X11 parar com um
  erro sobre `libxkbcommon-x11.so`, instala o pacote `libxkbcommon-x11-0`.
- **Diálogo Abrir.** Usa o `zenity` ou o `kdialog`, conforme o que estiver instalado.
  Sem eles, a barra de estado avisa, e os ficheiros continuam a abrir pela linha de
  comandos. Enquanto o diálogo está aberto a janela não responde, e alguns ambientes
  podem propor fechá-la.
- **Menus.** São desenhados na janela, com os mesmos itens e atalhos. Abrem com
  Alt+letra ou F10. Não há a integração com o Windows.
- **Clipboard.** Copiar coloca texto e HTML, ou uma imagem com Alt+arrastar num PDF.
  Não há RTF, e a imagem vai sem o texto. O que é copiado fica no rview até um gestor
  de Clipboard o guardar, por isso pode perder-se ao fechar o rview num ambiente sem
  esse gestor.
- **Tipos de letra.** Usa DejaVu, Noto ou Liberation, em `/usr/share/fonts`,
  `/usr/local/share/fonts`, `~/.local/share/fonts` ou `~/.fonts`. O japonês e o chinês
  precisam de Noto Sans CJK, WenQuanYi Zen Hei ou Droid Sans Fallback.
- **Área de notificação.** O `--tray` usa StatusNotifierItem, como no KDE, Xfce,
  Cinnamon e outros. O GNOME precisa da extensão AppIndicator. Sem anfitrião para o
  ícone, o `--tray` é ignorado e o rview abre como uma janela normal.
- **Tema e idioma.** "Sistema" lê as definições do GNOME ao arrancar e não acompanha
  alterações posteriores. O idioma vem de `LC_ALL`, `LC_MESSAGES` ou `LANG`. As
  definições ficam em `~/.config/rview`.
- **Diferenças.** Largar um ficheiro na janela funciona em X11, não em Wayland. As
  ligações e *Mostrar na pasta* usam `xdg-open`, só para ligações web e de correio.

Verificado no WSL2 em X11: páginas PDF, incluindo com tipos de letra LaTeX, selecionar
e copiar o texto, Alt+arrastar, Markdown com uma imagem PNG, menus e menu de contexto,
cópia com e sem formatos, um PDF danificado e modo residente. O `zenity` foi substituído
por um script de teste, por isso o diálogo real não foi visto, e o `kdialog` não foi
executado. O ícone foi testado com um anfitrião simulado. Em Wayland só foi arrancado,
não usado. JPEG e os outros formatos de imagem, deslocar PDF longos e seguir ligações
não foram experimentados.

## Como desenha

| Formato | Motor |
| --- | --- |
| Texto (`.txt`, `.log`, outros ficheiros) | rview: descodificado uma vez, desenhado de forma virtualizada (só as linhas visíveis) |
| CSV (`.csv`, `.tsv`) | rview: deteção do delimitador, campos entre aspas, grelha virtualizada |
| Markdown (`.md`) | rview: pulldown-cmark → layout de documentos do rview |
| HTML (`.html`, `.htm`) | rview: parser de HTML próprio e subconjunto de CSS → o mesmo layout |
| PDF (`.pdf`) | Motor de PDF do Windows (`Windows.Data.Pdf`) |
| Imagens em Markdown/HTML | Codecs de imagem do Windows (PNG, JPEG, GIF, BMP, TIFF, ICO, WebP/HEIF se instalados) |

O motor de PDF e os codecs de imagem fazem parte do Windows 10/11, por isso
não acrescentam nada ao executável. Tudo o resto é desenhado pelo renderizador
próprio do rview, no CPU (tiny-skia + ab_glyph com os tipos de letra do
sistema). O motor de PDF do Windows só desenha páginas, por isso o texto dos
PDF é lido à parte, numa thread em segundo plano, com a biblioteca lopdf.

## Compilar

Requisitos: Rust 1.88+ com o alvo MSVC e as MSVC Build Tools.

```powershell
cargo build --release -p rview
# ou, com Build Tools portáteis (carrega $env:RUST_TOOLS_MSVC_ENV, ou procura em
# \Portable\Tools\dev\msvc nos discos E: e D:):
./dev.ps1 build --release -p rview
./dev.ps1 test -p rview
./dev.ps1 clippy -p rview --all-targets
```

O resultado é `target\release\rview.exe`. Usa o perfil `release`: ao contrário
do `release-small`, mantém o desenrolar de pânicos, para que um PDF malformado
não feche o programa. A biblioteca C é ligada
estaticamente; o ícone, a informação de versão e o manifesto (DPI por monitor
v2, caminhos longos) são embebidos pelo `build.rs`.

## Utilização

```
rview [--tray] [--new-window] [ficheiro]
rview --register | --associate | --unregister
```

Abra ficheiros com Ctrl+O, largando-os na janela ou pela linha de comandos.

**Modo residente:** `rview --tray` arranca escondido na área de notificação.
Lançar o rview outra vez (com ou sem ficheiro) ou clicar no ícone mostra a
janela de imediato e abre lá o ficheiro; fechar a janela (ou Ctrl+W) apenas a
esconde. Saia pelo menu do ícone ou em Ficheiro › Sair. `--new-window` abre
uma janela separada mesmo com o rview residente. Para arrancar com o Windows,
ponha um atalho para `rview.exe --tray` em `shell:startup`. As definições
(tema, idioma, translineação, tamanho da janela, última pasta) ficam em
`%APPDATA%\rview\settings.ini`, ou junto do `rview.exe` se existir aí um
ficheiro chamado `rview.portable`.

**Integração com o Explorador** (Ficheiro › Integração com o Windows, ou a
linha de comandos). Tudo é escrito só para o utilizador atual (sem direitos de
administrador) e aponta para o `rview.exe` a partir do qual é executado:

- *Menu de contexto do Explorador* / `--register`: acrescenta "Abrir com o
  rview" ao menu do botão direito dos ficheiros `.pdf`, `.md`, `.markdown`,
  `.html`, `.htm`, `.txt`, `.log`, `.csv` e `.tsv`, e o rview à lista "Abrir
  com". Desmarcar (ou `--unregister`) remove tudo o que o rview acrescentou,
  incluindo predefinições, e repõe as anteriores.
- *Definir como visualizador predefinido* / `--associate`: regista também o
  rview nas Aplicações predefinidas do Windows e torna-o a predefinição dos
  tipos em que não escolheu nenhuma. O Windows protege as predefinições que
  escolheu (por exemplo um leitor de PDF ou um navegador); o rview não as
  altera, mas abre Definições › Aplicações predefinidas e indica esses tipos
  para poder escolher lá o rview.

Se mover o `rview.exe`, o item do menu aparece desmarcado; marque-o outra vez
para apontar as entradas para o novo local. Numa consola, as opções da linha
de comandos escrevem o resultado; caso contrário, é mostrado numa caixa de
mensagem. O código de saída é 0 em caso de sucesso.

| Ação | Teclas |
| --- | --- |
| Abrir | Ctrl+O |
| Recarregar | F5 |
| Fechar (esconder em modo residente) | Ctrl+W |
| Selecionar | Arrastar com o rato (ver abaixo) |
| Selecionar tudo | Ctrl+A |
| Copiar com formatos | Ctrl+C |
| Copiar como texto simples | Ctrl+Shift+C |
| Limpar a seleção | Esc |
| Ampliar / reduzir | Ctrl+= / Ctrl+-, Ctrl+roda do rato (amplia no ponteiro) |
| Tamanho real (100 %) | Ctrl+0 |
| Ajustar à largura (PDF) | Ctrl+9 |
| Translineação (texto) | Alt+Z |
| Ecrã inteiro | F11 (Esc sai) |
| Deslocar | Setas, Page Up/Down, Espaço / Shift+Espaço, Home / End, roda do rato (Shift para os lados) |
| Página anterior / seguinte do PDF | Ctrl+Page Up / Ctrl+Page Down |
| Mover o documento | Arrastar com o botão do meio, ou Ctrl+arrastar |
| Atalhos de teclado | F1 |

## Selecionar e copiar

Arraste para selecionar, faça duplo clique para uma palavra, triplo clique
para uma linha ou parágrafo, Shift+clique para estender, e arraste para lá do
limite da janela para continuar a deslocar. Ctrl+A seleciona tudo. O menu
Editar e o menu do botão direito têm os mesmos comandos. Um clique numa
ligação abre-a; arrastar a partir dela seleciona.

**Copiar** (Ctrl+C) põe a seleção no Clipboard como texto, HTML e RTF, para
que o Word, o Outlook, o Excel, os navegadores e o WordPad mantenham os
formatos: tipos de letra, tamanhos, negrito, itálico, cores, ligações, títulos,
listas, citações, tabelas e código colorido. **Copiar como texto simples**
(Ctrl+Shift+C) põe apenas texto. As cópias usam as cores originais do
documento, mesmo no tema escuro.

- **Texto e código:** quaisquer caracteres; o código mantém as cores de sintaxe.
- **Markdown e HTML:** quaisquer caracteres, através de parágrafos, listas e
  tabelas. Uma imagem selecionada vai como ligação no HTML; uma imagem
  selecionada sozinha vai também como imagem.
- **CSV:** células, em retângulo; clique no número da linha para a linha, no
  título da coluna para a coluna, ou no canto para tudo. O texto simples é
  separado por tabulações (com as aspas das folhas de cálculo), por isso é
  colado em células.
- **PDF:** texto, entre páginas, com tipos de letra, tamanhos e cores; nas
  cópias com formatos, as linhas de um parágrafo são unidas. Alt+arrastar
  seleciona antes um retângulo da página, copiado como imagem tal como se vê
  no ecrã (e como texto, se o tiver). Páginas sem texto, como digitalizações,
  selecionam sempre um retângulo; Ctrl+A num documento sem nenhum texto
  seleciona a página atual.

Seleções com mais de 8 MB de texto (ou 200 000 células de CSV) são copiadas só
como texto simples, e a barra de estado indica-o.

## Detalhes por formato

- **Realce de sintaxe:** ficheiros de código abertos como texto (Rust, C, C++,
  C#, Java, JavaScript, TypeScript, Go, Python, HTML, CSS, JSON, XML, SQL,
  shell, PowerShell, batch, INI, TOML, YAML, properties, diff; detetados pelo
  nome do ficheiro ou por uma linha `#!`) e blocos de código em Markdown
  (` ```rust `) e HTML (`<pre><code class="language-…">`). Ficheiros acima de
  16 MB são mostrados sem realce.
- **Texto:** deteção da codificação (UTF-8, UTF-8 com BOM, UTF-16 LE/BE com ou
  sem BOM, a página de código do sistema), CRLF/LF/CR, números de linha,
  translineação para ficheiros até 16 MB (os maiores deslocam na horizontal),
  caracteres CJK largos, caracteres de controlo mostrados como símbolos.
- **CSV:** vírgula, ponto e vírgula, tabulação ou barra vertical, detetados nas
  primeiras linhas; campos entre aspas com `""` e quebras de linha; linha de
  títulos e números de linha fixos; números alinhados à direita; larguras das
  colunas calculadas por amostragem de todo o ficheiro.
- **Markdown:** CommonMark com tabelas GitHub, listas de tarefas, texto riscado
  e notas de rodapé; os títulos têm âncoras ao estilo do GitHub, por isso as
  ligações `#` funcionam; o HTML embebido é desenhado pelo motor de HTML. Segue
  o tema claro/escuro.
- **HTML:** os documentos são mostrados numa página branca, como num navegador.
  Suportado: títulos, parágrafos, listas (aninhadas, numeradas com `start`),
  tabelas (com `colspan`), `pre`/`code`, citações, ligações e âncoras, imagens
  locais e `data:`, blocos `<style>`, ficheiros `<link rel=stylesheet>` locais,
  `style=""`, `bgcolor`/`align`/`<font>`/`<center>` antigos. CSS: cores, fundos,
  peso/estilo/tamanho/família de letra (monoespaçada), alinhamento e decoração
  de texto, `display: none`, `white-space: pre`; seletores por etiqueta, classe,
  id, atributo, descendente e filho, com especificidade.
- **PDF:** as páginas são desenhadas numa thread em segundo plano para as
  páginas visíveis (mais a seguinte e a anterior) ao zoom exato; guardam-se até
  256 MB de páginas desenhadas e as mais distantes são descartadas primeiro.
  Ajustar à largura é a predefinição. O texto é lido de tipos de letra
  embebidos, Type 1, TrueType, compostos e Type 3, em páginas rodadas e dentro
  de form XObjects; mantém a ordem do próprio ficheiro, que é a ordem de
  leitura na maioria dos documentos.
- **Ligações:** as `#âncoras` deslocam; ligações para documentos locais dos
  tipos suportados abrem no rview; ligações `http`, `https` e `mailto` abrem no
  navegador ou programa de correio predefinido ao clicar. As ligações nunca
  podem lançar programas. O destino aparece na barra de estado ao passar por
  cima.

**Modo escuro:** no tema escuro, Markdown, texto e CSV usam cores escuras, e as
páginas HTML e PDF também aparecem escuras: as cores do autor do HTML são
adaptadas (fundos claros tornam-se escuros, texto escuro torna-se claro, os
matizes mantêm-se) e as páginas PDF têm a luminosidade invertida. As páginas
PDF já escuras (fotografias de página inteira, diapositivos escuros) mantêm as
cores; as fotografias dentro de páginas claras são invertidas com a página. Ver
› Páginas escuras no tema escuro desliga isto: todos os documentos (Markdown,
texto, CSV, HTML, PDF) passam a aparecer numa página clara, como impressos,
mantendo-se escuros a janela, os menus e a barra de estado. Os painéis Acerca
de e Atalhos de teclado são desenhados pelo rview no tema atual.

Também: tema claro/escuro/do sistema (incluindo menus escuros) e a interface em
alemão, inglês (Reino Unido/EUA), espanhol, francês, italiano, japonês,
português (Brasil/Portugal) e chinês simplificado, seguindo o Windows ou
escolhida em Ver › Idioma da interface.

## Testes e verificação

`cargo test -p rview` corre 95 testes em Windows e 97 em Linux: os parsers de texto, CSV, Markdown, HTML e CSS, o
motor de layout (quebra de linhas, zoom, palavras longas, ligações),
codificações, ligações e caminhos de imagens, definições, a tabela de traduções
(todas as frases do código e dos menus têm todos os idiomas; os marcadores e as
teclas de acesso coincidem), um desenho em alemão de cada tipo de documento que
falha com qualquer frase por traduzir (também no tema escuro), realce de
sintaxe (blocos de código, classes de código HTML, comentários de várias
linhas), adaptação de cores das páginas escuras (cores HTML e páginas PDF, que
mantêm as páginas escuras sem alterações), integração com o Explorador
(registar, associar e remover numa chave de registo temporária: a predefinição
do utilizador fica intacta, uma predefinição anterior é reposta, os verbos de
outros programas mantêm-se) e os motores do Windows: um PDF gerado no teste é
aberto e a página 1 desenhada através de `Windows.Data.Pdf` (píxeis do texto
verificados), um PDF inválido devolve um erro e um PNG é descodificado pelos
codecs do Windows. A seleção é testada por formato (caracteres, palavras,
linhas, texto com translineação, zoom a manter a seleção, linhas e colunas de
CSV, zonas de PDF), tal como o texto simples, o HTML, o RTF e a imagem escritos
para o Clipboard, e o texto de PDF (posições, rotação, crop box, form XObjects,
tipos de letra compostos, ligaduras, tipos de letra Type 3).

Medido no Windows 10 (compilação release):

- Registo de 101 MB (1 000 001 linhas): carregado em 0,37 s, cerca de 140 MB de
  memória; End salta de imediato para a última linha.
- CSV de 42 MB (1 000 000 linhas): carregado em 0,13 s, cerca de 80 MB de
  memória.
- PDF ilustrado de 12 páginas: primeira página mostrada ajustada à largura; o
  zoom a 300 % redesenha com nitidez; cerca de 90-125 MB de memória com páginas
  de alta resolução.
- Markdown (como este README) e um relatório HTML com CSS, tabelas, listas,
  entidades, uma fotografia local, elementos escondidos e um script que não
  pode correr: desenhados corretamente.
- Tema escuro: um relatório HTML com cores do autor e um PDF de texto aparecem
  escuros e legíveis; um PDF cuja capa é uma fotografia escura mantém-na como
  está. Com as páginas escuras desligadas, Markdown, CSV e PDF aparecem claros
  na janela escura; os painéis Atalhos e Acerca de são escuros e fecham com
  Esc / Enter.
- Realce: um ficheiro Rust e blocos de código Markdown (Rust, JSON).
- `--tray`: arranca escondido (cerca de 21 MB); um lançamento com ficheiro
  passa-o à janela residente, que o mostra; o botão de fechar esconde-a mantendo
  o documento; um lançamento sem ficheiro mostra-a; Ficheiro › Sair termina.
- Seleção e conteúdo copiado, em documentos de todos os tipos, nos temas claro
  e escuro: Markdown, HTML, CSV e código, um PDF com tipos de letra TrueType
  embebidos (incluindo uma página rodada e uma página só com imagem) e PDF do
  pdflatex (tipos de letra Type 1 e Type 3). Com o rato e o teclado reais:
  arrastar, duplo e triplo clique, menu do botão direito, Ctrl+C e
  Ctrl+Shift+C, arrastar para lá do limite, Alt+arrastar num PDF e mover com o
  botão do meio; o Clipboard do Windows foi lido após cada cópia, e o RTF
  carrega corretamente no RichEdit (o motor do WordPad).
- Compilação release: sem avisos, clippy limpo; `dumpbin /dependents` lista só
  DLL do sistema Windows.

Não exercitado à mão: clicar em ligações (as regras das ligações têm testes
unitários), colar no Word, Excel ou Outlook, leitores de ecrã, monitores de alta
densidade, PDF muito grandes ou protegidos por palavra-passe, documentos da
direita para a esquerda.

## Limitações

- Windows 10 ou posterior (PDF e imagens usam os motores do Windows).
- O HTML é um subconjunto de documento, não um navegador: sem JavaScript, sem
  floats, flexbox, grid ou posicionamento absoluto, sem tipos de letra web, sem
  consultas `@media` com larguras; imagens e folhas de estilo remotas nunca são
  obtidas. As páginas feitas para ler (relatórios, documentos exportados, artigos
  guardados) ficam bem; aplicações web complexas não.
- O texto é desenhado sem composição complexa: escritas da direita para a
  esquerda e escritas que precisam de composição (árabe, devanágari, tailandês)
  mostram glifos separados; os emojis são monocromáticos.
- PDF: sem pesquisa, índice ou formulários; ficheiros protegidos por
  palavra-passe não podem ser abertos. Texto sem informação Unicode no ficheiro
  (alguns tipos de letra antigos ou deliberadamente codificados) é copiado como
  `�`, e as páginas digitalizadas não têm texto (pode copiar-se uma imagem
  delas). A seleção segue a ordem do texto no ficheiro, que pode saltar em
  paginações invulgares.
- Sem pesquisa em nenhum formato e sem impressão (é um visualizador com zoom,
  de propósito).
- A seleção faz-se com o rato, mais Ctrl+A; não há cursor de teclado.
- Ficheiros acima de 512 MB (texto, CSV, Markdown, HTML) são recusados.

## Licença

MIT. Ver [LICENSE](../../LICENSE).
