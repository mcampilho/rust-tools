# rcare — manutenção do Windows numa só janela

Só Windows, sem limite de tamanho (perfil `release`, `platforms = "windows"`). Junta as páginas das ferramentas
pequenas (rclean, rdup, rsize, rinst, e mais tarde as novas) para não abrir várias com administração. O plano, as
decisões e o que fica de fora de propósito (antivírus próprio, defrag do registo, "limpar a RAM"…) estão em
`docs/rcare.pt.md` (e `.md`). Pedido do utilizador em 2026-11-22; nome escolhido por ele.

## Comandos

```powershell
./dev.ps1 build --release -p rcare
./dev.ps1 test -p rcare                                     # 98 testes (mais 1 no binário)
./dev.ps1 clippy -p rcare --all-targets
./dev.ps1 test -p rcare --test screens '--' --ignored       # apps/rcare/target/screens/*.png
./dev.ps1 test -p rcare --lib picture '--' --ignored        # o ícone
```

## Arquitetura

- **Páginas partilhadas.** O motor já estava em crates (`rt-clean`, `rt-inst`, `rt-dups`, `rt-scan`). O que faltava
  era a interface, presa ao `main.rs` de cada app. Agora cada app tem uma página na sua biblioteca (rclean:
  `rclean::page::CleanPage`: estado, fios de trabalho, desenho, teclado e rato) e o `main.rs` da app pequena e o
  `shell.rs` do rcare só a seguram. A página **não é dona da janela**: o que lhe pede (redesenhar, mostrar uma
  mensagem, reconstruir os menus) anota em `fx` (`rt_ui::page::Effects`) e a janela leva-o depois de cada chamada
  (`after_page`); perguntar ao utilizador é uma função que a janela passa (`Confirm`); o que os fios dizem volta como
  `Event` pelo `Sender` com que a página nasceu. Os comandos que são da janela (Sair, tema, língua, ajuda) a página
  devolve-os (`key` e `press` dão `Option<Command>`) e a janela decide (`Cmd::from_clean`).
- **Traduções.** Cada app tem a sua tabela e o `rt_core::i18n` só instala uma como "da app": o rcare instala a sua e
  junta as das páginas com `rt_core::i18n::add` (`rclean::i18n::add_table`), procuradas depois da dele e antes da
  comum. `i18n::install` é chamada por `set_lang`, `tr` e `tf`.
- **Menus.** `commands::menu_tree(página)`: Ficheiro e Ajuda são da janela, e entre eles vão os menus da página
  (`page_menus`, com `M::map` do `rt-core` a converter os comandos da página em `Cmd`); reconstruídos a cada mudança de
  página (`menu_rebuild`). Teclas de acesso verificadas por teste, por página.
- **Barra lateral** (`sidebar.rs`, desenhada com o `rt-gfx`): `layout`/`hit`/`draw` sem janela, por isso testável.
  Aberta 224 px, recolhida 52 px (o ícone não se mexe). Estado em `settings.ini` (`sidebar=`, `page=`). Cada página é
  uma entrada de `pages::PAGES` (id, chave de tradução, ícone).
- `shell.rs` (só Windows): janela, eventos, menus nativos, árvore para leitores de ecrã (botão da barra = 10,
  páginas = 11…, linhas do limpador = 100 + índice). `main.rs` só chama `shell::run()`; fora do Windows diz que é só
  para Windows (o crate compila no CI em Linux, onde só a biblioteca se testa).
- O tema é o `rt_ui::theme::Theme`, o mesmo das quatro apps (desde 2026-11-22; antes cada uma tinha uma cópia).

## Acrescentar uma página

1. Extrair a página da app (como o `rclean::page`): estado, trabalho, desenho e entrada para a biblioteca; `main.rs`
   fica com a janela. Mesmas regras: `fx`, `Confirm`, `Event`/`Sender`.
2. Uma entrada em `pages::PAGES`, o ícone em `sidebar::draw_glyph`, os menus em `commands::page_menus`, a tabela
   de traduções da app com `add_table` em `i18n::install`.
3. Um campo no `Shell` (e o encaminhamento de teclas, rato, roda, eventos e árvore de acessibilidade para a página
   selecionada: hoje só há uma, por isso `shell.rs` fala com `self.clean` diretamente; com a segunda, passar a um
   `enum` de páginas).

## Estado

Fase 0 (2026-11-22): páginas do rclean, rdup, rsize e rinst (`rclean::page::CleanPage`, `rdup::page::DupPage`,
`rsize::page::SizePage`, `rinst::page::InstPage`), `Effects`/`Dialogs`/`Mods` no `rt-ui`, `add`/`tr_from` no `rt-core::i18n`, `M::map`; rcare com
barra lateral, menus combinados dos quatro e as quatro páginas. **A passagem de testemunho completa está em
`docs/rcare.pt.md`** (padrão de página, como o rcare funciona, verificado e não, próximos passos): ler primeiro.
Custom Clean (`custom.rs`) e System Maintenance (`maint.rs`, `disks.rs`; fase 2) Startup Optimizer (`startup.rs`, `startup_page.rs`) Speed Up (`speed.rs`, `speed_page.rs`) Program Deactivator (`services.rs`, `services_page.rs`) Extensions (`extensions.rs`, `extensions_page.rs`; fase 3 completa) Registry Cleaner (`registry_page.rs`, motor `rt_clean::registry`; fase 4) e Protect System (`protect.rs`, `protect_page.rs`; `ps.rs` corre o PowerShell e ferramentas sem janela; fase 5), páginas do próprio rcare, feitas. Internet Booster (`net.rs`, `net_page.rs`) e Privacy Sweep (`privacy.rs`, `privacy_page.rs`) também: fase 5 completa. Fase 6: System Information (`sysinfo.rs`, `sysinfo_page.rs`) e Reparar (`repair.rs`, `repair_page.rs`) feitas: as fases 0–6 estão completas; depois, o Agendador (`schedule.rs`, `schedule_page.rs`, `rcare --run`) e as Ferramentas de ficheiros (`files.rs`, `files_page.rs`: pastas vazias e File Shredder; a barra lateral desliza) , o Context Menu Manager (`ctxmenu.rs`, `ctxmenu_page.rs`) e os Processos com Auto Shutdown (`processes.rs`, `processes_page.rs`); o System Information e o Agendador ficam sempre em último (teste em `pages.rs`) (ver os próximos passos em `docs/rcare.pt.md`). Arranca como administrador por omissão (`start.rs`; `--user` sem pedir, `--tray` nunca pede; testes de arranque com `--user`). Feito também: `--tray`, uma só instância, "Integração com o Windows" (um verbo "Analisar com o rcare" → Custom Clean), `rcare-v*` no `release.yml`. Falta: o primeiro release (push: perguntar) e o bucket.
Arranque verificado (abre, fecha com código 0). **Não verificado**: usar a janela. A compilação das apps pequenas religadas em Linux passou no CI
(2026-11-22).
## Regras

- Nada lento no fio da janela: leituras e alterações num `bg::Job`, recolhidas no `animate`.
- Perguntas pelo `Dialogs::confirm` (desenhadas no tema pelo `ShellDialogs`), nunca a caixa do Windows.

- Visível só pelo `strings.txt` (da app ou da página); teclas de acesso diferentes em cada menu em todas as línguas.
- `README.md` e `README.pt.md` atualizados em conjunto.
- Toda a ação destrutiva mostra o que vai fazer, pergunta e guarda cópia para desfazer (ver o plano).

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
