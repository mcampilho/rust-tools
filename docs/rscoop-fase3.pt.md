# rscoop-cli: passagem de testemunho para a fase 3

Escrito a 2026-11-27 (Sonnet 5.5) para a sessão com o Opus 5.5. Ler primeiro este ficheiro, depois
`docs/rscoop.pt.md` (plano e o que as fases 0 a 2 deixaram) e `apps/rscoop-cli/CLAUDE.md`.

## Estado

- Fases 0 a 2 feitas, commit `7caafa8`, enviado para o GitHub. **Sem release** (versão 0.2.0 no `Cargo.toml`; o
  último publicado é o 0.1.3). No release, `bucket/rscoop-cli.json` passa a criar também o comando `sco`.
- **Este ficheiro está por fazer commit.**
- CI do `7caafa8` verde: clippy e testes em Windows e Linux, formato, versão mínima do Rust e auditoria.
- O utilizador corrigiu o proprietário de `E:\Portable\scoop` (o git já o aceita) e experimentou o `status` e o
  `download` verdadeiros: "OK". O aria2 e o `-g` como administrador continuam por confirmar.
- Windows Sandbox: ativado antes do reinício; confirmar que existe `C:\Windows\System32\WindowsSandbox.exe`.

## O que foi pedido

Um Scoop em Rust, rápido e **compatível em tudo** com o oficial (o mais importante, nas palavras do utilizador), sobre
a mesma instalação. Nome `rscoop-cli`, comando curto `sco`. Respostas em português europeu.

## Primeiro: rever antes de avançar

**Feita a 2026-11-27** (Opus 5.5): correções e desvios em `docs/rscoop.pt.md` §11. O `Bin.args` é agora uma lista
(todos os elementos depois do nome); o `remove_tree` tem testes da junção `current` só de leitura e de caminhos
longos.

Foi combinado que o Opus revê o `rt-scoop` e a fase 2 antes de escrever a fase 3. Onde olhar com mais atenção:

- `crates/rt-scoop/src/disk.rs` (`remove_tree`): a fase 3 vai depender dele para apagar aplicações. Atenção às
  ligações físicas do `persist` (o atributo só de leitura é partilhado).
- `apps/rscoop-cli/src/cmd/update.rs`: `hand_over` decide o que vai para o Scoop; o `old` é apagado como no oficial.
- `cmd/maintain.rs` (`clean_app`): quando é que se apaga uma versão.
- `rt-scoop/src/resolve.rs` e `apps.rs`: base das dependências e do "instalado".
- `rt-sys/src/net.rs`: foi reorganizado e é usado por "Procurar atualizações" em todas as aplicações.
- `cmd/checkup.rs` junta dois JSON à mão.

Limites conhecidos: `which` ignora aliases e funções do PowerShell; manifestos de endereços web não são lidos; o
`update` não volta a criar o shim do Scoop nem confirma os shims no `PATH`; o `rt-scoop` lê os manifestos sem cache.

## Fase 3: `shim`, `reset`, `uninstall`

Especificação: o Scoop oficial em `E:\Projects\Git\Categories\System\scoop\Apps\scoop` (v0.6.0, `e6aa3b36`).

| Assunto | Onde está no oficial |
|---|---|
| Criar shims (por tipo de ficheiro), substituir e repor | `lib\core.ps1` (`shim`, `warn_on_overwrite`, `get_shim_path`, `Get-ShimTarget`), `lib\install.ps1` (`create_shims`, `rm_shim`) |
| Comando `shim` | `libexec\scoop-shim.ps1` (ainda não lido) |
| `current`, `persist` | `lib\install.ps1` (`link_current`, `unlink_current`, `persist_data`, `unlink_persist_data`, `persist_permission`) |
| Ambiente e `PATH` | `lib\system.ps1`, `env_set`/`env_add_path` em `lib\install.ps1` |
| Atalhos e módulos | `lib\shortcuts.ps1`, `lib\psmodules.ps1` |
| Scripts dos manifestos | `Invoke-HookScript`, `Invoke-Installer` (`lib\install.ps1`) |
| `uninstall`, `reset` | `libexec\scoop-uninstall.ps1`, `scoop-reset.ps1` |

Pontos que é fácil falhar:

- O shim de um `.exe`/`.com` é `nome.exe` (cópia de `apps\scoop\current\supporting\shims\<shim>\shim.exe`, `kiennq`
  por omissão) mais `nome.shim` (`path = "…"`, `args = …`); programa gráfico → o `shim.exe` é marcado como gráfico.
  O bagger falha exatamente aqui (só escreve `.cmd`).
- Um shim de outra aplicação é guardado como `nome.shim.<app>` e reposto na remoção, pela data de escrita.
- `current` é uma junção com atributo só de leitura (`no_junction` dispensa-a).
- `persist`: junção para pastas, ligação física para ficheiros, `<nome>.original` quando há dados nos dois lados.
- Os scripts veem `$dir`, `$original_dir`, `$persist_dir`, `$version`, `$app`, `$manifest`, `$architecture`,
  `$global`, `$bucket`, e as variáveis passam de um script para o seguinte. A proposta do plano: um PowerShell por
  aplicação que carrega `apps\scoop\current\lib\*.ps1`. Falta desenhar o protocolo (um script pode fazer `exit`,
  `abort` ou `Read-Host`).
- Ordem do `uninstall`: `pre_uninstall`, processos a correr, `uninstaller`, shims, atalhos, `current`, módulo, `PATH`,
  variáveis, `persist`, pasta, `post_uninstall`, versões antigas, `--purge`.
- No ambiente: `REG_EXPAND_SZ` quando há `%`, `WM_SETTINGCHANGE`, `use_isolated_path`.

Há código a aproveitar no próprio `rscoop-cli`: junções (`sys::make_junction`), registo (`registry.rs`, com um falso
para testes), leitura de shims (`checks::parse_shim`) e de `.lnk` (`lnk.rs`).

## Como ensaiar

- **Nunca** correr comandos que escrevam nas instalações do utilizador sem pedir. Há duas:
  `C:\Users\Miguel\scoop` (a do `PATH` nas sessões do Claude) e `E:\Portable\scoop` (1500 aplicações).
- Testes em instalações fictícias em `target\` (ver `cmd/tests_write.rs`); o registo com `FakeRegistry`.
- Comparação diferencial no Windows Sandbox: a mesma lista de aplicações com o `scoop` e com o `rscoop-cli` em duas
  raízes, comparando ficheiros, shims, junções, atalhos e variáveis; e a interoperação nos dois sentidos. Ainda não há
  nada preparado (pasta mapeada, script `.wsb`).
- `apps\rscoop-cli\compare.ps1 -Exe <exe> [-Root <raiz>]` compara as leituras com o oficial: correr depois de mexer
  no `rt-scoop`.
- Descarregar ficheiros da Internet para ensaios pede autorização.

## Este PC

- Compilar: `./dev.ps1 …` (carrega o MSVC portátil e come o `--`: passar `'--'`). Antes,
  `$env:CARGO_INCREMENTAL="0"`; para o `cargo fmt`, pôr `E:\Portable\scoop\apps\rust\current\bin` à frente no `PATH`.
- Testes com `TEMP`/`TMP` dentro do projeto (`target\tmp`): o Bitdefender mata executáveis corridos do TEMP.
- Linux (o CI corre clippy no workspace todo):
  `wsl -- bash -lc 'export PATH="$HOME/.cargo/bin:$PATH" CARGO_TARGET_DIR="$HOME/rt-target"; cd /mnt/e/Projects/AI/Claude/rust-tools && cargo clippy --workspace --all-targets --locked -- -D warnings'`
- Os ficheiros têm CRLF ou LF misturados na cópia de trabalho: em substituições por script, normalizar primeiro; e
  cuidado com o `$_` no texto de substituição do `-replace`.
- Commit, push e release só com aprovação explícita. Uma edição em bloco que incluía `CLAUDE.md` foi recusada pelo
  modo automático: mexer nesses ficheiros um a um e com aprovação.
