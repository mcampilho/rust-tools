# rcare — suite de manutenção do Windows (plano)

Estado: planeado (2026-11-22). Só Windows, sem limite de tamanho, perfil `release` (não `release-small`).

O `rcare` junta numa só janela o que várias ferramentas pequenas fazem, para não ser preciso abrir várias com
administração. Modelos: BleachBit, CCleaner, Glary Utilities, IObit Advanced SystemCare e Uninstaller, Auslogics
BoostSpeed, Ashampoo WinOptimizer, AVG TuneUp, Wise Care 365, Wintoys, as ferramentas Sysinternals.

## Decisões

1. **Fundir não é copiar.** Os motores já estão em crates (`rt-clean`, `rt-inst`, `rt-dups`, `rt-scan`). Os ecrãs
   saem do `main.rs` de cada app para páginas que a app pequena e o `rcare` usam. O rclean, o rinst, o rdup e o
   rsize ficam como estão (portáteis, pequenos); uma correção faz-se uma vez.
2. **Trait `Page` no `rt-ui`.** Cada página desenha-se, trata o rato e o teclado, corre os seus fios de trabalho e
   dá as suas entradas de menu. O `rcare` só tem a moldura: janela, sidebar, menu, barra de estado.
3. **Sidebar recolhível** desenhada com o `rt-gfx` (sem biblioteca de interface): ícones e texto, ou só ícones. O
   estado guarda-se.
4. **Menus.** Ficheiro e Ajuda são comuns. Entre eles vai o menu da página ativa, reconstruído a cada mudança de
   página. O teste das teclas de acesso corre para cada página.
5. **Administração** pelo serviço `rtsvc` que já existe (nível de utilizador por omissão, elevado nas ações que
   precisam).
6. **Ferramentas nativas do Windows primeiro** (Defender, Firewall, DISM, SFC, `netsh`, `powercfg`, `pnputil`,
   Windows Update); nada de motor próprio de antivírus ou firewall.

## Limites honestos

- Antivírus, spyware, malware, firewall: mostrar o estado do Defender e da Firewall, lançar análises e atualizar
  definições, corrigir o que está desligado. Sem analisador próprio.
- Registry Defrag: o Windows não compacta hives em uso. Só análise, por agora.
- "Limpar" a RAM: esvaziar working sets costuma piorar. Mostrar a memória como está e libertar só o que é seguro.
- "Otimizar" o TCP: o Windows ajusta-se sozinho. Mostrar os valores, perfis seguros e "repor predefinições".
- Update Drivers e Software Updater usam a rede: vão pelas APIs do próprio Windows e precisam de uma exceção
  aprovada à regra "sem rede".
- Reparar a Cortana: fora (já não existe).

## Fases

**0. Fundações** — extrair as páginas do rclean, rinst, rdup e rsize; criar `apps/rcare` (janela, sidebar, menu,
barra de estado, textos em 10 línguas, README en/pt, perfil, workflow de release, bucket do Scoop); ligar ao `rtsvc`.

**1. Páginas existentes** — Disk Cleaner (rclean), Duplicate Files (rdup), Large Files (rsize), Uninstall Apps
(rinst), Custom Clean (lixo, duplicados e grandes numa só análise).

**2. Manutenção num clique** — corre em sequência os módulos disponíveis (privacidade, lixo, registo, atalhos,
verificação de disco, saúde do software) e faz um relatório.

**3. Velocidade** — Startup Optimizer; App/Extension Cleaner e Program Deactivator; Game Mode e Real-Time TuneUp
(monitor de desempenho); Smart RAM.

**4. Registo** — Registry Cleaner (20 categorias, Deep Scan, cópia `.reg` antes de cada limpeza, restauro);
Registry Defrag (análise).

**5. Rede e proteção** — Internet Booster (TCP, DNS Protector, otimizar browsers); Protect System (Defender,
Firewall, anti-tracking, proteção do browser, pasta segura, undelete a avaliar).

**6. Sistema** — Fast Startup, reparar o Menu Iniciar, Update Drivers, Set Default Program, System Information,
saúde do hardware e do software, e uma página de Reparação (SFC `/scannow`, DISM `/RestoreHealth`, `chkdsk`).

**Mais tarde** — Auto Shutdown, Fix problems, Context Menu Manager, Disk Doctor, System Control, File Shredder,
Empty Folders, Shortcut Fixer, Process Manager, Software Updater, Default Program.

## Regras

- Toda a ação destrutiva mostra o que vai fazer, pergunta e guarda uma cópia para desfazer (registo, serviços,
  arranque).
- Os módulos testam-se com dados falsos; as ações reais só numa máquina elevada.
- Textos visíveis no `strings.txt`; README.md e README.pt.md atualizados em conjunto.
