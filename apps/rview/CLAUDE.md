# rview — visualizador de documentos portátil em Rust

Visualizador para Windows (~4,4 MB, um só `.exe`) e Linux de PDF, Markdown, HTML, texto
e CSV, com zoom, seleção e cópia para o Clipboard (com formatos ou só texto).
Sem rede (só se o utilizador ligar Ver › Mostrar imagens da web: ver abaixo), sem telemetria, nunca executa scripts.
Integração opcional com o Explorador (`--register`, `--associate`, `--unregister`).
Detalhes por formato, atalhos e limitações estão no `README.md`.

## Comandos

```powershell
./dev.ps1 build --release -p rview
./dev.ps1 test -p rview       # 105 testes; os do código partilhado estão nos crates
./dev.ps1 clippy -p rview --all-targets
```

Correm na raiz do repositório. Saída: `target\release\rview.exe`. Rust 1.88+, alvo `x86_64-pc-windows-msvc`.
`build.rs` (com `rt-build`) embebe ícone, versão e manifesto.

## Arquitetura (`src/`)

- `doc/` — texto (virtualizado), CSV (deteção de delimitador, grelha virtualizada) e
  `pdf.rs`: texto dos PDF com posições (lopdf + interpretador próprio do conteúdo das páginas).
- `clip.rs` — o que vai para o Clipboard: um só fluxo de eventos escrito como texto, HTML e RTF.
- `view/selection.rs` — seleção por formato (`Sel`), teste de posição e construção do `Payload`.
- `rich/` — Markdown (pulldown-cmark), parser HTML próprio, subconjunto de CSS, motor de layout comum.
- `syntax/` — cache de estados por linha para o realce; linguagens, lexer e cores vêm de `rt-syntax`.
- `media.rs` — PDF via `Windows.Data.Pdf` e imagens via codecs do Windows; em Linux, hayro e image
  (threads em fundo, cache de 256 MB). Os tamanhos das páginas são em DIPs nos dois sistemas.
- `view.rs`, `main.rs` — vista, janela, input, modo `--tray`. O `impl Shell` está repartido por `input.rs`, `menus.rs` e `render.rs`; a base da janela, os menus desenhados e o painel de mensagem vêm do `rt-ui`.
- `win/` — Win32 de `rt-sys` (menus escuros, diálogos, tray) com a identidade do rview, mais `clipboard.rs`
  (o `Payload` em vários formatos) e `assoc.rs` (os tipos de ficheiro do rview para `rt_sys::win::assoc`, que
  escreve as associações só em HKCU e tem os testes). "Definir como visualizador predefinido" abre o painel
  `rt_ui::assoc::AssocPanel` (2026-11-14, pedido do utilizador): tipos a marcar, quem abre cada um (e se é
  escolha do utilizador, `UserChoice`, que nunca se muda), "Abrir com…" (`SHOpenWithDialog`), Aplicar
  (`set_defaults`); `Shell::assoc`, `modal()`. O `--associate` continua a fazer todos.
- Uma janela mostra um só documento (2026-11-24, pedido do utilizador): a instância residente que recebe ficheiros
  (`Shell::open_incoming`) abre o primeiro na própria janela só se estiver escondida ou vazia; se já mostra um
  documento, cada ficheiro abre noutra janela (`rview --new-window <ficheiro>`), e antes dá o direito de
  primeiro plano (`allow_any_foreground`). Antes só abria o primeiro e substituía o que estava aberto.
- `rich/extras.rs` (2026-11-24, pedido do utilizador): o que o pulldown-cmark não tem e o markdown-it tem: emojis
  (`:wink:`, emoticons; a cores pelo `rt-gfx`, camadas COLR do `seguiemj.ttf`), `^sup^`, `~sub~` (o parser não os lê
  dentro de palavras), `++ins++`, `==mark==`, abreviaturas (`*[X]: …`, tiradas da fonte antes de ler) e contentores
  `::: aviso` (a fonte parte-se neles).
- `rich/math.rs` (2026-11-24, pedido do utilizador): fórmulas `$…$` e `$$…$$` (`Options::ENABLE_MATH`, eventos
  `InlineMath`/`DisplayMath`; a de bloco é um parágrafo centrado) como texto estilizado, sem motor de composição: gregos
  e símbolos em Unicode (o `rt-gfx` cai no `seguisym.ttf`), variáveis em itálico, `^`/`_` com `valign` (aninhados ficam
  todos ao mesmo nível), `\text`, `\mathbb`/`\mathbf`, acentos por caracteres combinantes, `matrix`/`cases`/`aligned`
  em linhas. `convert_delimiters` passa `\(…\)` e `\[…\]` a `$…$` e `$$…$$` antes de ler (fora do código). Três
  `Inline` de fórmulas são caixas desenhadas por `rich/layout/mathbox.rs` (filho do `layout`, vê o `Ctx`): `Frac`
  (`\frac`, `\over`), `Root` (`\sqrt`: sinal escalado à altura do conteúdo e barra por cima) e `Limits` (`\sum`, `\prod`,
  `\lim`, `\max`…: limites por cima e por baixo, só em `$$…$$`; `\int` fica com eles ao lado, como no TeX; dentro de
  frações e raízes também ao lado). Cada caixa é medida uma vez (`Row` = textos e caixas na mesma linha de base) e
  desenhada com itens de texto e filetes, por isso a seleção e a cópia funcionam; frações e raízes aninham. Dentro de
  expoentes e índices (já pequenos) a fração é `a/b` e a raiz `√(…)`. Os números (0,78 de altura, 0,22 de descida, eixo
  a 0,3) são estimativas dos tipos de letra: verificados à vista, não medidos. `Inline::Matrix` (`matrix`, `cases`,
  `aligned`…) é uma grelha (`Boxed::Matrix`: colunas, linhas, parênteses retos por filetes e `( ) { }` por glifos
  escalados à altura). `convert_delimiters` também junta cada bloco `$$…$$` de várias linhas numa só linha: o
  parser de Markdown vê a estrutura de blocos primeiro e uma linha `=` ou `-` da fórmula seria um título ou lista.
- `<details>` (2026-11-24, pedido do utilizador): `Kind::Details` (sumário, corpo, `open`); a ligação `rview:details:N`
  da linha do sumário (N = o seu número de ligação, que também é a identidade do bloco) é tratada em `Viewer::follow`:
  alterna `RichDoc::flipped`, invalida o layout e mantém o scroll (`keep_scroll`). No Markdown o `<details>` abre e
  fecha em blocos HTML separados à volta de Markdown: `Builder::flush_html` parte o HTML nas etiquetas e usa
  `Frame::Details`; em HTML (`html.rs`) é o ramo `"details"` do conversor.
- `rich/mermaid.rs` + `rich/layout/diagram.rs` (2026-11-24, pedido do utilizador): só fluxogramas (`graph`/`flowchart`,
  direções TD/TB/BT/LR/RL; formas `[] () ([]) (()) {} {{}} [[]] [()]`, setas `--> --- -.-> ==>` com texto `|x|` ou
  `-- x -->`, cadeias e `&`; `subgraph`, `classDef`, `style`… são ignorados). Outros tipos, mais de 150 nós ou 400
  arestas ficam como código. O desenho: camadas pelo caminho mais longo (ciclos cortados nas arestas de retorno), ordem
  por baricentro, posições puxadas para os vizinhos, linhas retas entre os contornos, tudo em `Item::Shape` (polígono
  cheio ou contorno) e texto (seleção e cópia funcionam); se for mais largo que a coluna refaz-se mais pequeno.
- Duas opções do menu Ver, ambas desligadas por omissão e guardadas em `settings.ini` (2026-11-24, pedido do
  utilizador): `remote_images` e `expand_date`. **Imagens da web** (exceção à regra de rede aprovada nesse dia):
  `Viewer::request_images` só pede as chaves `http(s)` se `Viewer::remote_images`, e só as que `view::fetchable`
  deixa (não vai a `localhost`, nomes sem ponto, `.local`/`.internal`, IP de loopback, privado, de ligação local ou
  CGNAT, nem a endereços com `user@`; um nome que resolva para um IP privado não se apanha), no máximo 48 por
  documento; `Media::fetch_images` descarrega em até 3 threads com `rt_sys::net::get` (GET, 24 MB, sem cookies,
  agente `rview`) e entrega os bytes à fila do trabalhador como uma imagem local; falha → `Done::Image` sem bitmap.
  Ligar ou desligar chama `remote_images_changed` (esquece as chaves remotas e refaz o layout). **`$(date)`**:
  `rich/date.rs`, aplicado por `markdown::parse` ao texto (fora de código) se `date::enabled()` (um `AtomicBool`
  posto pelo menu e no arranque); só `date`, `+FORMATO` (`%Y %y %m %d %e %H %I %M %S %p %j %A %a %B %b %F %T %R %D`,
  nomes em en/pt/es pela língua da interface) e `-I`; sem argumentos entendidos o texto fica como está; nada é
  executado. Mudar a opção volta a ler o documento Markdown aberto.
- O `rt-gfx` não desenha nem mede selectores de variação e juntadores (`U+FE0F`, `U+200D`…): `⚠️` já não leva um
  quadrado.
- `linux/` — Clipboard (arboard: texto com HTML, ou imagem), e o resto de `rt-sys`: diálogo Abrir (zenity ou
  kdialog), tema, links, e o modo residente com socket Unix e ícone StatusNotifierItem.
- `ui/menu.rs` — menus desenhados na janela (`rt-gfx`), só fora do Windows. Em Linux, `cfg(not(windows))`
  significa Linux.
- `i18n/strings.txt` — traduções (10 idiomas), mesma convenção do rpad/rcalc.

## Regras

- HTML é um subconjunto de documento, não um browser: sem JS, sem folhas de estilo remotas; imagens remotas só com a
  opção Mostrar imagens da web ligada (desligada por omissão).
- Links `http`/`https`/`mailto` abrem no programa por omissão; links nunca podem lançar programas.
- Integração com o Explorador escreve só em HKCU, não sobrepõe defaults escolhidos pelo
  utilizador e `--unregister` repõe o estado anterior; os testes usam uma chave de registo temporária.
- Qualquer string visível nova precisa de linha completa em `strings.txt`.
- Dentro de cada menu, as teclas de acesso não se repetem em nenhuma língua (`rt_core::menu::access_key_clashes`,
  com teste).
- Copiar usa sempre as cores originais do documento, nunca as adaptadas ao tema escuro.
- A leitura de texto dos PDF corre numa thread própria com `catch_unwind`: um ficheiro que faça
  a lopdf entrar em pânico só fica sem texto selecionável. Por isso o rview usa sempre o perfil
  `release`, nunca o `release-small`, que aborta em pânicos.
- Em Linux, esconder a janela destrói-a e mostrar cria-a de novo, porque o Wayland não esconde janelas.
- O hayro e o image correm dentro de `catch_unwind`: um ficheiro malformado dá erro, não fecha o programa.
- O teclado e o rato reais são precisos para testar arrastos: o winit confirma a posição real do
  cursor e ignora `WM_MOUSEMOVE` simulado.
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
