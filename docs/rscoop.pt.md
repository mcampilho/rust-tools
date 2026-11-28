# rscoop-cli completo — análise e plano

Escrito a 2026-11-27. **Estado (2026-11-28):** os comandos que instalam e removem iguais ao oficial nos quatro cenários
da Sandbox (secção 15), ainda atrás de `RSCOOP_NATIVE=1`.
**Antes:** fases 0 a 3 feitas e comparadas no Windows Sandbox (secção 12); fase 4 com `install`,
`update <app>` e `import` iguais ao oficial na Sandbox (secção 13); fase 5 com os extras (secção 14); versão 0.2.0
lançada a 2026-11-27 (com o comando `sco` no bucket).

## 1. Resumo

- **Objetivo:** o `rscoop-cli` passa a fazer o que o `scoop` faz, muito mais depressa, sobre a mesma instalação, de
  forma que se possa alternar entre os dois sem estragar nada.
- **Como:** de raiz neste workspace, com o Scoop oficial (v0.6.0, 2026-09-30) como especificação. Do bagger, hok, sfsu
  e hyperscoop aproveitam-se ideias, casos de teste e algum código, não a base.
- **Adoção:** desde o primeiro dia, um comando ainda não implementado é entregue ao `scoop.ps1`. O `rscoop-cli` é
  sempre utilizável e o Scoop oficial continua instalado.
- **PowerShell:** só quando um manifesto traz scripts, e então corre com as bibliotecas do próprio Scoop instalado.

## 2. Ponto de partida

### Tempos do Scoop oficial neste PC

Uma medição por comando, em `C:\Users\Miguel\scoop` (361 aplicações):

| Comando | Tempo | Meta para o rscoop |
|---|---:|---:|
| `pwsh` / `powershell` vazios | 0,56 s / 0,38 s | — |
| `scoop prefix git`, `which git` | 0,3 s | < 30 ms |
| `scoop help`, `info 7zip` | 1,2 s / 1,3 s | < 50 ms |
| `scoop search ripgrep` | 4,6 s | < 300 ms |
| `scoop list` | 10,7 s | < 150 ms |
| `scoop status --local` | 24,8 s | < 500 ms |

As metas são objetivos, não medições. Com o disco frio, o antivírus pesa mais do que o código (o `rscoop-cli` atual
demora 261 s na primeira verificação e 17 s nas seguintes), por isso é preciso uma cache própria.

### Há duas instalações do Scoop

`C:\Users\Miguel\scoop` (361 aplicações; é o `scoop` do PATH nesta sessão) e `E:\Portable\scoop` (1501 aplicações,
7212 manifestos em 15 buckets). O `rscoop-cli` tem de tratar bem as duas e de dizer sempre sobre qual está a agir.

### O que os manifestos exigem

| | Instaladas em `E:` (1499) | Todos os buckets (7212) |
|---|---:|---:|
| Com scripts PowerShell | 516 (34 %) | 3354 (47 %) |
| `shortcuts` | 731 | 3337 |
| `persist` | 369 | 2154 |
| `installer` / `uninstaller` | 77 / 80 | 1004 / 745 |
| `innosetup` | 63 | 191 |

Os scripts usam funções internas do Scoop: `Expand-7zipArchive`, `Expand-MsiArchive`, `Expand-DarkArchive`,
`installed`, `appdir`, `ensure`, `shim`, `get_config`, `Add-Path`, `info`, `error`… Um terço das aplicações não se
instala corretamente sem elas.

## 3. Os projetos existentes

| Projeto | Último commit | O que é | Licença |
|---|---|---|---|
| Scoop oficial | 2026-09-30 (v0.6.0) | a referência, ~8 mil linhas de PowerShell | Unlicense ou MIT |
| bagger | 2026-11-03 (1.3.1) | derivado do hok; quase todos os comandos; ~25 mil linhas, 176 testes | Apache-2.0 (biblioteca MIT/Apache) |
| hok | 2026-10-21 (0.1.0-beta.7) | base do bagger; bem estruturado, mas incompleto | Apache-2.0 (biblioteca MIT/Apache) |
| sfsu | 2026-10-20 (1.18.1) | versões rápidas dos comandos de consulta; o autor procura quem o mantenha | MIT/Apache |
| hyperscoop | 2026-10-29 (4.5.7) | `hp`: muitos comandos extra, em evolução própria | MIT |
| scoop-rs | 2021-08-29 | protótipo antigo que deu origem ao hok | MIT/Apache |

### bagger: promissor, mas não é compatível

Lido no código (`crates/scoop-rs`):

- **Não cria os shims nativos.** Para um `.exe` escreve só `nome.cmd`, nunca o par `nome.exe` + `nome.shim`. Deixam
  de funcionar os programas que lançam outro diretamente (`CreateProcess("git.exe")`), e os programas gráficos passam
  por uma consola.
- Perante um shim de outra aplicação **salta**, em vez de o substituir guardando-o como o Scoop faz.
- Não atualiza o próprio Scoop, e não há referências a `hold_update_until`, `autostash_on_conflict` nem ao
  `scoop.db`.
- Dependências pesadas (libgit2, libcurl, SQLite): `.exe` anunciado de 8,5 MB, acima da meta de 5 MB.

Tens o bagger instalado; em `E:\Portable\scoop\shims` não há nenhum shim ao estilo dele.

**Aproveitar:** a lista de funções recriadas para os scripts (como inventário do que testar), os casos de teste de
transações, atualização e remoção parcial, o resolvedor de dependências (`dag.rs`, do hok) e o `SCOOP_FEATURES.md`
como lista de verificação.

### Os outros

- **sfsu:** a melhor ideia é o `sfsu hook`, uma função `scoop` no perfil do PowerShell que desvia só os comandos
  suportados e deixa o resto ao Scoop. Também `outdated`, buckets sem uso e saída em JSON.
- **hyperscoop:** boas ideias de comandos (`uses`, `leaves`, `missing`, `size`, instalação com reversão se falhar).
  Não seguir: escreve os nomes antigos `install.json`/`manifest.json`, a biblioteca não refere `no_junction` nem
  `use_isolated_path`, e extrai um 7-Zip embutido para a pasta temporária.
- **hok:** separação limpa entre biblioteca (sessão e eventos) e linha de comandos.

## 4. Contrato de compatibilidade

O que tem de ficar igual ao Scoop oficial. Cada ponto tem um teste.

**Raiz e configuração**

- Raiz: `SCOOP`, depois `root_path`, depois a pasta onde o Scoop está (`…\apps\scoop\current`), depois `~\scoop`.
  Global: `SCOOP_GLOBAL`, `global_path`, `%ProgramData%\scoop`. Cache: `SCOOP_CACHE`, `cache_path`, `<raiz>\cache`.
- `config.json` portátil em `<raiz>\config.json` passa à frente de `~\.config\scoop\config.json`.
- O `rscoop-cli` atual não considera a localização do Scoop nem o `config.json` portátil: corrigir já na fase 0.
- Respeitar as chaves do `scoop config` (proxy, `private_hosts`, `gh_token`, `aria2-*`, `no_junction`,
  `use_isolated_path`, `default_architecture`, `shim`, `use_external_7zip`, `use_lessmsi`, `use_sqlite_cache`,
  `ignore_running_processes`, `force_update`, `update_nightly`, `show_manifest`, `hold_update_until`,
  `scoop_repo`, `scoop_branch`, `last_update`, `alias`) e preservar as desconhecidas ao gravar.

**Disco**

- `apps\<app>\<versão>`; `current` é uma junção só de leitura (ou não existe, com `no_junction`).
- `scoop-install.json` e `scoop-manifest.json` (ler também os nomes antigos); JSON como o Scoop o escreve (UTF-8 sem
  BOM, CRLF, 4 espaços).
- Shims: um formato por tipo (`.exe`+`.shim`, `.cmd`, `.ps1`, `.jar`, `.py`, bash); o `shim.exe` copiado do núcleo do
  Scoop segundo `shim`; subsistema gráfico acertado; o shim substituído fica `nome.shim.<app>` e é reposto ao remover.
- `persist`: junção para pastas, ligação física para ficheiros, `.original` quando há dados nos dois lados.
- Cache: `app#versão#sha7.ext` (e o nome antigo, ao ler).
- Atalhos em `Scoop Apps`, com subpastas; módulos em `modules` e `PSModulePath`.
- PATH e variáveis no registo, `REG_EXPAND_SZ` quando há `%`, com `WM_SETTINGCHANGE`.

**Comportamento**

- Comparação de versões igual (portar os testes `Scoop-Versions.Tests.ps1`), incluindo `nightly`.
- Ordem dos buckets (os conhecidos primeiro, pela ordem do `buckets.json`), pasta `deprecated`, `app@versão` pelo
  `scoop.db` e pelo histórico git.
- Ordem exata da instalação: descarregar, verificar a soma, extrair, `pre_install`, `installer`, `current`, shims,
  atalhos, módulos, ambiente, `persist`, `post_install`, gravar os dois JSON.
- `hold`, instalação falhada, processos a correr, direitos para `--global`, atualização automática com mais de 3 h,
  recurso de arquitetura em ARM64.
- Se `use_sqlite_cache` estiver ligado, manter o `scoop.db` atualizado.

Não precisa de ser igual byte a byte: o texto no ecrã. O `export` tem de ser igual, para o `import` funcionar nos dois
sentidos.

## 5. Arquitetura

```
crates/rt-scoop        a biblioteca: raiz e configuração, manifestos, versões, buckets, estado
                       instalado, shims, junções, persist, ambiente, atalhos, transações
apps/rscoop-cli        argumentos (os mesmos do scoop), saída (texto e --json), en/pt/es
```

O `rt-scoop` serve depois o rlaunch, o rinst e o rclean, que já têm cada um o seu conhecimento do Scoop.

| Assunto | Escolha | Porquê |
|---|---|---|
| Scripts de manifestos | um PowerShell por aplicação, que carrega `apps\scoop\current\lib\*.ps1`, mantém as variáveis entre `pre_install` e `post_install` e só arranca se houver scripts | compatibilidade exata sem recriar as funções |
| git | o `git.exe` do Scoop | configuração, proxy e credenciais do utilizador; sem libgit2 |
| Extração | os mesmos programas do Scoop (7-Zip, msiexec ou lessmsi, innounp, dark) | mesmos resultados; o tempo está no 7-Zip, não em quem o chama |
| Descargas | WinHTTP (`rt_sys::net`, a que falta gravar em ficheiro, cabeçalhos e progresso), várias em paralelo; aria2 se a configuração o pedir | sem libcurl |
| JSON | `serde_json` com a ordem preservada | ler e gravar manifestos e configuração |
| Cache | SQLite próprio, por tamanho e data (como hoje) | `list`, `search` e `status` imediatos |
| Concorrência | um bloqueio por raiz | o Scoop não tem; evita duas instâncias do rscoop ao mesmo tempo |

Tamanho: o atual tem 2,4 MB. Estimo que o completo fique abaixo de 5 MB, a confirmar na fase 1.

**Regras do repositório que mudam:** o `rscoop-cli` passa a usar a rede e a apagar e instalar ficheiros. Precisa da tua
aprovação explícita e de ficar escrito no `CLAUDE.md`.

## 6. Fases

Cada fase termina utilizável, com testes e comparação com o Scoop oficial.

### Fase 0 — fundamentos

`rt-scoop` com raiz, configuração, manifestos, versões, buckets e estado instalado. Entrega ao `scoop.ps1` de tudo o
que não estiver feito. Um script de comparação que corre o mesmo comando nos dois e compara o `--json`/os dados.

### Fase 1 — consulta (não escreve nada)

`list`, `search`, `info`, `cat`, `status` (primeiro `--local`), `which`, `prefix`, `depends`, `home`, `help`,
`bucket list|known`, `cache show`, `config` (ler), `export`, e `checkup`.

O **`checkup`** junta o oficial (bucket `main`, caminhos longos, modo de programador, 7-Zip, innounp, dark, NTFS,
exclusões do Defender) às dez verificações atuais e ao `--fix`. As opções de hoje continuam a funcionar sem subcomando.

### Fase 2 — escrever sem instalar

`update` (Scoop e buckets, em paralelo), `status` com rede, `bucket add|rm`, `download`, `cache rm`, `config` (gravar),
`hold`/`unhold`, `cleanup`, `alias`.

### Fase 3 — a maquinaria, em operações com volta

`shim`, `reset`, `uninstall`: shims, junções, `persist`, ambiente, atalhos, módulos e o anfitrião de scripts.

### Fase 4 — instalar e atualizar

`install`, `update <app>|*`, dependências, `app@versão`, manifestos por URL ou ficheiro, `--global`, `import`. Com
reversão quando algo falha a meio, desde que o estado final de um sucesso seja o do Scoop.

### Fase 5 — extras

Descargas paralelas entre aplicações, `uses`, `leaves`, `size`, buckets sem uso, `outdated`, `hook` para o perfil do
PowerShell, `virustotal`, `create`.

**Fora do plano:** as ferramentas de quem mantém buckets (`checkver`, `autoupdate`, `checkhashes`, em `bin\`).
Continuam as oficiais.

## 7. Como garantir a compatibilidade

1. **Testes portados** dos `*.Tests.ps1` oficiais (versões, manifestos, dependências, opções, extração).
2. **Testes de unidade** em árvores falsas em `target\`, com registo em memória, como hoje.
3. **Comparação diferencial:** instalar, atualizar e remover a mesma lista de aplicações com o `scoop` e com o
   `rscoop-cli` em duas raízes iguais e comparar os ficheiros, shims, junções, atalhos e variáveis. O Scoop oficial
   escreve no registo verdadeiro, por isso isto pede uma máquina descartável (Windows Sandbox, se o ativares).
4. **Interoperação:** instalar com um e atualizar ou remover com o outro, nos dois sentidos.
5. **Acompanhar o oficial:** registar o commit de referência e rever o que muda em `lib\` e `libexec\` a cada versão.

Até a fase 4 passar estes testes, não usar o `install`/`uninstall` do `rscoop-cli` na instalação de 1501 aplicações.

## 8. Riscos

- **Alvo em movimento:** o Scoop mudou há pouco os nomes dos JSON; os projetos parados ficaram para trás por isso.
- **Scripts de manifestos** podem fazer qualquer coisa, incluindo terminar o processo ou pedir uma resposta: o
  anfitrião tem de aguentar os dois casos.
- **Antivírus:** o Bitdefender deste PC já matou executáveis recém-compilados; os testes ficam dentro do projeto.
- **Nome:** já existe o "Rscoop" (interface gráfica, AmarBego), que tens instalado. O `rscoop-cli` não colide, mas um
  nome curto `rscoop` colidiria.

## 9. Decisões

1. **Rede e escrita no `rscoop-cli`.** Recomendo aprovar, limitado ao que o `scoop` faz: git dos buckets, os URL dos
   manifestos e a API do VirusTotal só quando pedida.
2. **Nome para escrever.** O nome fica `rscoop-cli` (decidido: evita o "Rscoop" e segue as ferramentas `-cli`).
   Recomendo que o manifesto do bucket crie também o comando curto `sco`, livre neste PC e nos teus buckets
   (`"bin": ["rscoop-cli.exe", ["rscoop-cli.exe", "sco"]]`); quem preferir outro usa
   `scoop shim add <nome> rscoop-cli`.
3. **Onde experimentar as fases 3 e 4.** Recomendo Windows Sandbox para a comparação e uma raiz de ensaio para o dia
   a dia.
4. **Modelos.** Sonnet 5.5 nas fases 0, 1, 2 e 5; Opus 5.5 nas fases 3 e 4, com revisão do `rt-scoop` antes.

A rede e a escrita (ponto 1) ainda não são usadas: confirmar ao começar a fase 2 e registar então a regra no
`CLAUDE.md`.

## 10. Fases 0 e 1: o que ficou

Crate `rt-scoop` e os comandos `list`, `search`, `info`, `cat`, `status --local`, `which`, `prefix`, `depends`,
`home`, `bucket list|known`, `cache show`, `config`, `export`, `checkup` e `help`; o resto é entregue ao Scoop.
Executável de 2,8 MB, sem cache nova (não foi precisa).

Comparado com o Scoop oficial (`apps/rscoop-cli/compare.ps1`) nas duas instalações: `list`, `status --local`,
`search`, `bucket known`, `cache show`, `depends`, `prefix` e `info` iguais; `checkup` com o mesmo resultado. Única
diferença: em `E:`, o `bucket list` oficial fica sem a origem, porque o git recusa repositórios de outro utilizador do
Windows.

Diferenças deliberadas ou por fazer:

- `search` sem resultados não consulta no GitHub os buckets conhecidos.
- `info -v` não mostra o tamanho da descarga.
- Manifestos de um endereço web não são lidos (`status` dá-os como atualizados).
- `which` não conhece os aliases nem as funções do PowerShell.
- O manifesto `bucket/rscoop-cli.json` cria também o comando `sco` (desde a 0.2.0).

## 11. Fase 2: o que ficou

`update` (Scoop e buckets), `status` com rede, `bucket add|rm`, `download`, `cache rm`, `config` (gravar),
`hold`/`unhold`, `cleanup` e `alias`. Executável de 3,0 MB. O `rt_sys::net` ganhou a descarga para ficheiro, que segue
os redirecionamentos um a um e volta a decidir os cabeçalhos em cada endereço.

Entregue ao Scoop, de propósito: atualização que reponha ou substitua repositórios (Scoop ou bucket `main` sem git,
outro repositório ou ramo configurado, sem o shim do Scoop), `use_sqlite_cache`, `use_isolated_path`, `app@versão`,
manifestos de um endereço da web, descargas com `proxy`, FTP, FossHub, metalink ou release privado do GitHub.

Mais cauteloso do que o oficial: nomes de bucket só simples; `cleanup` sem versão atual conhecida não apaga; remoção
que nunca segue ligações; um `config.json` ilegível não é reescrito.

Desvios conhecidos do oficial (revisão de 2026-11-27, deixados de propósito):

- `update` sem o bucket `main`: o oficial aborta ("Failed to remove local 'main' bucket", porque o `rm_bucket` de um
  bucket que não existe devolve 1); aqui os outros buckets são atualizados.
- `update` com um bucket cujo `pull` falha: o oficial não dá por isso e sai com 0; aqui avisa e sai com 1 (o
  `last_update` é gravado na mesma, como no oficial).
- `scoop config <nome> 8` guarda o número 8 e `$true` guarda `true`, como quando o `scoop` é chamado do PowerShell;
  chamado do `cmd`, o oficial guardaria os textos `"8"` e `"$true"`.
- Uma ligação física só de leitura do `persist` é apagada e o ficheiro guardado mantém o atributo (o
  `std::fs::remove_file` do Rust 1.100 apaga sem o tirar); o `Remove-Item -Force` do oficial tira-o ao ficheiro
  guardado. A junção `current` só de leitura perde o atributo ela própria (`attrib -R /L`); a versão para onde aponta
  não é tocada (testado).

Na mesma revisão: o `bin` de um manifesto passou a levar como argumentos todos os elementos depois do nome
(`$target, $name, $arg = $item`), que o Scoop escreve no `.shim` separados por espaços; o `remove_tree` passou a tirar
o atributo só de leitura às pastas também para lá dos 260 caracteres; o `update -q` entregue ao Scoop leva o `-q`.
Ficou por tratar (menor): no Linux, o `rt_sys::net::download` deixa o `curl` seguir os redirecionamentos com os mesmos
cabeçalhos e sem limite de tempo; um redirecionamento relativo só com `?…` e um `Location` de mais de 8192 caracteres
falham; o `checkup --json` com `--help` ou `--version` não dá JSON válido.

Ensaiado só em instalações fictícias, com repositórios git no disco e um servidor HTTP local. Nas instalações
verdadeiras só correram comandos de leitura. Por experimentar: `update` e `status` contra o GitHub, uma descarga
HTTPS verdadeira, o aria2, e o `-g` como administrador.

Diferenças por tratar: o `update` não volta a criar o shim do Scoop nem a verificar a pasta dos shims no `PATH`
(fase 3); em `E:`, o git recusa os repositórios enquanto o proprietário das pastas for o SID antigo.

## 12. Fase 3: o que ficou

Feito a 2026-11-27 (Opus 5.5). Executável de 3,4 MB.

**No `rt-scoop`** (cada função diz a do Scoop que segue):

- `shims`: o `shim` oficial para cada tipo (`.exe`/`.com` → cópia do `shim.exe` escolhido por `shim` mais `.shim`
  com `path`/`args`, subsistema gráfico copiado, `System32`↔`Sysnative` para um `shim.exe` de 32 bits; `.bat`/`.cmd`,
  `.ps1` (caminho relativo a partir da pasta dos shims, ou absoluto noutro disco), `.jar`, `.py` e o resto para
  `bash`, cada um com o `.cmd` e o script `sh` ao lado, com os mesmos textos e fins de linha). O shim de outra
  aplicação fica como `nome.ext.<app>` (`warn_on_overwrite`) e o `rm_shim` repõe o mais recente.
  `Get-ShimTarget`, `get_app_name`, `Get-PESubsystem`/`Get-PEMachine`.
- `links`: `link_current`/`unlink_current` (junção só de leitura na própria junção), `persist_data` (junção para
  pastas, ligação física para ficheiros, `.original`, pasta vazia quando não há nada), `unlink_persist_data` (junções e
  ficheiros com mais de um nome), `persist_permission` (`icacls … /grant:r *S-1-5-32-545:(OI)(W)`), módulos.
- `env`: `Split-PathLikeEnvVar` com o `-like`, `Add-Path`, `Remove-Path`, `Set-EnvVar` (`REG_EXPAND_SZ` com `%`, a
  espécie antiga, apagar quando fica vazia, `WM_SETTINGCHANGE`), `env_add_path`/`env_rm_path` com
  `use_isolated_path`, `env_set`/`env_rm`. O `ExpandString` dos valores de `env_set` é feito aqui para `$nome`,
  `${nome}` e `$env:NOME`; outra coisa (`$(…)`, um acento grave, `$HOME`) entrega o `reset` inteiro ao Scoop.
- `shortcuts`: `Scoop Apps` e subpastas, alvo e ícone que têm de existir, `$dir`/`$original_dir`/`$persist_dir`.
- `system`: o que sai das pastas do Scoop, num trait — `WinSystem` (registo, `IShellLinkW`/`IPersistFile` como o
  `WScript.Shell`, pastas conhecidas, processos) e `FakeSystem` para os testes.
- `hooks`: o anfitrião dos scripts. Um PowerShell por aplicação (`pwsh` se houver, como o `delegate`) carrega as
  bibliotecas que o `scoop.ps1` e o comando carregam, define as variáveis do comando (`$cmd = 'uninstall'`, que 580
  manifestos com scripts de remoção consultam, `$app`, `$dir`, `$manifest`, `$architecture`…) e recebe as linhas a
  correr por pipes anónimos herdados; a consola fica livre para o que os scripts escrevem e para o `Read-Host`. As
  linhas são as funções do próprio Scoop (`Invoke-HookScript`, `Invoke-Installer -Uninstall`), na mesma sessão,
  por isso o `$global:` passa de um script para o seguinte, como no oficial. Um `exit`, um `abort` ou um erro que
  pare o PowerShell acabam o comando com o mesmo código.

**No `rscoop-cli`:** `shim add|rm|list|info` (o `alter`, que pede uma escolha, é do Scoop), `reset` e `uninstall`
(o de `scoop` é do Scoop). A ordem do `uninstall` é a do oficial; o `reset` decide antes de mexer se o entrega ao
Scoop. **O `reset` e o `uninstall` só são nativos com `RSCOOP_NATIVE=1`** até passarem a comparação diferencial
(secção 7); sem isso continuam a ir para o Scoop.

**Comparado:** `compare.ps1` passou a comparar também o `shim list`: 873 shims iguais em `C:\Users\Miguel\scoop`
(nome, caminho, origem, alternativas, global, escondido), em 1,3 s contra 41 s. O `IsHidden` conta com os aliases e
funções do PowerShell por omissão (`pwsh` 7 e 5.1, listas fixas) e com as funções das bibliotecas do Scoop. Os
atalhos reais por COM foram lidos de volta com o leitor de `.lnk`; o registo verdadeiro não foi tocado.

**Desvios conhecidos:**

- Um `bin` de um só elemento (`["x.exe"]`) não tem nome no `rm_shims` oficial e o shim fica; aqui sai.
- `persist_data` pára no primeiro erro (por exemplo, um `.original` que é uma pasta); o oficial escreve o erro e
  continua.
- O `current` que seja uma pasta verdadeira, no fim do `uninstall`, é removido; o oficial pára com um erro.
- `shim add` por cima de um shim de uma aplicação, com um alvo de fora do Scoop: o oficial apaga o shim antigo
  (`"$shim."` é o próprio ficheiro no Windows); aqui o antigo fica de lado como `nome.ext.<app>`.
- Sem manifesto instalado, o `uninstall` segue com um manifesto vazio (o oficial parava no `Invoke-HookScript`).

**Por fazer:** o `update` voltar a criar o shim do Scoop. (A comparação no Windows Sandbox, abaixo, já correu.)

### Comparação no Windows Sandbox (preparada a 2026-11-27, por correr)

`apps/rscoop-cli/sandbox/`: `sandbox.ps1` (no PC), `inside.ps1` (na Sandbox), `snapshot.ps1` (a fotografia).

```powershell
./apps/rscoop-cli/sandbox/sandbox.ps1                 # os dois lados e a comparação (target\sandbox\report.txt)
./apps/rscoop-cli/sandbox/sandbox.ps1 -Side rscoop    # só um lado outra vez
./apps/rscoop-cli/sandbox/sandbox.ps1 -CompareOnly    # só a comparação
./apps/rscoop-cli/sandbox/sandbox.ps1 -Side rscoop -KeepOpen   # a Sandbox fica aberta, para ver
./apps/rscoop-cli/sandbox/sandbox.ps1 -Scenario all            # os quatro cenários (secção 15)
# Desde a fase 4 os passos são seis: 1-installed (`install`), 2-updated (`update --force`), 3-reset,
# 4-uninstalled, 5-official e 6-imported (`import`); o git e os buckets continuam a ser postos pelo Scoop.
```

- Duas Sandboxes novas, uma depois da outra, com o Scoop em `C:\scoop` nas duas. Os passos são os mesmos, só muda
  quem faz os passos 2 e 3: `1-installed` (o instalador do Scoop, `git`, o bucket `extras` e as aplicações, pelo
  Scoop), `2-reset` (o mesmo estrago — junções `current`, shims, atalhos, variáveis do `env_set` e pastas das
  aplicações no `PATH` — e `reset *`), `3-uninstalled` (`uninstall` de quatro aplicações, a última com `--purge`),
  `4-official` (o Scoop remove o resto: a interoperação).
- Aplicações por omissão: `jq`, `which`, `psutils`, `gsudo`, `oh-my-posh`, `notepadplusplus`, `everything`,
  `posh-git`, `ditto` (mais `git` e `7zip`), escolhidas por terem, entre elas, shims `.exe` de consola e com janela e
  `.ps1`, `bin` com nome e argumentos, atalhos, `persist` de pastas e ficheiros, `env_set`, `env_add_path`,
  `pre_uninstall`, `uninstaller` e módulos do PowerShell. `-Apps` e `-First` mudam a lista.
- O primeiro lado guarda os commits dos buckets em `pins.json` e o segundo usa-os: as mesmas versões dos dois lados.
- A fotografia: os ficheiros, pastas e ligações da raiz (sem `cache`, `buckets` nem o próprio Scoop) com tamanho,
  só de leitura, destino das ligações, número de nomes e o conteúdo dos shims, do `persist` e dos JSON das
  aplicações; as variáveis do utilizador; `HKCU\Software\Classes`, `Run` e `Uninstall`; os atalhos de `Scoop Apps`.
- **Descarrega da Internet, dentro das Sandboxes:** o instalador (`get.scoop.sh`), o Scoop, o git, os buckets `main`
  e `extras` e as aplicações — umas centenas de MB, guardadas em `target\sandbox\share\cache` para a vez seguinte.
  No PC só se escreve em `target\sandbox`.
- Experimentado aqui: a fotografia (numa instalação fictícia) e a comparação (com uma diferença feita de propósito).
  Primeira volta (2026-11-27): o passo 1 do lado oficial correu (11 aplicações, 11 272 entradas); parou no estrago
  por erro do script, e mostrou que o instalador traz o `main` em zip (agora volta a ser posto com git) e que desligar
  a Sandbox por dentro dá "connection to the sandbox was lost" no Windows 10 (agora é o PC que a fecha).
- Segunda volta (2026-11-27), os quatro passos dos dois lados: **tudo igual** (11 272 ficheiros, pastas e ligações,
  as variáveis, 3682 linhas do registo, os atalhos) **menos uma linha**: o `uninstall` do rscoop-cli deixava a
  ligação `modules\posh-git`. Aponta para `current`, que o `uninstall` já tirou; o `exists()` seguia a ligação
  partida e dizia que não estava lá (o `Test-Path` do oficial diz que está). Corrigido no `uninstall_psmodule`, com
  teste.
- Terceira volta (2026-11-27, só o lado `rscoop`, com as mesmas versões): **os quatro passos iguais** — 11 272,
  11 272, 10 956 e 60 entradas na raiz, as variáveis, o registo e os atalhos; códigos de saída 0 dos dois lados.
  O `reset` e o `uninstall` continuam atrás de `RSCOOP_NATIVE=1` até o utilizador decidir tirá-lo.

## 13. Fase 4: o que ficou (em curso)

Começada a 2026-11-27 (Opus 5.5). Executável de 3,5 MB.

**Feito:** `install` e `update <app>|*`, atrás de `RSCOOP_NATIVE=1` como os da fase 3.

- `install` (`cmd/install.rs`): opções `-g -i -k -s -u -a`, as dependências e os ajudantes (`Get-Dependency`),
  o aviso de já instalada, `prune_installed`, o aviso do aria2, e para cada aplicação o `install_app` na ordem do
  oficial. Feito aqui: a descarga para a cache e a soma (o mesmo código do `download`, com aria2 quando o Scoop o
  usaria), a cópia para a pasta da versão, `ensure_install_dir_not_in_path`, `current`, shims, atalhos, módulo,
  `env_add_path`, `env_set` (no código do Scoop quando o valor é uma expressão), `persist`, o `scoop-manifest.json`
  (cópia byte a byte) e o `scoop-install.json` (`bucket`, depois `architecture`, CRLF), as notas e as sugestões.
  No código do próprio Scoop, num PowerShell por aplicação: `Invoke-Extraction` (7-Zip, msi, Inno Setup, dark e
  zip sem 7-Zip, tal e qual), `pre_install`, `Invoke-Installer` e `post_install`, com as variáveis do
  `install_app` (`$cmd`, `$app`, `$version`, `$dir` — a pasta da versão, depois `current` —, `$original_dir`,
  `$persist_dir`, `$fname`, `$manifest` pelo `Get-Manifest`…).
- `update` (`cmd/update_apps.rs`): a lista como o oficial (desatualizadas, fixadas, já na última versão, `*` e
  `-a`, `-g`, `-f`, `-q`), e para cada uma: a versão nova para a cache com a soma, depois a antiga desligada como no
  `uninstall` mas sem apagar a pasta (fica para o `cleanup`; com `-f` e a mesma versão vai para `_<versão>.old`),
  os scripts dela com `$cmd = 'update'` e as variáveis do `update` (`$old_version`, `$old_manifest`…), e a nova
  instalada no mesmo PowerShell, com as dependências que faltem.
- Vai inteiro para o Scoop: `app@versão`, manifestos de um ficheiro ou endereço (e as aplicações instaladas assim),
  a cache SQLite, `show_manifest`, um proxy, descargas FTP/FossHub/release privado, e uma instalação falhada (que o
  oficial repara com `reset` ou `uninstall`).

**Testado:** em instalações fictícias, com um servidor HTTP local e a biblioteca do Scoop falsa: dependência
instalada primeiro, a soma conferida, a cópia, os scripts pela ordem certa e com as variáveis certas, os dois JSON
iguais aos do oficial, a remoção a seguir; o `update` da 1.0 para a 2.0 (scripts de remoção com `$cmd = 'update'`,
a 1.0 fica, `current` na 2.0, variáveis novas) e o `--force` da mesma versão.

**Comparado na Sandbox** (2026-11-27, as mesmas 11 aplicações, cinco passos, instalação e `update --force` feitos pela
ferramenta de cada lado): **tudo igual** — 11 272, 11 541, 11 543, 10 969 e 60 entradas na raiz, as variáveis, o
registo e os atalhos; códigos de saída 0. A primeira volta mostrou um erro do `inside.ps1` (a lista passada ao
`scoop` como um só argumento; o `update` oficial não tinha corrido), corrigido.

- `import` (`cmd/import.rs`, 2026-11-27): as definições (`set_config`: `"true"`/`"false"` passam a booleanos, o nome a
  minúsculas), os buckets (`bucket add`; um que já exista é dito e o resto segue) e as aplicações, cada uma como um
  `install` à parte (uma que falhe não pára as outras, como o `& scoop-install.ps1` do oficial), com `--global` e
  `--arch` tirados do `Info`, e `hold` se estava fixada. `<auto-generated>` dá `app@versão`, que vai para o Scoop;
  um ficheiro de um endereço e `use_isolated_path` nas definições também. Testado numa instalação fictícia; na
  Sandbox há um passo 6 (`6-imported`: o `import` do `scoop export` do passo 1 depois de o Scoop ter removido tudo).

  Na Sandbox (2026-11-27, os dois lados, seis passos): **tudo igual** também no `6-imported` (11 312 entradas, as
  variáveis, o registo e os atalhos); códigos de saída 0.

- Reversão (2026-11-27): o oficial deixa uma instalação falhada, que o `install` seguinte apaga com um `uninstall`;
  aqui isso faz-se logo. Se o `install_app` falha depois de criar a pasta da versão (descarga, soma, extração,
  scripts, ligações), `install::roll_back` tira o que a aplicação já tinha posto — shims (os de outra aplicação voltam),
  atalhos, `current` se aponta para esta versão, módulo se é o dela, `PATH`, variáveis do `env_set`, ligações do
  `persist` e a pasta da versão; os dados persistidos ficam. Num `update`, se a versão nova falha depois de a antiga
  ter sido desligada, a antiga volta a estar em uso (o `reset` dela; com `--force` a `_<versão>.old` volta ao lugar).
  O que os scripts já fizeram (o `uninstaller` da versão antiga, um `pre_install`) não se desfaz. Testado com um
  `post_install` que falha, no `install` e no `update`; a Sandbox voltou a dar os seis passos iguais (a reversão só
  muda o caminho da falha, que o oficial não tem igual).

**Por fazer:** descargas em paralelo entre aplicações (fase 5);
decidir quando sai o `RSCOOP_NATIVE=1`.

## 14. Fase 5: o que ficou

Começada a 2026-11-27 (Opus 5.5). Executável de 3,5 MB. Comandos que o Scoop não tem; só leem, menos as descargas.

- **Descargas em paralelo** (`download::prefetch`): o `install` de várias aplicações (com as dependências) e o
  `update` de várias descarregam primeiro, quatro de cada vez, o que não está na cache; depois cada uma instala-se
  como antes e confere a soma do que encontra na cache. O que falhe aqui é descarregado outra vez pela instalação,
  que diz porquê. Não com aria2 (que já faz as suas ligações) nem com `--no-cache`.
- **`outdated`**: as aplicações com versão mais recente no bucket (o `app_status` do `status`), com fixadas e globais
  marcadas; `--json`. 1,9 s nas 360 de `C:`.
- **`leaves`** e **`uses <app>`**: quem precisa de quem, pelas `depends` do manifesto instalado e pelos ajudantes
  que o Scoop acrescenta (`rt_scoop::resolve::needs`: 7-Zip, lessmsi, innounp ou innounp-unicode, dark ou
  wixtoolset). 1,4 s.
- **`size [app…]`**: a pasta de cada aplicação (todas as versões, sem seguir ligações), o `persist` e a cache dela,
  da maior para a menor; `--json` em bytes. Lê o disco todo: 56 s nas 360 de `C:` a frio.
- **`bucket unused`**: os buckets locais de onde não veio nenhuma aplicação instalada.
- **`hook`**: escreve uma função `scoop` para o perfil do PowerShell (`rscoop-cli hook | Out-String |
  Invoke-Expression`) que manda tudo ao `rscoop-cli`: sem argumentos dá a ajuda, uma opção primeiro vai ao Scoop
  (`--version`), o resto como está. Diferença: um comando entregue ao Scoop corre noutro processo, por isso o que ele
  muda na sessão (o `PATH` do próprio terminal) só se vê num terminal novo.

**Fora:** `virustotal` e `create` são do Scoop e continuam a ir para ele (o `virustotal` pediria a API do
VirusTotal, que a regra de rede do repositório não dá ao `rscoop-cli`).

**Testado:** os extras numa instalação fictícia (desatualizada e fixada, dependência, tamanhos e cache, bucket sem
uso, o texto do `hook`) e as duas descargas do teste do `install` feitas ao mesmo tempo; nas instalações verdadeiras
só as leituras e o `hook` numa sessão à parte. Na Sandbox, com a cache vazia antes do lado `rscoop`:
10 ficheiros descarregados ao mesmo tempo no `install` e os seis passos iguais aos do oficial. (A cache é partilhada:
o lado que corre primeiro enche-a; para repetir, apagar `target\sandbox\share\cache` antes de `-Side rscoop`.)

## 15. Comparação alargada e o que faltava (2026-11-28)

**Os três pontos por fazer:**

- O `update` volta a criar os shims do próprio `scoop` no fim, como o `Sync-Scoop` (`shim "$currentdir\bin\scoop.ps1"`).
- As aplicações instaladas a partir de um endereço da web: o `status`, o `outdated` e o `update` vão buscar o manifesto
  a esse endereço (o `url_manifest`; um caminho `\\servidor\…` lê-se do disco), e uma que não responda conta como
  removida, como no oficial. A regra de rede do `CLAUDE.md` da raiz diz-o.
- `app@versão` (`cmd/versions.rs`): o ficheiro do bucket se já tem essa versão, senão o histórico git do bucket
  (`show HEAD`, `log -G`, `log -S`, como o `Find-HistoricalManifestInGit`), escrito em `<raiz>\workspace\<app>.json`; a
  aplicação fica "de um caminho" (`url` antes de `architecture` no `scoop-install.json`, a ordem do Windows PowerShell;
  o manifesto guardado com uma linha a mais no fim, como o `Out-UTF8File`). O que só o `autoupdate` faria, e o
  `use_git_history` desligado, ficam com o Scoop.

**Cenários da Sandbox** (`sandbox.ps1 -Scenario default|global|settings|heavy|all`; a fotografia passou a ter a raiz
global, as variáveis da máquina, o menu Iniciar de todos e as definições do Scoop sem o `last_update`):

| Cenário | O que tem | Resultado |
|---|---|---|
| `default` | as 11 aplicações de antes, e no fim `fzf@0.74.3` do histórico | igual nos sete passos |
| `global` | 5 aplicações com `--global` (`C:\ProgramData\scoop`, `HKLM`, menu de todos) | igual |
| `settings` | `no_junction` e `use_isolated_path` ligados | igual |
| `heavy` | `python` (instalador por dark/msi, `pip` no `post_install`), `vscode`, `windows-terminal` | igual, depois das correções abaixo |

**O que as voltas mostraram (corrigido):**

- O `update` oficial grava `scoop_repo` e `scoop_branch` sempre que arranca, também quando só atualiza aplicações.
- `Add-Path` numa variável que não existe deixa um `;` no fim (o `Split-PathLikeEnvVar` com um valor nulo).
- Os scripts correm numa sessão do PowerShell que não via as mudanças feitas aqui ao `PATH` e às variáveis; no oficial,
  o `Add-Path`, o `Remove-Path`, o `env_set` e o `env_rm` mudam também a sessão (o `post_install` do `python` corre
  `python`). Agora as mesmas mudanças vão para a sessão dos scripts (`machinery::mirror`), antes do `post_install` e do
  `post_uninstall`.
- Erros do próprio ensaio: o `--global` passado letra a letra (uma lista de um só elemento desfeita pelo `if`), e o
  `app@versão` corrido depois de o Scoop ter removido o `git`.
- Os `.pyc`, os lançadores do `pip` e o `RECORD` levam a hora da instalação: a fotografia compara-os só pelo tamanho.
