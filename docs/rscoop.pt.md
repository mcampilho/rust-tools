# rscoop-cli completo — análise e plano

Escrito a 2026-11-27. **Estado:** fases 0, 1 e 2 feitas; fase 3 escrita e testada em instalações fictícias, à espera
da comparação no Windows Sandbox (secção 12); versão 0.2.0 por lançar.

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
- No lançamento, o manifesto `bucket/rscoop-cli.json` passa a criar também o comando `sco`.

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

**Por fazer:** a comparação no Windows Sandbox (secção 7: a mesma lista de aplicações removida e reposta pelos dois,
comparando ficheiros, shims, junções, atalhos e variáveis); o `update` voltar a criar o shim do Scoop; o
`install_psmodule` já existe para a fase 4.
