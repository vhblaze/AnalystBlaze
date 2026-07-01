# Optimization Safety Matrix

Este documento define como o AnalystBlaze Desktop classifica e executa acoes locais de otimizacao. Ele deve ser lido junto com `src-tauri/src/optimizations/safety.rs`, que e a fonte tecnica do safety gate.

## Principios

- Preferir acoes reversiveis por snapshot.
- Pedir confirmacao local para qualquer acao sensivel.
- Bloquear comandos criticos enquanto nao houver fluxo de rollback, elevacao e consentimento adequado.
- Nunca aceitar caminho de purge enviado pelo caller como autoridade.
- Nao alterar processos, servicos ou apps marcados como criticos/protegidos.
- Separar comando manual, comando remoto e policy local.
- Manter comandos remotos dentro de uma allowlist.

## Estados De Restauracao

| Estado | Significado |
|---|---|
| `reversible` | Ha snapshot local suficiente para tentar desfazer a acao. |
| `quarantine_reversible` | Arquivos foram movidos para quarentena e podem voltar enquanto a quarentena existir. |
| `irreversible_after_confirm` | A acao e permanente depois de confirmada. |
| `blocked` | A acao nao deve ser executada automaticamente na versao atual. |
| `observational` | Apenas coleta dados; nao altera o sistema. |

## Acoes Seguras Ou Observacionais

| Acao | O que faz | Risco | Restauracao |
|---|---|---|---|
| `DETECT_FOREGROUND_GAME` | Detecta jogo/app em primeiro plano. | safe | observational |
| `windows_inventory` | Lista apps de inicializacao e servicos. | safe | observational |
| `network_diagnostics` | Coleta diagnosticos de rede, ping, jitter e perda. | safe | observational |
| `energy_diagnostics` | Le plano de energia, bateria e recomendacoes. | safe | observational |
| `run_performance_scan` | Calcula score e gargalos locais. | safe | observational |
| `scan_cleanup_categories` | Estima bytes recuperaveis por categoria. | safe | observational |
| `scan_startup_impact` | Estima impacto de apps de inicializacao. | safe | observational |
| `audit_log` | Le auditoria local. | safe | observational |
| `optimization_snapshots` | Le snapshots locais. | safe | observational |

## Acoes Reversiveis Por Snapshot

| Acao | O que altera | Snapshot | Restore esperado |
|---|---|---|---|
| `SET_POWER_PLAN_HIGH_PERFORMANCE` | Ativa alto desempenho. | plano anterior | `restore_pending_optimizations` ou sessao |
| `SET_POWER_PLAN_BALANCED` | Ativa plano equilibrado. | plano anterior | `restore_pending_optimizations` ou sessao |
| `SET_POWER_PLAN_POWER_SAVER` | Ativa economia de energia. | plano anterior | `restore_pending_optimizations` ou sessao |
| `APPLY_VISUAL_PERFORMANCE_MODE` | Reduz animacoes, transparencia e efeitos visuais HKCU. | valores de registro anteriores | `RESTORE_VISUAL_EFFECTS` |
| `DISABLE_STARTUP_APP` | Remove entrada de inicializacao segura. | valor original do Registro | `RESTORE_STARTUP_APP` |
| `DELAY_STARTUP_APP` | Move app seguro para fila de inicializacao atrasada. | valor original do Registro | `RESTORE_DELAYED_STARTUP_APP` |
| `STOP_SERVICE` | Para servico modificavel e nao critico. | estado anterior do servico | `RESTORE_SERVICE` |
| `SET_PROCESS_PRIORITY` | Ajusta prioridade de processo permitido. | prioridade anterior | restore de snapshot |
| `APPLY_BACKGROUND_QUIET_MODE` | Reduz impacto de processos de fundo elegiveis. | prioridade/eficiencia anterior | `RESTORE_LATENCY_SESSION` ou restore geral |
| `APPLY_FOREGROUND_BURST_MODE` | Prioriza app em primeiro plano e reduz fundo. | prioridade/afinidade/eficiencia anterior | `RESTORE_LATENCY_SESSION` ou restore geral |
| `ENTER_FOCUS_MODE` | Cria sessao de foco com quiet mode e ajustes reversiveis. | snapshots da sessao | `RESTORE_FOCUS_SESSION` |
| `APPLY_GAME_MODE` | Aplica energia, prioridade, foco, visual e limpeza segura conforme policy. | snapshots agregados | `RESTORE_PERFORMANCE_SESSION`, restore do Modo Gamer ou restore geral |
| `APPLY_PC_CLEAN_FAST_PROFILE` | Mede baseline, aplica limpeza/visual/fundo/startup e mede depois. | snapshots agregados | `RESTORE_PERFORMANCE_SESSION` |
| `APPLY_ADAPTIVE_OPTIMIZATION` | Orquestra otimizacoes conservadoras por contexto. | snapshots dos ajustes aplicados | restore da sessao correspondente |

## Limpeza E Quarentena

| Categoria/acao | O que limpa | Risco | Restauracao |
|---|---|---|---|
| `EMPTY_TEMP` | `%TEMP%`, `TMP`, `%LOCALAPPDATA%\Temp` e opcionalmente `%WINDIR%\Temp`. | sensitive | quarantine_reversible |
| `APPLY_CLEANUP_CATEGORY:user_temp` | TEMP do usuario. | safe/sensitive conforme modo | quarantine_reversible |
| `APPLY_CLEANUP_CATEGORY:windows_temp` | TEMP do Windows. | sensitive, helper recomendado | quarantine_reversible |
| `APPLY_CLEANUP_CATEGORY:directx_shader_cache` | caches DirectX/GPU. | safe | quarantine_reversible |
| `APPLY_CLEANUP_CATEGORY:thumbnail_cache` | cache de miniaturas/icones do Explorer. | safe | quarantine_reversible |
| `APPLY_CLEANUP_CATEGORY:crash_dumps` | dumps antigos locais e WER. | safe | quarantine_reversible |
| `APPLY_CLEANUP_CATEGORY:browser_cache` | caches de navegadores suportados. | safe | quarantine_reversible |
| `APPLY_CLEANUP_CATEGORY:windows_update_cache` | downloads do Windows Update. | sensitive, helper recomendado | quarantine_reversible |
| `APPLY_CLEANUP_CATEGORY:delivery_optimization_cache` | cache do Delivery Optimization. | sensitive, helper recomendado | quarantine_reversible |
| `APPLY_CLEANUP_CATEGORY:memory_dumps` | minidumps e LiveKernelReports. | sensitive, helper recomendado | quarantine_reversible |
| `PURGE_CLEANUP_QUARANTINE` | Apaga permanentemente a quarentena. | sensitive | irreversible_after_confirm |

Enquanto a quarentena existir, os arquivos movidos por limpeza podem ser restaurados por snapshot. Depois de `PURGE_CLEANUP_QUARANTINE`, o espaco e liberado de verdade e a restauracao deixa de ser possivel.

## Acoes Irreversiveis Ou Permanentes

| Acao | Motivo | Requisito |
|---|---|---|
| `PURGE_CLEANUP_QUARANTINE` | Remove os arquivos de quarentena permanentemente. | confirmacao local e payload com `confirmation = "purge_cleanup_quarantine"` |
| Remocao manual de snapshots | Descarta capacidade de restore daquele snapshot. | deve ser uma acao explicita de manutencao |
| Logout local | Remove sessao local; nao altera plano/otimizacoes. | acao manual do usuario |

## Acoes Bloqueadas Ou Nao Automatizadas

| Acao/area | Status | Motivo |
|---|---|---|
| `APPLY_LATENCY_TWEAKS` | blocked | marcado como critical no safety gate. |
| Winsock reset | blocked/manual-only | disruptivo, geralmente exige reboot e rollback robusto. |
| Troca automatica de DNS | blocked/admin-plan | precisa snapshot de DNS por adaptador e confirmacao elevada. |
| Propriedades avancadas de NIC | blocked/admin-plan | dependem de driver/vendor e rollback confiavel. |
| Processos criticos | blocked | `lsass.exe`, `svchost.exe`, `explorer.exe`, Defender e similares sao protegidos. |
| Servicos criticos | blocked | Defender, Windows Update, RPC, Event Log, DHCP, DNS Cache e similares sao protegidos. |
| Prioridade realtime | blocked | nunca deve ser aplicada automaticamente. |

## Regras Para Novas Acoes

1. Defina a acao em `supported_actions`.
2. Adicione um `CommandSafetyProfile` em `command_profile`.
3. Marque se exige confirmacao local, snapshot e helper privilegiado.
4. Adicione validacao de payload em `validate_action_payload`.
5. Se alterar estado, crie `SnapshotEntry` ou documente por que nao ha rollback.
6. Atualize esta matriz com risco, restore e comportamento esperado.
7. Adicione teste cobrindo permissao, bloqueio e payload perigoso.
