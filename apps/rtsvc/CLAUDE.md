# rtsvc — o serviço das rust-tools

Serviço do Windows (`RustToolsSvc`) e utilitário de linha de comandos (`install`, `uninstall`, `status`, `console`, `run`).
Não leva o sufixo `-cli` (é um serviço) nem tem `windows_subsystem`. Desenho, protocolo e ameaças: `docs/service.pt.md`.
Fora do Windows só diz que é só Windows (`platforms = "windows"`).

## Módulos

- `main.rs`, `cli.rs`, `strings.txt` (en/pt/es pelo `rt-cli`) — comandos e mensagens.
- `scm.rs` — Gestor de Serviços: `install` (cria ou altera o serviço, atraso no arranque, ações de recuperação),
  `start`, `stop`, `remove`, `query` (estado e programa registado).
- `install.rs` — `install` (elevado; aviso e confirmação; verifica a pasta com `rt_svc::secure::check_dir` antes e
  depois de copiar o programa; protege `%ProgramData%\rust-tools`), `uninstall`, `status` (liga-se com `Trust::Installed`).
- `service.rs` — o ponto de entrada do serviço (`StartServiceCtrlDispatcherW`), recusa-se a arrancar se a pasta do programa
  não é segura, `start_background` (fotografias, agendador, vigia do registo), `console`.
- `handler.rs` — `Svc`: `ScanVolume` (nível pela `Visibility`, raiz só `X:\…` sem `.`/`..`, no máximo 2 leituras
  de cada vez, progresso a cada ~100 ms, a leitura pára se o cliente sai), fotografias, tarefas (só `Elevated`).
- `snapshots.rs` — `Store`/`Kind`/`Viewer`: fotografias com idade, vista por nível, disco (`.snap` com soma de
  verificação), `tick` do agendador (feito se pedido, ou sujo/velho/em falta e o computador parado; nunca duas
  vezes dentro de `min_gap`). `kinds.rs`: `programs` e `clean`. `idle.rs` e `watch.rs`: parado e vigia do registo.
- `jobs.rs` — `Jobs`/`Executor`: tarefas uma de cada tipo de cada vez, cancelamento no item seguinte, registo por
  item. `work.rs`: o que fazem de verdade (`Clean` por id de `rt_clean` com `admin` e só `Files`/`RecycleBin`;
  `UninstallMsi` só para códigos que são de programas da máquina, `msiexec /x … /qn`).
- `config.rs` — `%ProgramData%\rust-tools`, `service.cfg` (visibilidade), registo em ficheiro (1 MB, roda), `protect`
  (DACL por SDDL).

## Regras

- Nunca aceitar um caminho ou um comando de um cliente; só nomes (ids, códigos) que o serviço resolve.
- Escrever só para `Level::Elevated` (decidido pelo token no pipe). Ler para qualquer autenticado, conforme a `Visibility`.
- Testes: pipes de nome único, nunca o do serviço instalado. Compilar com `CARGO_INCREMENTAL=0` neste PC.
- Por experimentar a sério (elevado): `install`, `uninstall`, o serviço como SYSTEM, `ScanVolume` real, `UninstallMsi`.
- Por fazer: restringir os privilégios do serviço (`SERVICE_CONFIG_REQUIRED_PRIVILEGES_INFO`), tarefas nas janelas do
  rclean e do rinst, indicador "via serviço" e botão para o ignorar, recursos de versão no `.exe`.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
