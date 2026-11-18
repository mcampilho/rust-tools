# rsize-cli

Equivalente ao `du` do rsize, só de linha de comandos (sem janela, sem `windows_subsystem`). Lê com o `rt-scan`
(`scan_with`: Everything → NTFS → pasta a pasta, como o rsize) e escreve texto, JSON ou CSV. Primeira aplicação
`-cli` nova depois do rscoop-cli; a regra do sufixo está no `CLAUDE.md` da raiz.

## Módulos

- `cli.rs` — as opções (`Opts`, `parse`, `help`): curtas juntas (`-sh`, `-ahd2`), longas com `=` ou valor a seguir,
  `--` e `/?`. `lang_asked` acha o `--lang` antes do resto, para os erros saírem na língua pedida. `Error::describe`
  traduz.
- `report.rs` — `select` escolhe as entradas (pós-ordem, como o `du`: a pasta depois do que tem dentro; `-d`, `-a`,
  `--sort`), `largest` (`--top`), `write` (texto `tamanho<TAB>caminho`, JSON, CSV). O caminho mostrado é o que o
  utilizador escreveu mais os nomes. Trabalha sobre `Entry` (dados soltos da árvore), por isso testa-se com uma
  árvore feita à mão.
- `main.rs` — `run(args, out, err)` devolve o código de saída (0, 1 não leu um caminho ou pastas, 2 uso errado);
  `collect` lê um caminho (um ficheiro dá o comprimento, sem `alloc` próprio). Os testes de `main` leem pastas reais
  em `target\rsize-cli-tests`.
- `strings.txt` — chave, en, pt, es (tabs). Texto novo = linha completa; o teste `the_table_of_texts_is_complete`
  usa `rt_cli::table_problems`. Não usa o `rt_core::i18n` (10 línguas, só das aplicações com janela).

## Regras

- Perfil `release-small`. Dependências: `rt-scan` e `rt-cli` (sem `windows-sys` direto, sem biblioteca de interface).
- Só leitura: nunca apaga nem escreve fora de stdout/stderr.
- A saída máquina (`tamanho<TAB>caminho`, JSON, CSV) não se traduz; só ajuda e mensagens.
- Os testes criam pastas em `target\rsize-cli-tests` (nunca no TEMP, por causa do Bitdefender).
- `--exclude` (`report::glob`, nome ou caminho; `select` recalcula os totais das pastas sem o excluído, mesmo abaixo
  de `-d`). `-x` é aceite e não faz nada: o `rt-scan` já não entra em pastas de outro sistema (`F_OTHER_FS`). O
  progresso (`--progress`/`--no-progress`) sai por stderr com `\r`, por omissão só num terminal (`run(args, interactive, …)`).
- Por fazer: `-L`, `--time`, `--block-size`, `--threshold`.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
