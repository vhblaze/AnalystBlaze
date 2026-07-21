# Compatibilidade Windows 10 / Windows 11

Levantamento feito na Tarefa B1. Cobre os 18 modulos de `src-tauri/src/optimizations/` e os pontos sensiveis a versao do Windows em `src-tauri/src/telemetry/`.

## Deteccao

`src-tauri/src/optimizations/os_version.rs` le `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\CurrentBuildNumber` uma unica vez por processo (cacheado com `OnceLock`) e classifica em `WindowsGeneration::{Windows10, Windows11, Unknown}` pelo numero de build (>= 22000 = Windows 11), nao pelo texto `ProductName` (que pode ainda dizer "Windows 10" em builds antigas do Windows 11).

## Tabela por modulo

| Modulo | Diferenca Win10 vs Win11? | Detalhe |
|---|---|---|
| `os_version.rs` | infraestrutura nova | detecta a geracao, usada pelos dois itens abaixo |
| `visual_effects.rs` | **sim** | `TaskbarAnimations` e `EnableAeroPeek` sao no-ops conhecidos na barra de tarefas reescrita do Win11 (Fluent, icones centralizados, sem Aero Peek classico) - essas duas chaves deixam de ser escritas no Win11. As outras 6 chaves (VisualFXSetting, transparencia, listview, MinAnimate, MenuShowDelay, DragFullWindows) sao Control Panel/Explorer classicas e continuam identicas nas duas versoes. |
| `energy.rs` / `snapshot.rs` (esquema de energia) | **sim** | Win11 tem um "Modo de energia" (slider Eficiencia/Equilibrado/Desempenho) que e um *overlay scheme* separado do esquema classico (`powercfg /getactualoverlayscheme`). Agora lido nos diagnosticos e, ao aplicar Alto Desempenho/Economia no Win11, tambem setamos o overlay correspondente (best-effort, nao bloqueia se falhar). No Win10 esse overlay nao e consultado nem setado. |
| `adaptive.rs` | atualizado | o gate "e Windows 10 ou 11" trocou de string-matching no rotulo do SO (`sysinfo`) para o build number real de `os_version.rs`; o comportamento em si (todas as otimizacoes adaptativas) continua identico nas duas versoes. |
| `safety.rs` | nao | lista de servicos criticos usa nomes classicos do SCM (WinDefend, wuauserv, etc.), identicos nas duas versoes. Nenhuma ramificacao por SO. |
| `windows_actions.rs` | nao | consulta/altera servicos por `sc.exe` com os mesmos nomes nas duas versoes. |
| `windows_inventory.rs` | nao | enumera `Run`/`RunOnce`, presentes identicamente desde o Windows 7. |
| `snapshot.rs` (demais entradas) | nao | registro, processos, DNS, quarentena de arquivo - todos usam APIs/caminhos inalterados entre as versoes. |
| `network_admin.rs` | nao | `netsh`/`ipconfig` identicos nas duas versoes. |
| `privileged_helper.rs` | nao | named pipe + `sc.exe`, sem dependencia de versao do SO. |
| `processes.rs` | nao | `SetPriorityClass`/afinidade/eficiencia sao APIs Win32 inalteradas. |
| `memory.rs` | nao | nao interage com nada especifico de versao. |
| `cleanup.rs` | nao | caminhos de cache/temp sao os mesmos. |
| `focus.rs` | nao | logica propria do app, sem dependencia de SO. |
| `latency.rs` | nao | logica propria do app. |
| `performance_suite.rs` | nao | orquestra os modulos acima; herda o comportamento deles. |
| `detection.rs` | nao | deteccao de jogo por processo em primeiro plano, API inalterada. |
| `protected_apps.rs` | nao | armazenamento proprio do app. |
| `local_ai_policy.rs` | nao | politica propria do app. |

## Fora do escopo desta tarefa (achados, nao implementados)

- **Game Mode nativo do Windows**: o "Modo Gamer" deste app e 100% proprio (perfil de energia + limpeza + prioridades) e nunca leu/escreveu `HKCU\SOFTWARE\Microsoft\GameBar\AutoGameModeEnabled` (o toggle real do SO). Nao ha diferenca de codigo a corrigir aqui porque simplesmente nunca foi implementado - se quiser que o app tambem detecte/ative o Game Mode nativo do Windows, isso e trabalho novo, nao um ajuste de compatibilidade.
- **`StartupApproved\Run`**: ao desabilitar um app de inicializacao, o app remove o valor de `Run` mas nao atualiza a flag correspondente em `...\Explorer\StartupApproved\Run`, que e a fonte de verdade da tela Configuracoes > Aplicativos > Inicializar (identica nas duas versoes desde o Windows 8/10 1809). Isso pode fazer o proprio painel do Windows mostrar "Ativado" para um app que este app ja desabilitou. Nao e uma diferenca Win10-vs-Win11 (afeta as duas igualmente), entao ficou fora do escopo de B1, mas vale registrar para uma tarefa futura de inicializacao (ligado a Tarefa C4).
- **Nomes de servico**: nao foi encontrado nenhum servico usado pelo app que tenha sido renomeado entre Win10 e Win11.

## Regras de seguranca preservadas

- Nenhuma acao nova (overlay scheme) contorna `safety::command_profile`/`validate_command` - o overlay e setado dentro do mesmo fluxo ja existente de `SET_POWER_PLAN_*`, que ja e `Sensitive` e ja exige snapshot.
- Toda mudanca nova e revertida por um `SnapshotEntry` (`PowerOverlayScheme`, novo, aditivo - nao altera o formato dos snapshots ja salvos).
- Nenhuma acao classificada Critical foi tocada.
