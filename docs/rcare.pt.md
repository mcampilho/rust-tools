# rcare — suite de manutenção do Windows (plano e passagem de testemunho)

Estado (2026-11-22): **as páginas da fase 0 estão feitas** — as páginas do rclean, do rdup, do rsize e do rinst estão
atrás de uma barra lateral recolhível, depois da **Custom Clean** do próprio rcare (fase 1 feita). Da fase 0 falta o serviço, a residência, as atualizações e o release (ver
"Próximos passos"). Só Windows, sem limite de tamanho, perfil `release` (não `release-small`).

O `rcare` junta numa só janela o que várias ferramentas pequenas fazem, para não ser preciso abrir várias com
administração. Modelos: BleachBit, CCleaner, Glary Utilities, IObit Advanced SystemCare e Uninstaller, Auslogics
BoostSpeed, Ashampoo WinOptimizer, AVG TuneUp, Avast Cleanup, Wise Care 365, Norton Utilities, Outbyte, Fortect,
System Mechanic, Wintoys, Kudu, as ferramentas Sysinternals. O utilizador pediu-o em português em 2026-11-22 (o nome,
`rcare`, é escolha dele) e quer Rust, uma barra lateral recolhível como a do Claude, o ecrã habitual de cada
ferramenta pequena à direita, e os menus repensados para juntar os das ferramentas. Tudo o que vem depois de "Windows
System Information" na lista abaixo fica de propósito para mais tarde.

## Ler primeiro (para quem continuar)

- Falar com o utilizador em português europeu (CLAUDE.md global). `python3`, não `python`. **Nunca fazer commit nem
  push sem aprovação explícita.** Verificar imagens e documentos à vista. As regras do projeto estão no `CLAUDE.md`
  (raiz) e no `apps/rcare/CLAUDE.md`: "pronto" = sem avisos, `clippy -D warnings` limpo, testes a passar; `cargo fmt
  --all` antes de um commit, com `E:\Portable\scoop\apps\rust\current\bin` à frente no `PATH`; compilar com
  `CARGO_INCREMENTAL=0`; todo o texto visível no `strings.txt` (10 línguas, separado por tabs, teclas de acesso
  diferentes dentro de cada menu em todas as línguas); `README.md` e `README.pt.md` atualizados em conjunto;
  `./dev.ps1 …` carrega o ambiente do MSVC.
- Commits deste trabalho no `master`: `dedf452` (esqueleto do rcare, página do rclean), `a31480b` (páginas do rdup e do
  rsize, `Dialogs`, `tr_from`), `2f8c234` (página do rinst), e depois a Custom Clean. **Desde 2026-11-22 o utilizador
  deixa fazer commits sem perguntar** neste plano; os pushes, e o que fugir ao plano ou for decisão dele, continuam a
  precisar dele.
- Este documento tem o que é preciso para não ter de reler o código todo.

## O que existe

| Onde | O quê |
| --- | --- |
| `apps/rcare` | a suite: `src/{lib,main,shell,commands,pages,sidebar,settings,storage,appicon}.rs`, `src/i18n/`, `tests/screens.rs`; só Windows (o `main` diz isso fora dele; a biblioteca compila em todo o lado) |
| `apps/rclean/src/page.rs` | `CleanPage` (+ `Event`, `Sender`, `shortcuts_text`) |
| `apps/rdup/src/page.rs` | `DupPage` (+ `Event`, `Sender`, `Popup`, `Out`, `shortcuts_text`) |
| `apps/rsize/src/page.rs` | `SizePage` (+ `Event`, `Sender`, `Popup`, `shortcuts_text`); é dona das suas `Settings` |
| `apps/rinst/src/page/` | `InstPage` (`mod.rs`: estado, leitura, comandos, desenho; `input.rs`: teclas, caixa de pesquisa, rato; `uninstalling.rs`: a fila, o relatório, os restos) (+ `Event`, `JobMsg`, `Sender`, `Popup`, `shortcuts_text`) |
| `crates/rt-ui/src/page.rs` | `Effects`, `Dialogs` (+ `SystemDialogs`), `Mods` |
| `crates/rt-ui/src/theme.rs` | `Theme`: as cores que as quatro apps e o rcare partilham (o `ui::theme` de cada app reexporta-o) |
| `crates/rt-core/src/i18n.rs` | `add` (tabelas extra), `tr_in_from`/`tr_from`/`tf_from` (uma tabela procurada primeiro) |
| `crates/rt-core/src/menu.rs` | `M::map` (converter os comandos de um menu noutro tipo) |
| `apps/rcare/src/maint.rs`, `disks.rs` | System Maintenance: `MaintPage`, `Check`, `View`, `disks_text`; estado dos discos pelo `Get-Volume` |
| `apps/rcare/src/startup.rs`, `startup_page.rs` | Startup Optimizer: o que arranca com o Windows (chaves `Run` do utilizador e da máquina, 64/32 bits, as duas pastas Arranque) e desligar/ligar itens por `Explorer\StartupApproved`, como o Gestor de Tarefas |
| `apps/rcare/src/custom.rs` | Custom Clean: `CustomPage` (disposição, clique, desenho, acessibilidade), `Part`, `Status`, `View`, `large_files` |
| `docs/rcare(.pt).md` | este plano |

As apps pequenas (rclean, rdup, rsize, rinst) foram religadas para usarem as mesmas páginas: continuam a funcionar
como antes e uma correção faz-se uma vez.

## O padrão de página (o contrato que toda a página segue)

Uma página é dona do seu estado, dos seus fios de trabalho, do seu desenho num retângulo da janela, do rato e do
teclado. **Não** é dona da janela. Ler `apps/rclean/src/page.rs` (a mais simples), depois `apps/rdup/src/page.rs`
(diálogos, menus que saltam, caixas de texto) e `apps/rsize/src/page.rs` (definições, horas de despertar, caches do
treemap).

- `XPage::new(…, send: Sender)`; `Sender = Arc<dyn Fn(Event) + Send + Sync>`: os fios mandam aí os `Event` e a janela
  transforma-os no seu `UserEvent` (rcare: `UserEvent::Clean/Dups/Size/Inst`). `page.event(e)` trata um.
- `page.fx: rt_ui::page::Effects` — o que a página pede à janela: `redraw`, `menu_dirty` (mudou o que os menus mostram
  como ativo/marcado), `info` (uma mensagem), `save` (mudaram definições). A janela chama `after_page()` depois de
  cada chamada à página: `let fx = page.fx.take(); …`. As páginas chamam `self.fx.redraw()`, `.menus()`, `.info(t, x)`,
  `.save()`.
- Perguntar ao utilizador (síncrono): `rt_ui::page::Dialogs` (`confirm`, `pick_folder`, `save_file(nome, (tipo,
  padrão), pasta)`, `recycle`), passado a `page.command(cmd, &mut dialogs)`; a janela faz `SystemDialogs { hwnd }`. As
  quatro páginas usam-no.
- `page.command(cmd, …) -> bool`: faz os comandos que são da página e devolve true; false para os da janela (Sair,
  tema, língua, ajuda, menu do Explorador, arranque, reiniciar como administrador).
- `page.enabled(cmd)`, `page.checked(cmd)` para menus e botões.
- `page.render(gfx, área, tema, escala, menu_bar)` só desenha dentro de `área` (`view::draw` preenche `l.full`, já não
  limpa a tela toda).
- Entrada: `key(…) -> Option<Command>` (o comando de um atalho, ou a página usou a tecla), `pointer(x, y, blocked)`,
  `pointer_left()`, `press(…) -> Option<Command | Out>` (um botão pediu um comando, ou um menu que salta),
  `release()`, `drag…`, `wheel(delta, at)`, `right_click(…)`. A posição do rato e os modificadores vêm como argumentos
  (`Mods { ctrl, shift, alt }`); a página guarda a última escala vinda do `render`.
- Menus que saltam (`Popup`): a página só diz qual; a janela constrói-o e mostra-o
  (`rt_sys::win::menu::popup_at_cursor`) e corre o comando escolhido.
- Despertar: `animate()` — rclean e rinst `bool` (um arco roda), rdup `Option<Duration>`, rsize `Option<Instant>`; a
  janela põe `ControlFlow::WaitUntil`.
- Leitores de ecrã: `accessibility(&mut nodes, &mut children) -> Option<foco>` (as linhas são `NodeId(100 + índice)`;
  1 e 3 são a lista e a linha de estado) e `accessibility_click(i)`. A janela acrescenta os seus nós (diálogo 4–6,
  barra lateral do rcare 10–99).
- Definições: rclean/rdup/rinst guardam-nas nas `Settings` da janela e a página copia para dentro/fora
  (`store_into`); a página do rsize **é dona** das suas `Settings` (tema, língua e tamanho da janela também lá estão). O
  rcare guarda as do rdup, do rsize e do rinst em ficheiros próprios, `rdup.ini`, `rsize.ini` e `rinst.ini`, ao lado do
  `settings.ini` (as marcas do rclean estão nas definições do próprio rcare).
- O `i18n::add_table()` de cada app e o seu `tr`/`tf` procuram **primeiro a tabela da própria app**
  (`rt_core::i18n::tr_from`). É preciso porque as apps traduzem a mesma chave inglesa com teclas de acesso diferentes
  (cada uma afinada para os seus menus); o `Cmd::label` do rcare entrega ao `label()` da página pela mesma razão.
  Qualquer página nova tem de fazer o mesmo.

## System Maintenance (fase 2, página do próprio rcare, a primeira da barra lateral)

As verificações que existem, de uma vez ("Verificar", F5), uma linha cada com o que encontrou e um botão: **Junk Files**,
**Broken Shortcuts** e **Invalid Registry Entries** (a análise do limpador, separada por `Group`: o lixo conta os alvos
marcados, atalhos e registo tudo o que foi encontrado), **Software Health** (programas de todas as listas do
desinstalador com um `Problem`: desinstalador desaparecido, instalação falhada, manifesto removido), **Drive Health** (o
`Get-Volume` do próprio Windows num PowerShell escondido, sem administrador: `HealthStatus` e `OperationalStatus`; os
Detalhes mostram cada disco e como reparar um com `chkdsk X: /f` ou Propriedades do disco › Ferramentas › Verificar —
neste PC o disco S: diz "Warning / Full Repair Needed"). "Rever" abre o limpador ou o desinstalador; nesta página não se
muda nada. Mais verificações entram com as fases seguintes (limpeza de privacidade na fase 5 — apagar histórico ou
cookies é decisão do utilizador, porque o rclean nunca o faz —, arranque, integridade do Windows na fase 6).

## Startup Optimizer (fase 3, página do próprio rcare, a última da barra lateral)

A lista que o separador Arranque do Gestor de Tarefas mostra: `HKCU\…\Run`, `HKLM\…\Run` (64 e 32 bits), as pastas
Arranque do utilizador e de todos (atalhos lidos com `rt_sys::lnk`). Uma linha cada com um interruptor, de onde arranca e
o que corre (a laranja quando o programa não existe; "Precisa de direitos de administrador" nos itens de todos quando o
rcare não está elevado — não se podem mudar e a página diz porquê). Desligar escreve os 12 bytes que o Gestor de Tarefas
escreve em `Explorer\StartupApproved\{Run,Run32,StartupFolder}` (03 + FILETIME; ligar: 02): nada é apagado e o separador
do próprio Windows concorda; verificado a sério com um item próprio (`startup::tests::toggles_an_item_of_its_own`,
ignorado: escreve no `Run` do utilizador e depois tira). Botões e menu Editar: Desligar, Ligar, Abrir localização,
Atualizar (F5); Espaço liga ou desliga o item selecionado.

## Custom Clean (página do próprio rcare, a segunda da barra lateral)

Uma vista de conjunto, não um limpador: três cartões (Junk Files, Duplicate Files, Large Files) com o espaço e a
contagem, e "Rever", que abre a página que mostra essa parte por inteiro, onde se limpa. Nesta página não se apaga
nada. "Analisar" (F5, Ficheiro › Analisar) põe o trabalho **nas outras páginas**: o limpador analisa (o sistema todo),
o rdup procura na pasta (`DupPage::open_paths`: as raízes escolhidas da página de duplicados passam a ser essa pasta, e
valem os filtros dela), o rsize lê-a (`SizePage::start_scan`). A pasta é a do utilizador (`USERPROFILE`), salvo se
"Mudar de pasta…" escolheu outra (`custom_folder=` no `settings.ini`). Ficheiros grandes são os de 100 MB ou mais
(`custom::LARGE`), contados na árvore do rsize uma vez por árvore (`large_cache`). O shell lê o estado das páginas para
o `View` antes de desenhar (`custom_status`); o que as páginas comunicam redesenha-a. Cartões lado a lado quando há
largura, uns por baixo dos outros quando é estreita e alta, colunas estreitas com o botão em baixo numa janela pequena
(imagens `custom-*.png`).

## Como o rcare funciona

- `pages.rs`: `PAGES` (id guardado nas definições, `Kind`, chave do rótulo, ícone). Acrescentar uma página = um `Kind`,
  uma entrada, um ícone em `sidebar::draw_glyph`, um campo e um braço no `shell.rs` para cada operação.
- `commands.rs`: `Cmd::{Clean(c), Dups(c), Size(c), Inst(c), CustomAnalyze, CustomFolder, Sidebar, RestartAdmin, Exit, Theme, UiLanguage,
  Shortcuts, CheckUpdates, About}`; `from_clean/from_dups/from_size/from_inst` convertem em comandos da janela os comandos da página que
  são dela. **Os menus saem do `menu_tree()` de cada app pequena** (`own_menus`): o que é da janela (Sair, reiniciar,
  menu do Explorador, arranque, tema, língua, ajuda) cai, os itens de Ficheiro da app vão primeiro no Ficheiro do rcare
  (depois Reiniciar como administrador e Sair), os de Ver primeiro no Ver (depois Recolher barra lateral, Tema,
  Língua), os outros menus (Editar, Marcar, Programa…) ficam entre eles, e a Ajuda é da janela. Uma página nova ganha portanto
  menus sem esforço; se duas etiquetas colidirem numa língua, o teste `access_keys_differ_within_each_menu` diz qual
  (corrigir a string na tabela do rcare ou da app). Com quatro páginas o menu Ver está cheio: "Collapse Side&bar"
  precisou de uma letra livre em todas (o alemão diz "Seitenleiste: nur S&ymbole", o espanhol "Contra&er barra lateral").
- `sidebar.rs`: disposição/clique/desenho sem janela (testável); 224 px aberta, 52 px recolhida (os ícones não se
  mexem); estado no `settings.ini` (`sidebar=`, `page=`).
- `shell.rs` (só Windows): a macro `page!(self, p => …)` corre uma expressão na página mostrada; as operações cujas
  assinaturas diferem por página (`key`, `press`, `command`, `enabled`, `render`, `wheel`…) são `match` explícitos de
  três braços. `take_fx` junta os efeitos de todas as páginas (uma página que não se vê ainda pode avisar: uma
  pesquisa acaba). `ensure_started(i)` faz o primeiro trabalho de uma página da primeira vez que se mostra (limpador:
  primeira análise; desinstalador: ler os programas; as outras: ler os discos). Agora são quatro campos e `match` de
  quatro braços: com uma quinta página, trocá-los por um trait pequeno ou um `enum` de páginas.
- Ficheiros de dados: a pasta do rcare (`%APPDATA%\rcare`, ou portátil) — `settings.ini`, `rdup.ini`, `rsize.ini`,
  `rinst.ini`, `hashes.bin` (cache de hashes dos duplicados: separada da do rdup), `backups\registry …` (as cópias
  `.reg` do limpador), `backups\leftovers ….reg` (as do desinstalador).

## Verificado, e não

Verificado (2026-11-22, Windows 10): `cargo fmt --check`, `clippy --workspace --all-targets -D warnings`, `test
--workspace` (os dois testes de pipe do `rt-svc` falharam uma vez com carga e passam sozinhos: corrida que já
existia, não mexi); as imagens das quatro páginas, claro/escuro, barra aberta/recolhida (`tests/screens.rs`, vistas);
um teste de arranque do `rcare.exe` a sério (compilação de depuração, modo portátil em `target\debug`): a janela abriu,
ficou 8 s, fechou com a mensagem de fecho e código 0, e escreveu os quatro ficheiros de definições (apagados depois).

**Não verificado**: usar a janela a sério (rato, teclado, menus nativos, menus que saltam, diálogos, mudar de página,
desinstalar), correr as apps pequenas em Linux depois de religadas (compilação, clippy e testes em Linux passaram no CI
em 2026-11-22, execução 45064520969, depois do push de `bf7e572`; os menus desenhados e os diálogos delas não foram
corridos), a ligação do rcare ao serviço `rtsvc` (as páginas usam-no como as apps pequenas), leitores de
ecrã.

## Próximos passos, por ordem

1. ~~Página do rinst~~ feita (2026-11-22): "Uninstall Apps" na barra lateral.
2. ~~Custom Clean~~ feita (2026-11-22): ver a secção acima.
3. ~~Unificar as cópias de `Theme` e o `Confirm` do rclean~~ feito (2026-11-22): `rt_ui::theme::Theme`, `Dialogs` em
   todas. Falta o utilizador experimentar a janela a sério (só foi aberta e fechada).
4. ~~System Maintenance (fase 2)~~ feita (2026-11-22): ver a secção.
5. ~~Residência e integração~~ feitas (2026-11-22): `--tray` (arranca escondido, o ícone ou outro arranque mostra-o,
   fechar esconde; experimentado com o `.exe` a sério), uma só instância (`hand_off`, `--new-window`), Ficheiro ›
   Integração com o Windows › Menu do Explorador (**um** verbo nas pastas e discos, "Analisar com o rcare": a pasta abre
   na Custom Clean, analisada, só dessa vez — `custom_once`; `--register`/`--unregister`) e Iniciar com o Windows;
   `rcare [pasta]` faz o mesmo que o verbo. O `rcare-v*` está no `release.yml`. O serviço: as páginas já o usam como as
   apps pequenas (fotografias do `rtsvc` no limpador e no desinstalador, `scan_with` nos discos); as tarefas dele
   (`Clean`, `UninstallMsi`) só aceitam clientes elevados, por isso um rcare não elevado não tem mais nada a pedir-lhe.
   Falta: "Procurar atualizações…" funciona quando houver release; o primeiro release (etiqueta `rcare-v0.1.0`, um
   push: **perguntar ao utilizador**) e depois o manifesto do bucket do Scoop (`bucket/rcare.json`, depois
   `checkver.ps1 -Dir bucket -Update`).
6. ~~Startup Optimizer~~ feito (2026-11-22): ver a secção.
7. O resto da fase 3, como o utilizador decidiu em 2026-11-22 (abaixo), depois as fases 4–6.

## Decisões para o resto da fase 3 (o utilizador escolheu a opção recomendada de cada, 2026-11-22)

- **App/Extension Cleaner**: remover extensões dos browsers obriga a mexer no perfil de cada um (`Preferences` do
  Chrome/Edge, `extensions.json` do Firefox) com o browser fechado — ou só listá-las e abrir a página de extensões do
  próprio browser. Recomendado: listá-las (nome, browser, perfil, ativa) e abrir a página do browser. **Escolhido.**
- **Game Mode / "programas parados, RAM libertada"**: parar programas termina processos do utilizador (pode perder-se
  trabalho não guardado). Recomendado: o interruptor do Modo de Jogo do próprio Windows
  (`HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled`) e uma lista do que corre, fechando só o que o utilizador
  marcar, com delicadeza (`WM_CLOSE`), nunca à força. **Escolhido.**
- **Smart RAM**: recomendado como está em "Limites honestos": mostrar a memória e libertar só a lista de espera a pedido
  (precisa de administrador: `NtSetSystemInformation` com `MemoryPurgeStandbyList`). **Escolhido.**
- **Program Deactivator**: além dos itens de arranque, também serviços em arranque automático? Mexer em serviços precisa
  de administrador e pode estragar o Windows: recomendado só serviços de terceiros, com cópia do valor para desfazer.
  **Escolhido.**

## Decisões

1. **Fundir não é copiar.** Os motores já estavam em crates (`rt-clean`, `rt-inst`, `rt-dups`, `rt-scan`). Os ecrãs
   saíram do `main.rs` de cada app para páginas que a app pequena e o `rcare` usam. O rclean, o rinst, o rdup e o
   rsize ficam como estão (portáteis, pequenos); uma correção faz-se uma vez.
2. **Ferramentas nativas do Windows primeiro** (Defender, Firewall, DISM, `sfc /scannow`, `netsh`, `powercfg`,
   `pnputil`, Windows Update): nada de motor próprio de antivírus ou firewall. Uma página de Reparação oferece `sfc
   /scannow`, `DISM /Online /Cleanup-Image /RestoreHealth` e `chkdsk` (o utilizador pediu-os).
3. **Administração** pelo serviço `rtsvc` que já existe (nível de utilizador por omissão, elevado nas ações que
   precisam), ou reiniciando o rcare elevado (o que as apps pequenas fazem).
4. O utilizador aprovou: o nome `rcare`; as apps pequenas ficam; Defender/Firewall em vez de motor próprio.

## Limites honestos

- Antivírus, spyware, malware, firewall: mostrar o estado do Defender e da Firewall, lançar análises e atualizar
  definições, corrigir o que está desligado. Sem analisador próprio.
- Registry Defrag: o Windows não compacta hives em uso. Só análise, por agora.
- "Limpar" a RAM: esvaziar working sets costuma piorar. Mostrar a memória como está e libertar só o que é seguro.
- "Otimizar" o TCP: o Windows ajusta-se sozinho. Mostrar os valores, perfis seguros e "repor predefinições".
- Update Drivers e Software Updater usam a rede: vão pelas APIs do próprio Windows e precisam de uma exceção aprovada
  à regra "sem rede" (perguntar ao utilizador).
- Reparar a Cortana: fora (já não existe).

## A lista do utilizador (até System Information), como pedida

Windows System Maintenance: Privacy Sweep, System Optimization, Antivirus Protection, Spyware Removal, Junk File
Clean, Internet Boost, Firewall, Security Reinforce, Shortcut Fix, Registry Defrag, Hardware Health, Vulnerability
Fix, Registry Clean, Disk Optimization, Software Health, Disk Check. **Custom Clean**: Junk Files, Duplicate Files,
Large Files. **Speed Up System**: Game Mode (programas parados, RAM libertada), Startup Optimizer, App/Extension
Cleaner, Real-Time TuneUp (AutoCare, Auto RAM Clean, Performance Monitor). **Protect System**: System Protection
(malware, firewall, anti-spyware), Privacy Protection (pasta segura, proteção de acesso, impressão digital,
anti-tracking), Browser Protection (email, remoção de anúncios, navegação, aconselhador de página inicial), Security
& Repair (DNS protect, undelete). **Smart RAM**: Smart Clean, Deep Clean. **Internet Booster**: TCP Optimize (max
duplicate ACKs, optimum route, TCP 1323 options, MTU discovery, black hole router detection, TTL 64), Browser
Optimize. **Registry Cleaner**: ActiveX and Component, Registry Integrity, Application Paths, Fonts, Help Files,
History Files, Shared Files, Shortcut, Software, Sounds, Start Menu, Startup Items, MUI Cache, Virtual Device, File
Extensions, System Extensions, Windows Services, Firewall Settings, Deep Scan. **Registry Defrag**: reduzir
fragmentos, reparar erros estruturais, recuperar memória não usada. **Other options**: Fast Startup, Repair Start Menu,
Update Drivers, Set Default Program, Uninstall Apps. **Program Deactivator**: programas ativos que se podem
desativar. **System Information**.

Mais tarde (decisão do utilizador): Auto Shutdown, Fix problems (geral, avançado, browser), Context Menu Manager,
Disk Doctor, System Control, DNS Protector, Disk Cleaner (= rclean: temporários, cópias, caches, registos, outros),
File Shredder, Empty Folder Scanner, Shortcut Fixer, Duplicate File Finder (= rdup), Large File Finder (= rsize),
Process Manager, Software Updater, Default Program.

## Fases

**0. Fundações** — páginas do rclean ✔, rdup ✔, rsize ✔, rinst ✔; rcare com barra lateral e menus ✔;
`rtsvc`, residência, atualizações, release (ver "Próximos passos").

**1. Páginas existentes na barra lateral** — Disk Cleaner (rclean) ✔, Duplicate Files (rdup) ✔, Large Files (rsize)
✔, Uninstall Apps (rinst) ✔, e **Custom Clean** ✔ (lixo, duplicados e grandes numa só análise).

**2. Manutenção num clique** ✔ (System Maintenance, com as verificações que já existem) — corre em sequência os módulos disponíveis (privacidade, lixo, registo, atalhos,
verificação de disco, saúde do software) e faz um relatório.

**3. Velocidade** — Startup Optimizer ✔; App/Extension Cleaner e Program Deactivator; Game Mode e Real-Time TuneUp
(monitor de desempenho); Smart RAM.

**4. Registo** — Registry Cleaner (as categorias acima, Deep Scan, cópia `.reg` antes de cada limpeza, restauro);
Registry Defrag (análise).

**5. Rede e proteção** — Internet Booster (TCP, DNS Protector, otimizar browsers); Protect System (Defender,
Firewall, anti-tracking, proteção do browser, pasta segura, undelete a avaliar).

**6. Sistema** — Fast Startup, reparar o Menu Iniciar, Update Drivers, Set Default Program, System Information, saúde
do hardware e do software, e uma página de Reparação (SFC, DISM, chkdsk).

## Regras

- Toda a ação destrutiva mostra o que vai fazer, pergunta e guarda uma cópia para desfazer (registo, serviços,
  arranque).
- Os módulos testam-se com dados falsos; as ações reais só numa máquina elevada (o serviço nunca correu como serviço
  a sério: `install`/`uninstall` só numa máquina elevada).
- Textos visíveis no `strings.txt`; README.md e README.pt.md atualizados em conjunto.
