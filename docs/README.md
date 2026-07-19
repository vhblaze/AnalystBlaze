# AnalystBlaze Desktop Documentation

Este diretorio guarda documentos operacionais e de seguranca do agente desktop. O README raiz explica o produto; estes arquivos detalham decisoes, validacoes e limites de automacao.

## Indice

- `optimization-safety-matrix.md`: matriz de acoes locais, risco, reversibilidade, purge e bloqueios.
- `security-assumptions.md`: premissas de seguranca usadas pelo desktop e pela harness.
- `security-pentest.md`: escopo da suite de verificacoes seguras.
- `windows-lab-validation.md`: validacoes manuais permitidas apenas em VM descartavel.
- `adaptive-optimization-manager.md`: desenho do gerente de otimizacao adaptativa.
- `windows-latency-optimizer-wave1.md`: plano de evolucao para otimizacoes de latencia.
- `remediation-checklist.md`: checklist de hardening e pendencias de release.

## Regra De Manutencao

Quando uma nova acao local for adicionada ao agente:

1. Atualize `src-tauri/src/optimizations/safety.rs`.
2. Garanta snapshot, rollback ou justificativa documentada.
3. Atualize `optimization-safety-matrix.md`.
4. Adicione ou ajuste testes.
5. Atualize este indice se um novo documento for criado.
