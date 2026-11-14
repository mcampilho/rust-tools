# runinst — desinstalador portátil em Rust

Lista os programas instalados para todos os utilizadores (sistema, o atual e, como administrador, os outros), com os
atalhos do menu Iniciar e do ambiente de trabalho que lhes pertencem. Um único executável (2,1 MB em Windows; 5,9 MB em
Linux, com os pacotes dpkg/rpm/Flatpak/Snap). Depois de experimentar (2026-11-13), o utilizador pediu as outras fontes
(faltava o ChatGPT, um MSIX): registo, Windows Installer, Loja/MSIX e winget numa só lista sem repetidos, com a
origem; Scoop e Chocolatey em separadores. Depois (2026-11-13): um separador por ferramenta — DISM (só Windows), npm, pip,
Cargo e vcpkg, só quando existem. Pedido do utilizador (2026-11-12): ter em conta entradas do sistema e de
utilizadores, atalhos de todos ou de alguns; sem privilégios vê-se o sistema e o próprio utilizador (o HKLM lê-se sem
administrador), com privilégios tudo. Depois (2026-11-13): botão e menu para desinstalar, seleção múltipla, sem repetidos
nem separadores perdidos quando elevado com outra conta (`Me`: o utilizador da sessão do ambiente de trabalho).
Funcionalidades no `README.md`.

## Comandos

```powershell
./dev.ps1 build --profile release-small -p runinst
./dev.ps1 test -p runinst         # 41 testes (37 em Linux)
./dev.ps1 clippy -p runinst --all-targets
./dev.ps1 test -p runinst --lib screenshots '--' --ignored   # apps/runinst/target/screens/*.png (com os programas deste PC)
./dev.ps1 test -p runinst --lib report '--' --ignored --nocapture   # contagens deste PC
./dev.ps1 test -p runinst --lib leftovers_here '--' --ignored --nocapture   # restos de cada programa, se desaparecesse ($env:RUNINST_ONLY filtra)
./dev.ps1 test -p runinst --lib removes_and_saves '--' --ignored   # remove e repõe HKCU\Software\runinst-test (escreve no registo)
./dev.ps1 test -p runinst --lib orphans_here '--' --ignored --nocapture   # o que "Procurar restos" oferece aqui ($env:RUNINST_ADMIN)
./dev.ps1 test -p runinst --lib tools_here '--' --ignored --nocapture     # separadores DISM/npm/pip/Cargo/vcpkg deste PC
```

Se o `target\release-small\runinst.exe` estiver aberto, compilar com `--target-dir target\alt` (ou `alt2`, se esse
também estiver aberto).

Em Linux (WSL, com `CARGO_TARGET_DIR=$HOME/rt-target`): `cargo test -p runinst --lib packages_here -- --ignored
--nocapture` (pacotes deste sistema), `orphans_here` também, e `screenshots` usa os pacotes reais.

Medido em 2026-11-13 neste PC (sem administrador): 112 entradas (31 visíveis) de registo, Windows Installer e
pacotes em ~0,55 s; `winget list` 66 linhas em ~1 s, todas juntas a uma entrada; 7 atualizações; Scoop com 1307
apps (`D:\Portable\scoop`, ficheiros `scoop-manifest.json`); 102 atalhos; "Admin" e "Convidado" por ler.

## Arquitetura (`src/`)

- `inventory.rs` — `Program` (nome, editor, versão, tamanho de `EstimatedSize`, data de `InstallDate`, pasta,
  comandos, ícone de `DisplayIcon`, código MSI quando `WindowsInstaller` e a chave é um GUID, `system`, `update`
  (`ParentKeyName`, `ReleaseType`, KB), `no_remove`, `Scope`, `Bits`, chave, todos os valores), `read`: HKLM 64 e 32
  bits (repetidos tirados), HKCU, e cada perfil `S-1-5-21-*` do `ProfileList` que não é o atual
  (`rt_sys::win::user_sid`): `HKU\<SID>` se aberto, senão `NTUSER.DAT` com `Key::load_hive` (`RegLoadAppKeyW`, só
  leitura, descarregado ao fechar); falhou → `unread_users`. `parse_date`, `parse_icon`, `command_file`.
  `Program::problems` (`Problem`: `InstallFailed`/`ManifestRemoved` do Scoop, `UninstallerMissing` por
  `uninstaller_gone`: comando absoluto inexistente e pasta inexistente, nunca MSI); etiqueta na lista, linha
  "Problema" nos detalhes, contagem na barra de estado.
- `msi.rs` — produtos do Windows Installer por `MsiEnumProductsExW`/`MsiGetProductInfoExW` (todos os utilizadores com
  `s-1-1-0` se administrador), nunca o `Win32_Product` (que verifica e repara). `inventory::merge_msi`: junta à
  entrada `Uninstall` do mesmo código; sem ela, entrada própria marcada como componente do sistema.
- `appx.rs` — pacotes MSIX/Loja pelo `PackageManager` do WinRT (crate `windows`: `Main | Framework`; todos os
  utilizadores com `FindUsers` se administrador); frameworks, `SignatureKind::System` e pacotes sem `AppListEntry` são
  componentes do sistema; `icon` pelo `IShellItemImageFactory` de `shell:AppsFolder\<AUMID>`. `inventory::add_packages`:
  uma entrada por utilizador.
- `winget.rs` — `winget list --disable-interactivity` sem janela (`CREATE_NO_WINDOW`), num fio próprio; `parse` pelas
  posições das palavras do cabeçalho (linha antes dos traços); `merge`: `ARP\…\<chave>` e `MSIX\<nome completo>`
  exatos, senão pelo nome (o `…` dos nomes cortados); linhas sem par viram entradas próprias.
- `managers.rs` — Scoop (`scoop_all`; `SCOOP`/`~\scoop` e `SCOOP_GLOBAL`/`ProgramData\scoop`; `scoop-manifest.json` ou
  `manifest.json`, `scoop-install.json` ou `install.json`; versão pela pasta de `current` se faltar; sem
  `install.json` → `InstallFailed`, como o `scoop list`; manifesto em nenhum bucket (`scoop_buckets`, `bucket\`,
  raiz ou `deprecated\`) → `ManifestRemoved` — o Scoop atual já não diz isto, os antigos diziam-no sempre que a
  instalação falhava; neste PC, 2026-11-13: 10 falhadas) e Chocolatey
  (`ChocolateyInstall`, `lib\<id>\<id>.nuspec`), lidos das pastas.
- `shortcuts.rs` — o leitor de `.lnk` passou para `rt_sys::lnk` em 2026-11-13 (o rclean também o usa) (MS-SHLLINK: LinkInfo com caminho local Unicode ou ANSI, caminho
  relativo, bloco de variáveis de ambiente; os atalhos "anunciados" do MSI não têm caminho), `read` (menu Iniciar e
  ambiente de trabalho de todos, do utilizador — o ambiente de trabalho pelo `User Shell Folders` — e dos outros
  perfis lidos), `match_programs` (pela pasta mais longa que contém o destino; senão pelo nome simplificado).
- `app.rs` — `Found` (inventário, atalhos, ligações, Scoop, Chocolatey, `winget_done`), `Tab`, `set_winget`, filtros (`Whose`, componentes do sistema, atualizações,
  pesquisa por palavras), ordem (versões pelos números), seleção, caixas marcadas (`ticked`: ficam quando a pesquisa
  muda; com alguma, `chosen` devolve as marcadas em vez da seleção; limpas ao ler de novo e ao mudar de separador),
  CSV. Pedido do utilizador (2026-11-13): marcar vários por pesquisas seguidas, como as caixas do rdup.
- `ui/view.rs` — barra com pesquisa e escolhas (larguras pelo texto), aviso de administrador, lista com ícones
  (16 px), painel de detalhes com deslocamento próprio, barra de estado. A pesquisa é um `rt_core::textedit::TextEdit`
  (`App::search`, `edit_search`): cursor, seleção, colar (Ctrl+V em qualquer lado), menu do botão direito
  (`commands::search_menu`), `search_shift`/`search_offset` para o texto comprido e os cliques; com a caixa em uso,
  Delete/Home/End/←→/Ctrl+A/C/X são dela (`input.rs::search_key`). Pedido em 2026-11-13.
- `uninstall.rs` — `plan` (o que correr por programa e `How`: `UninstallString`; silencioso `QuietUninstallString`,
  `/VERYSILENT` no Inno, `msiexec /X{código} /qn`; `Modify`: `ModifyPath` ou `msiexec /I`, só se `can_modify` — Scoop: `scoop install [--global] <app>` se `Problem::InstallFailed` (o Scoop limpa a instalação falhada), senão `scoop reset <app>` (pedido 2026-11-13; antes o
  Scoop/Chocolatey desinstalava, erro visto nesse dia); nunca Chocolatey/winget/Loja, nem com `NoModify` e `NoRepair`; o botão fica ligado com um programa
  selecionado e explica quando não pode (o utilizador achou-o "sempre inativo": neste PC só 6 de 31 têm comando); Loja: `RemovePackageAsync`, só
  para o utilizador com que o runinst corre; Scoop: `powershell scoop.ps1 uninstall` com `SCOOP`; Chocolatey: `choco
  uninstall -y`, com direitos; só winget: `winget uninstall --id`; `Cannot` com o motivo: outro utilizador, sem
  comando, `NoRemove`) e `carry_out` (`ShellExecuteExW`, `runas` quando precisa; espera pelo processo e pelos
  descendentes por `CreateToolhelp32Snapshot`, porque muitos desinstaladores se copiam para o TEMP e saem; as linhas de
  comando escondidas guardam o fim da saída). Códigos 0, 1605, 1641 e 3010 contam como bem.
- `uninstalling.rs` (no `main`) — confirmar (`MessageBox`), fila num fio com `Stop` depois do atual, `App::busy`
  (linha por cima da lista com arco a rodar e barra de avanço; `Mark` de cada programa pintado na lista; o
  `about_to_wait` redesenha a cada 40 ms enquanto corre; cursor de "a trabalhar"), ler de novo e relatório (desinstalados, ainda instalados pela chave e nome, falharam, não
  começados, reiniciar). Os desinstalados de facto vão para `look_for_leftovers` (num fio, `UserEvent::Leftovers`);
  `remove_leftovers` pergunta e remove num fio (`UserEvent::Removed`), tirando da lista o que saiu.
- `leftovers.rs` — fase 3 (pedido em 2026-11-13). `names` (nome sem versões/arquitetura e nome da pasta de instalação,
  normalizados por `norm`; `GENERIC` e menos de 4 letras não contam), `publisher` (sem Inc./Ltd…), `Keep` (pastas de
  programas instalados: nem elas, nem o que têm, nem o que as tem; lugares procurados, Windows, perfil: nem o que os tem;
  nada a menos de 2 níveis de um disco), `in_folders` (filhos com o nome; a pasta do editor inteira só se só tiver os
  dele e mais nenhum programa do editor estiver instalado), `in_shortcuts`, `elsewhere_ok` (Everything: disco do
  sistema ou do programa, nome com espaço ou dentro da pasta do editor, sem perfis de outros, scoop, projetos,
  caches, cópias...). `win::find`: pasta de instalação, Program Files (+ Common Files), ProgramData, AppData
  (Local/Roaming/LocalLow → `Settings`, não marcado), `AppData\Local\Programs`, menus Iniciar, chaves em
  `HKLM\SOFTWARE` (+ WOW6432Node) e `HKU\<sid>\Software` (`Settings`), valores `Run`, a entrada `Uninstall` se o
  desinstalador sumiu, Scoop (pasta e `persist`), Everything (`rt_scan::everything::search`, `folder:wfn:"nome"`,
  `Elsewhere`, não marcado). `admin`: o que precisa de direitos que o runinst não tem (não se pode marcar).
  `win::remove`: `reg export /reg:64` de cada chave e linhas `.reg` próprias para valores (`reg_value_line`, `hex(2)`
  nos expansíveis) num só `.reg` UTF-16 (`backups\leftovers <data>.reg` na pasta das definições); sem cópia não se
  apaga; depois `reg delete`, e `rt_sys::recycle` para ficheiros e pastas. `win::Scan` lê uma vez os sítios, o que
  fica (`Keep`), os serviços (`Services\*\ImagePath`, `image_file`: `\??\`, `\SystemRoot\`, `system32\`) e as
  tarefas (`System32\Tasks`, XML UTF-16, `<Command>`; sem administrador lê-se pouco). Serviços (`Kind::Service`:
  cópia da chave, `sc stop` + `sc delete`, 1060 = já não existe) e tarefas (`Kind::Task`: cópia do XML em
  `<backup>.tasks\`, `schtasks /Delete /F`) que correm algo na pasta ou algo desaparecido com o nome.
  `find_orphans` ("Procurar restos", sem desinstalar): entradas `UninstallerMissing` como se acabassem de ser
  desinstaladas (não contam como instaladas no `Keep`); atalhos, `Run`, serviços e tarefas (não `\Microsoft\`) que
  correm algo desaparecido de um disco ligado (`gone_file`); a pasta para onde apontavam (`folder_of`, `doubt`);
  pastas vazias logo abaixo dos sítios (`empty_tree`, `plain_dir`: nem ligações nem ocultas+sistema;
  `never_empty_leftover`). `Leftover::doubt`: não vem marcado. Neste PC, sem administrador: 58 em 0,9 s (11
  serviços/drivers da IObit sem ficheiro, 45 pastas vazias, 1 atalho). Medido neste PC (2026-11-13): 60–330 ms por
  programa; sem os filtros, o Everything trazia dezenas de pastas alheias (`steam`, `everything` em projetos).
- `ui/leftovers_view.rs` — o separador Restos (`Tab::Leftovers`; colunas Onde/Programa/Tamanho/Tipo reaproveitando
  `Col`), detalhes com o que a remoção faz, texto para copiar, barra de estado. `App::leftovers` (`Leftovers`: itens,
  marcas próprias, `searching`, `removing`, `failed`); as caixas do `App` (`is_ticked`, `toggle_tick`…) servem os
  dois separadores. Botões `commands::LEFTOVER_ACTIONS` (`render.rs` pergunta `commands::actions(tab)`); Delete
  remove os marcados. `set_leftovers(_, show)`: enquanto procura não muda de separador; sem restos, ou ao fechar,
  volta a `back_tab` (antes saltava para Programas a partir do Scoop).
- `main.rs` (com `input.rs`, `menus.rs`, `render.rs`) — leitura num fio (`refresh`, eventos com geração), ícones num
  fio em lotes de 16 (`rt_sys::win::icon::extract`, cache por ficheiro e índice), "Reiniciar como administrador".
- `linux_pkgs.rs` — fase 5 (pedido em 2026-11-13). Leitores de texto que correm em qualquer sistema (testados em
  Windows): `parse_dpkg` (`/var/lib/dpkg/status`; `config-files` → `Problem::ConfigOnly`, outros estados a meio →
  `InstallFailed`; componente do sistema se `Auto-Installed` em `/var/lib/apt/extended_states`, `Priority`
  required/important, `Section` libs ou `Essential`; `Essential` → `no_remove`; data pelo `info/<pkg>.list`),
  `parse_rpm` (`RPM_FORMAT`, `part_name` para as bibliotecas), `parse_flatpak` (`FLATPAK_COLUMNS`, tamanhos SI
  por `parse_size`; runtimes → sistema; `user` → `Scope::User`), `parse_snap` (colunas pelo cabeçalho; notas
  base/core/snapd → sistema; tamanho e data do `.snap`). `package_id` (`Package` ou `Ref` nos `values`), `is_root`
  (`/proc/self/status`), `on_path`. Em Linux, `inventory::read` = dpkg + rpm; `Found::flatpak`/`snap` em
  `Tab::Flatpak`/`Tab::Snap` (`Tab::LISTS`). `uninstall::linux_plan`: `apt-get remove|purge|install --reinstall`
  com `DEBIAN_FRONTEND` (ou `dpkg`), `dnf`/`zypper`/`yum`/`rpm -e`, `flatpak uninstall|install --reinstall
  --user|--system` (sem `elevate`: pede ele), `snap remove`; `carry_out` em Linux passa por `pkexec env K=V …`
  sem root (sem `pkexec`: `NEEDS_ROOT`). Depois de remover, um pacote `ConfigOnly` conta como desinstalado.
  `leftovers::lin`: só a pasta pessoal (`~/.config`, `~/.local/share`, `~/.local/state` → `Settings`, `~/.cache` →
  `Folder`, `~/.<nome>`, `~/.var/app/<id>`, `~/snap/<nome>`, `.desktop` com `Exec` desaparecido), nada que seja
  o nome de um pacote ainda instalado nem do `NEVER`; `find_orphans`: dados do Flatpak/Snap sem app (só se a
  lista foi lida ou o gestor já não existe), `.desktop` partidos, pastas vazias; remoção para o lixo. Neste WSL
  (Ubuntu): 787 pacotes em 0,34 s, 45 à vista; 8 snaps (4 de sistema); sem Flatpak nem `pkexec`.
- `tools.rs` — fase 4 (pedido em 2026-11-13: um separador por ferramenta, só quando existe). `read_all` num fio
  próprio (`UserEvent::Tools`, guardado em `pending_tools` se vier antes da lista; `Found::tools` e `tools_done`;
  `set_found` mantém os separadores antigos até chegarem os novos; o relatório de desinstalar espera por eles), cada
  ferramenta num fio: `Tab::Dism` (só Windows: `Win32_OptionalFeature` por PowerShell, estado 1, sem administrador;
  com ele, `dism /Get-Capabilities /Format:Table /English` → `parse_capabilities`), `Tab::Npm` (`npm root -g` por
  `cmd /d /c` em Windows; `package.json` de cada pasta e de `@scope/*`; tamanho da pasta), `Tab::Pip` (primeiro de
  `python3`/`py`/`python` no `PATH`, sem os falsos do `WindowsApps`; `pip list -v --format=json` e `--not-required` em
  paralelo: dependências e o que outro instalador lá pôs (`Managed`, em `/usr` sem `installer`) são sistema; data
  do `.dist-info`), `Tab::Cargo` (`CARGO_HOME`/`.crates2.json`; origem, tamanho e data dos `bins`), `Tab::Vcpkg`
  (`VCPKG_ROOT` ou o `vcpkg` do `PATH` através do `.shim` do Scoop, com `.vcpkg-root`; `installed/vcpkg/status`, sem
  as linhas `Feature`). `uninstall::tool_plan`: `dism /Disable-Feature|/Remove-Capability /NoRestart` (com
  direitos), `npm uninstall -g`/`install -g n@v`, `python -m pip uninstall -y`/`install --force-reinstall --no-deps
  n==v` (os `Managed`: `OTHER_MANAGER`), `cargo uninstall`/`install --force --version` (só registo), `vcpkg remove
  n:triplet`; reparar não existe no DISM nem no vcpkg. Não se procuram restos depois destes. Neste PC (2026-11-13,
  sem administrador): 18 funcionalidades, npm 2, pip 188 (45 à vista), Cargo e vcpkg vazios, 3,4 s ao todo; no WSL:
  Cargo 2, pip 79 (todos da distribuição), npm vazio.
- Partilhado: o leitor de JSON passou do rwall para `rt_core::json`. `rt-sys`: `win::reg` (ler o registo nas vistas de 64/32 bits, valores, `load_hive`, `expand`),
  `win::icon` (ícone de um `.exe`/`.dll`/`.ico` em RGBA, máscara para ícones antigos), `win::user_sid`, `win::session_user`,
  `win::sid_of`.

## Fases

1. Feito (2026-11-12/13): inventário de todos os utilizadores (registo, Windows Installer, Loja/MSIX, winget, juntos,
   com a origem), separadores Scoop e Chocolatey, atalhos, ícones, detalhes, pesquisa, CSV.
2. Feito (2026-11-13), por experimentar a sério: desinstalar (normal, silencioso, `msiexec /x`), esperar pelos processos
   filhos, vários seguidos, reparar/modificar.
3. Feito (2026-11-13), por experimentar a sério depois de desinstalar: restos (pasta, Program Files/Common Files,
   ProgramData, AppData, menus Iniciar, atalhos, `Run`, registo, entrada `Uninstall` sem desinstalador, Scoop
   `persist`, Everything, serviços, tarefas), com cópia `.reg` (testada: `removes_and_saves`) e Reciclagem; Procurar
   restos sem desinstalar (entradas sem desinstalador, o que corre algo desaparecido, pastas vazias). Serviços e
   tarefas por experimentar a sério (como administrador).
4. Feito (2026-11-13): separadores DISM (funcionalidades e, como administrador, capacidades), npm, pip, Cargo e
   vcpkg. Por experimentar a sério: desinstalar e reparar em cada um, e as capacidades como administrador.
5. Feito (2026-11-13), antes da fase 4 a pedido do utilizador: Linux com dpkg, rpm, Flatpak e Snap, remover por
   `pkexec`, restos na pasta pessoal. Por experimentar a sério: remover e reparar (o WSL não tem `pkexec`), rpm e
   Flatpak (só testados com texto de exemplo).

## Regras

- Fora dos restos, o runinst não escreve no registo nem apaga ficheiros: só corre os desinstaladores, sempre depois
  de perguntar. Nos restos: só o que o utilizador marca e confirma; o registo é guardado num `.reg` antes (sem cópia
  não se apaga), os ficheiros vão para a Reciclagem; nunca pastas do Windows, perfis, discos, `Program Files`
  inteiro, nem o que é (ou tem, ou está dentro de) um programa ainda instalado (`Keep`). Definições do utilizador e
  o que vem do Everything nunca vêm marcados.
- Qualquer string visível nova precisa de linha completa em `strings.txt`; teclas de acesso diferentes em cada menu
  em todas as línguas (teste).
- `README.md` e `README.pt.md` são atualizados em conjunto.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
