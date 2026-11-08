# rust-tools — ferramentas portáteis em Rust para Windows

Monorepo (workspace Cargo) com cinco aplicações, cada uma um único `.exe`, e os crates que partilham
(`crates/`):

- `apps/rcalc` — calculadora.
- `apps/rpad` — editor de texto e código.
- `apps/rview` — visualizador de documentos.
- `apps/rsize` — analisador de espaço em disco.
- `apps/rdup` — procurador de ficheiros duplicados (lê os discos com o `rt-scan`, como o rsize).

Cada aplicação tem o seu `CLAUDE.md` com arquitetura e regras próprias.

## Comandos

```powershell
./dev.ps1 dist                               # todas, com o perfil certo, em dist\
./dev.ps1 test --workspace
./dev.ps1 clippy --workspace --all-targets
```

O `dev.ps1` carrega o MSVC portátil (`RUST_TOOLS_MSVC_ENV`, ou `\Portable\Tools\dev\msvc\devcmd.ps1`
em `E:` ou `D:`). Come o `--`: passa `'--'` entre aspas.

## Regras comuns

- "Pronto" = sem avisos, clippy limpo, testes a passar.
- Perfis só na raiz do `Cargo.toml`. rcalc, rpad, rsize e rdup usam `release-small` (`panic = "abort"`);
  o rview usa sempre `release`, porque precisa de `catch_unwind`.
- Compilar para distribuição com `./dev.ps1 dist`, que escolhe o perfil de cada aplicação.
- `README.md` e `README.pt.md` são atualizados em conjunto, na raiz e em cada aplicação.
- Textos visíveis ficam no `strings.txt` de cada aplicação, nunca no código de desenho.
- Sem rede, sem telemetria, sem biblioteca de interface.
- `rustfmt.toml`: `max_width = 140`.

## Releases

`.github/workflows/release.yml` corre com tags `<app>-v<versão>` (ex.: `rview-v1.1.0`). A versão da
tag tem de ser igual à de `apps/<app>/Cargo.toml`. Todas são publicadas para Windows e Linux (o rsize desde a
0.1.0, em 2026-11-07, e a 0.1.1 em 2026-11-08 com a correção do "Mostrar no Explorador" com espaços; o rdup ainda não tem nenhuma versão publicada).

## Testar em Linux

O WSL corre X11 e Wayland, mas não simula tudo: o winit em X11 precisa da `libxkbcommon-x11`, e o ícone
de notificação precisa de um anfitrião StatusNotifierItem, que o WSL não tem. Em Wayland, a barra de título
(`wayland-csd-adwaita`) faz o `weston` do WSLg cair com `signal 11` em cerca de 1 arranque em 4, e o cliente
acaba com `Broken pipe`: ver `/mnt/wslg/stderr.log`. Não é erro das aplicações; testar com `WAYLAND_DISPLAY=`.

No Ubuntu do WSL: compilar com `CARGO_TARGET_DIR=$HOME/rt-target` (fora de `/mnt`, e o `/tmp` perde-se entre
chamadas). A `libxkbcommon-x11` e a `libxcb-xkb1` estão extraídas sem sudo em `~/xkb` (`apt-get download` e
`dpkg -x`, mais a ligação `libxkbcommon-x11.so` → `.so.0`): correr com
`LD_LIBRARY_PATH=$HOME/xkb/usr/lib/x86_64-linux-gnu`.

## Crates partilhados

O código comum está em `crates/` (ver `crates/README.pt.md`): `rt-core` (traduções, definições, árvore de
menus), `rt-gfx` (desenho, texto, menus desenhados), `rt-sys` (Windows e Linux), `rt-scan` (leitura dos
discos: Everything, MFT do NTFS, pasta a pasta; a árvore `model`, `volumes` e `format`; usado pelo rsize e
pelo rdup), `rt-syntax` (realce) e `rt-build` (recursos do `.exe`). As aplicações guardam módulos pequenos com o mesmo nome de antes
(`ui/gfx.rs`, `storage.rs`, `win/mod.rs`, `linux/mod.rs`...) que reexportam dos crates e lhes dão a
identidade da aplicação. Uma correção num destes assuntos faz-se no crate, uma vez.

- Os crates não dependem das aplicações. Textos visíveis dos crates vão para `crates/rt-core/src/strings.txt`
  (procurado depois da tabela da aplicação).
- Linux inclui sempre X11 e Wayland.
- Uma ferramenta nova começa pelos passos de `crates/README.md`.

## Git

Ramo `master`. Commit e push só com aprovação explícita.
