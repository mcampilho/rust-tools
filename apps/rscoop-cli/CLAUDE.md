# rscoop-cli

Ferramenta de linha de comandos (sem janela) que verifica uma instalação do Scoop e, com `--fix`, corrige o que
aponta para uma raiz antiga. É o port em Rust de `E:\Projects\AI\Claude\scoop-check` (Python): mesmas verificações,
códigos de problema, opções e relatório; a versão Python serve de referência para comparar resultados
(`--json` de ambas).

## Módulos

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

- Perfil `release-small`. Dependências: `rt-core` (só o leitor JSON), `rusqlite` com `bundled`, `windows-sys`.
- Os testes criam as árvores em `target\rscoop-tests` (nunca no TEMP, por causa do Bitdefender).
- Em Linux compila (o CI corre clippy no workspace todo) mas só diz que precisa do Windows.
