# rscoop-cli

Ferramenta de linha de comandos (sem janela) que verifica uma instalação do Scoop e, com `--fix`, corrige o que
aponta para uma raiz antiga. É o port em Rust de `E:\Projects\AI\Claude\scoop-check` (Python): mesmas verificações,
códigos de problema, opções e relatório; a versão Python serve de referência para comparar resultados
(`--json` de ambas).

## Comandos do Scoop (desde 2026-11-27)

Plano, fases e o que cada uma deixou em `docs/rscoop.pt.md`. O Scoop oficial é a especificação (clone em
`E:\Projects\Git\Categories\System\scoop\Apps\scoop`, v0.6.0, commit e6aa3b36). Feitas as fases 0 a 4; a 5 (extras: `cmd/extras.rs`, `download::prefetch`) em curso.
Comparação diferencial no Windows Sandbox: `sandbox/` (`docs/rscoop.pt.md` §12).

- `cmd/` — `mod.rs` (linha de comandos, `is_native`, ajuda), um módulo por assunto e `delegate.rs`, que corre o
  `scoop.ps1` da instalação para tudo o resto. O que instala ou remove aplicações só passa a nativo quando passar os
  testes de compatibilidade do plano (fases 3 e 4): até lá, o `install`, o `update <app>`, o `import`, o `reset` e
  o `uninstall` só são nativos com `RSCOOP_NATIVE=1` (`cmd/mod.rs`, `on_trial`); os testes ligam-no.
- Fase 4 (em curso): `cmd/install.rs` (`install_app`, também usado pelo `update`), `cmd/update_apps.rs` e
  `cmd/import.rs`. A extração (`Invoke-Extraction`) e os scripts correm no código do Scoop; o `download.rs` dá o
  `to_cache` ao `download` e ao `install`.
- Fase 3: `cmd/shim.rs` (`shim add|rm|list|info`), `cmd/reset.rs`, `cmd/uninstall.rs` e `cmd/machinery.rs` (o que
  partilham: shims de uma aplicação, mensagens do ambiente, processos a correr) sobre os módulos `shims`, `links`,
  `env`, `shortcuts`, `system` e `hooks` do `rt-scoop`. O `Ctx` leva um `rt_scoop::system::System`: o `WinSystem` no
  programa, um `FakeSystem` nos testes (`cmd/tests_ops.rs`, com uma biblioteca do Scoop falsa para os scripts).
  Os scripts dos manifestos correm no código do próprio Scoop (`rt_scoop::hooks::Host`), nunca recriados aqui.
- A leitura da instalação e as escritas comuns (configuração, remoção) estão no crate `rt-scoop`; não duplicar regras
  do Scoop aqui.
- Casos que reporiam ou substituiriam alguma coisa (repositório do Scoop, bucket `main`) vão para o Scoop.
- Remover só com `rt_scoop::disk::remove_tree`, que nunca segue ligações. Nomes de bucket e de alias são validados
  antes de qualquer caminho se formar.
- `sandbox/sandbox.ps1` faz a comparação diferencial no Windows Sandbox (o Scoop e o rscoop-cli nos mesmos passos,
  cada um numa Sandbox nova; descarrega da Internet dentro delas). Ver `docs/rscoop.pt.md` §12.
- `compare.ps1 -Exe … [-Root …]` compara com o Scoop oficial (só leituras). Correr depois de mexer em `cmd/` ou no
  `rt-scoop`.
- Os testes que escrevem (`cmd/tests_write.rs`) usam instalações fictícias, repositórios git no disco, um servidor
  HTTP local e um `scoop.ps1` que só regista o que lhe pedem. Nunca ensaiar escritas nas instalações verdadeiras sem
  pedir.
- Os campos de `--json` e os textos de `Info` do `export` são os do Scoop, em inglês (o `import` lê-os).
- O manifesto `bucket/rscoop-cli.json` tem `"bin": ["rscoop-cli.exe", ["rscoop-cli.exe", "sco"]]` desde a 0.2.0 (lançada a 2026-11-27).

## Módulos da verificação

- `paths.rs` — `clean`/`normpath`/`norm`, `extract_paths` (regra das aspas: só contam se estiverem mesmo à volta do
  caminho, incluindo o `\"` dos `.reg`), `scoop_root_of`, leitura de texto (BOM, UTF-16 sem BOM, Windows-1252).
- `sys.rs` — tipo de ligação pela etiqueta de reparse (`FindFirstFileW`), junções por `FSCTL_SET_REPARSE_POINT`,
  `same_file`, `%VAR%`, hora local, língua do Windows, `WM_SETTINGCHANGE`.
- `lnk.rs` — leitor de `.lnk` (MS-SHLLINK) com destino, pasta, argumentos e ícone (o de `rt_sys::lnk` só dá o
  destino); `build` para os testes.
- `registry.rs` — trait `Registry` (`WinRegistry`, `FakeRegistry` só nos testes).
- `checks.rs` — `Context` (raízes, cache, `scoop.db`, memória de `exists`) e as dez verificações; `configs` corre em
  várias threads (`parallel`).
- `fix.rs` — `Fixer`; os atalhos são recriados por um script PowerShell (WScript.Shell), injetável nos testes.
- `cache.rs` — cache SQLite própria (`files_v1`: chave `tipo|caminho normalizado`, tamanho, mtime em ns, dados) e
  leitura do `scoop.db` do Scoop.
- `report.rs`, `cli.rs`, `text.rs` + `strings.txt` (chave, en, pt, es: o `text.rs` é só `rt_cli::Texts` com a
  tabela; língua por `rt_cli::language("RSCOOP_LANG")`; não usa o `rt_core::i18n`, que é para as aplicações com
  interface em 10 línguas).

## Regras

- Perfil `release-small`. Dependências: `rt-scoop`, `rt-sys` (`net`), `rt-core` (JSON), `rt-cli`, `regex-lite`,
  `rusqlite` com `bundled`, `windows-sys`.
- Os testes criam as árvores em `target\rscoop-tests` (nunca no TEMP, por causa do Bitdefender).
- Em Linux compila (o CI corre clippy no workspace todo) mas só diz que precisa do Windows.
