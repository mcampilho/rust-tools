# rust-tools — ferramentas portáteis em Rust para Windows

Monorepo (workspace Cargo) com três aplicações, cada uma um único `.exe`:

- `apps/rcalc` — calculadora.
- `apps/rpad` — editor de texto e código.
- `apps/rview` — visualizador de documentos.

Cada aplicação tem o seu `CLAUDE.md` com arquitetura e regras próprias.

## Comandos

```powershell
./dev.ps1 dist                               # as três, com o perfil certo, em dist\
./dev.ps1 test --workspace
./dev.ps1 clippy --workspace --all-targets
```

O `dev.ps1` carrega o MSVC portátil (`RUST_TOOLS_MSVC_ENV`, ou `\Portable\Tools\dev\msvc\devcmd.ps1`
em `E:` ou `D:`). Come o `--`: passa `'--'` entre aspas.

## Regras comuns

- "Pronto" = sem avisos, clippy limpo, testes a passar.
- Perfis só na raiz do `Cargo.toml`. rcalc e rpad usam `release-small` (`panic = "abort"`);
  o rview usa sempre `release`, porque precisa de `catch_unwind`.
- Compilar para distribuição com `./dev.ps1 dist`, que escolhe o perfil de cada aplicação.
- `README.md` e `README.pt.md` são atualizados em conjunto, na raiz e em cada aplicação.
- Textos visíveis ficam no `strings.txt` de cada aplicação, nunca no código de desenho.
- Sem rede, sem telemetria, sem biblioteca de interface.
- `rustfmt.toml`: `max_width = 140`.

## Releases

`.github/workflows/release.yml` corre com tags `<app>-v<versão>` (ex.: `rview-v1.1.0`) e publica só
Windows. A versão da tag tem de ser igual à de `apps/<app>/Cargo.toml`. Linux é compilado e testado,
mas não publicado: o que falta está no `README.md`. Atualizar esse quadro ao acrescentar suporte.

## Código repetido

`syntax/lexer.rs` e `win/tray.rs` são iguais no rpad e no rview; `win/darkmode.rs`, `syntax/lang.rs`,
`i18n/mod.rs`, `ui/gfx.rs` e `storage.rs` são quase iguais. Uma correção num destes ficheiros
provavelmente aplica-se às outras aplicações. Ainda não há crates partilhados.

## Git

Ramo `master`. Commit e push só com aprovação explícita.
