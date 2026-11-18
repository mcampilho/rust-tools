# rdup-cli

O `fdupes` do rdup, só de linha de comandos (sem janela, sem `windows_subsystem`). Pesquisa com o motor do crate
`rt-dups` (`search::find_with`: leitura pelo `rt-scan`, tamanho → pontas → conteúdo BLAKE3, cache de hashes) e
escreve texto, JSON ou CSV. Só lista: não apaga nem liga nada (a regra de segurança do rdup mantém-se). A regra do
sufixo `-cli` e o `rt-cli` estão no `CLAUDE.md` da raiz.

## Módulos

- `cli.rs` — as opções (`Opts`, `parse`, `help`) ao estilo do fdupes: curtas juntas (`-rSm`), longas com `=` ou valor
  a seguir, `--` e `/?`. `-r`, `-R` e `-n` são aceites e não mudam nada (é sempre recursivo e os vazios nunca
  entram). `Opts::filter()` monta o `rt_dups::filter::Filter` (`-A` junta `.*` aos nomes excluídos).
  `lang_asked` acha o `--lang` antes do resto. `Error::describe` traduz.
- `report.rs` — `write`: texto do fdupes (um ficheiro por linha, linha em branco entre grupos; `-S`, `-1`, `-f`),
  resumo `-m` (cópias = todos menos um de cada grupo; espaço = `Group::wasted`), JSON, CSV. Trabalha sobre
  `Group`, por isso testa-se com grupos feitos à mão.
- `main.rs` — `run(args, out, err)` devolve o código (0 pesquisou, 1 não leu algo, 2 uso errado). A cache fica em
  `hashes.bin` do rdup (`Location::detect` com o `Place` do rdup: partilhada com a janela), ou em `--cache`;
  `--no-cache` desliga. O progresso vai para stderr (uma linha com `\r`) só se for um terminal e sem `-q`.
- `strings.txt` — chave, en, pt, es (tabs). Texto novo = linha completa; o teste `the_table_of_texts_is_complete`
  usa `rt_cli::table_problems`.

## Regras

- Perfil `release-small`. Dependências: `rt-dups`, `rt-scan`, `rt-cli`, `rt-core` (só o `store::Location`).
- Os caminhos saem completos (o motor normaliza as raízes), não como o utilizador os escreveu.
- Só leitura, mais a cache de hashes (`hashes.bin`, escrita por `ficheiro .tmp` + `rename` no fim).
- A saída máquina (lista, JSON, CSV) não se traduz; só ajuda, resumo e mensagens.
- Por fazer: apagar/ligar (`-d`, `-N`, `-L`, `-B`, sempre com confirmação e nunca todas as cópias), `-H` (ligações
  físicas como cópias), pastas duplicadas (`Options::folders`), caminhos como o utilizador os escreveu.

## Git

Faz parte do monorepo `rust-tools`: ver o `CLAUDE.md` da raiz.
