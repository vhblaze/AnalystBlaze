# AnalystBlaze Desktop

AnalystBlaze Desktop e um aplicativo Tauri para Windows com frontend React/Vite e agente local em Rust. Ele monitora o computador, calcula diagnosticos de performance e aplica otimizacoes locais com confirmacao, auditoria e snapshots de restauracao.

O desktop nao substitui o web app: ele atua como o agente instalado na maquina do usuario. Login, plano, assinatura e insights remotos continuam sendo responsabilidade do backend/web do AnalystBlaze.

## O Que O App Faz

- Coleta telemetria local de CPU, GPU, RAM, disco, temperatura, energia, rede, latencia, uptime, janela ativa e tempo ocioso.
- Gera um Performance Scan com score, gargalos, metricas e comparacao antes/depois.
- Aplica perfis como Modo Gamer, Modo Foco e PC limpo/rapido.
- Executa limpezas de arquivos temporarios e caches com quarentena reversivel.
- Ajusta plano de energia, efeitos visuais e prioridades de processos.
- Gerencia apps de inicializacao e servicos seguros do Windows com snapshots.
- Recebe comandos remotos allowlisted, mas exige confirmacao local para acoes sensiveis.
- Mantem historico local de auditoria, snapshots e sessoes restauraveis.

## Arquitetura

```text
src/
  components/analystblaze/     UI principal React
  hooks/                       estado de auth, tema e telemetria
  services/tauri/agent.ts      ponte TypeScript para comandos Tauri
  services/telemetry/          eventos de interface

src-tauri/src/
  lib.rs                       comandos Tauri e bootstrap do agente
  auth/                        deep link, sessao e credenciais
  api/                         cliente autenticado e HMAC
  telemetry/                   coleta local e diagnosticos
  optimizations/               acoes locais, snapshots e safety gate
  audit.rs                     log local de eventos sensiveis
```

O frontend chama comandos Tauri por `src/services/tauri/agent.ts`. O backend Rust valida a acao em `optimizations/safety.rs`, executa a rotina em `optimizations/mod.rs` e registra snapshots em `optimizations/snapshot.rs` quando a acao e reversivel.

## Telas Principais

- **Dashboard**: resumo de status, usuario, saude do agente e atalhos principais.
- **Telemetry**: amostra local de CPU/GPU/RAM/disco/rede/temperatura.
- **Insights**: area para insights autenticados vindos do backend.
- **Local Controls**: painel operacional para limpeza, Modo Gamer, Modo Foco, energia, startup apps, servicos, helper admin, snapshots e auditoria.
- **Settings**: conta, login, billing, idioma e configuracoes basicas.

## Modelo De Seguranca

O projeto assume que qualquer acao capaz de alterar o Windows precisa passar por uma camada local de seguranca.

- Acoes sensiveis exigem confirmacao local.
- Comandos remotos precisam estar em allowlist assinada/politica autorizada.
- Acoes criticas sao bloqueadas por padrao.
- Processos e servicos essenciais do Windows ficam protegidos por denylist.
- Apps protegidos pelo usuario nao devem ser otimizados automaticamente.
- Alteracoes reversiveis criam snapshots locais antes/depois.
- Limpeza de arquivos usa quarentena antes de purge permanente.
- Secrets nao sao expostos por deep link, log, variaveis `VITE_*` ou argumentos.
- O helper privilegiado deve ser instalado apenas a partir de fonte confiavel e assinada.

Veja a matriz completa em [`docs/optimization-safety-matrix.md`](docs/optimization-safety-matrix.md).

## Reversivel Vs Irreversivel

Regra geral: o agente prefere mover, pausar, atrasar ou alterar com snapshot em vez de apagar ou modificar permanentemente.

| Tipo | Exemplos | Restauracao |
|---|---|---|
| Reversivel por snapshot | plano de energia, efeitos visuais, apps de inicializacao, prioridade de processos, servicos parados, arquivos em quarentena | `restore_pending_optimizations`, restauracao de sessao ou restore especifico |
| Reversivel enquanto a quarentena existe | TEMP, caches, dumps e categorias de cleanup | snapshot move os arquivos de volta |
| Irreversivel | purge da quarentena de limpeza | exige confirmacao explicita; depois nao ha restore |
| Bloqueado/nao automatizado | tweaks criticos de latencia/admin, Winsock reset, alteracoes sem rollback confiavel | precisa fluxo futuro com helper, consentimento e rollback |

## Desenvolvimento

Instale dependencias:

```bash
npm install
```

Abra apenas o frontend Vite:

```bash
npm run dev
```

Abra o aplicativo desktop em modo desenvolvimento:

```bash
npm run desktop:dev
```

Gere build frontend:

```bash
npm run build
```

Gere pacote desktop:

```bash
npm run desktop:build
```

Em maquinas com Smart App Control ativo, os scripts `desktop:dev`, `desktop:build`, `cargo:check`, `cargo:test` e `cargo:clippy` passam por wrappers em `scripts/` para reutilizar o fluxo local de assinatura.

## Verificacao Local

```bash
npm run build
npm run cargo:test
```

Para a harness de seguranca:

```powershell
cd src-tauri
cargo test --test security_harness
```

## Ambiente

Copie `.env.example` quando precisar configurar endpoints:

- `VITE_ANALYSTBLAZE_TELEMETRY_URL`: endpoint opcional para lotes de telemetria de interface.
- `VITE_ANALYSTBLAZE_INSIGHTS_URL`: endpoint opcional para insights.
- `ANALYSTBLAZE_API_URL` e `ANALYSTBLAZE_WEB_URL`: usados pelo agente Rust/Tauri em runtime.
- Em `production`, endpoints HTTP sao bloqueados. Use `https://`; `http://localhost` e `http://127.0.0.1` sao aceitos apenas em modo dev.

Sem endpoint de telemetria, os eventos ficam salvos localmente e limitados pelo tamanho maximo configurado.

Por padrao, batches enviados ao backend usam telemetria minimizada: nomes de janelas ficam apenas locais, listas de processos nao saem cruas, SSID e hostname exigem opt-in, e DNS/gateway sao enviados apenas como resumo. Use `ANALYSTBLAZE_DIAGNOSTIC_TELEMETRY=1` somente para diagnostico explicito; planos Family ainda exigem `ANALYSTBLAZE_FAMILY_DETAIL_CONSENT=1` para qualquer detalhe de processo mascarado.

## Autenticacao Desktop

O desktop nao gerencia pagamentos, usuarios ou planos. Ele abre o login do web app em:

```text
{ANALYSTBLAZE_WEB_URL}/login?desktop=1&redirect_uri=analystblaze%3A%2F%2Fauth
```

Depois que o web app validar conta e assinatura, ele deve redirecionar para `analystblaze://auth` com um `token` ou `access_token`, e opcionalmente `refresh_token`, via query string ou fragmento. Sem esse redirect, o usuario fica logado apenas no navegador e o desktop nao recebe a sessao.

Se o usuario ja estiver logado no web app, a tela `/login?desktop=1` pode trocar a sessao web existente por uma sessao desktop em `POST /api/v1/auth/desktop-session` e abrir o deep link sem pedir senha novamente.

Para o desktop exibir a conta, o callback tambem pode enviar campos publicos como `name` ou `username`, `email`, `plan` e `has_paid_plan`. Se esses campos estiverem no JWT, o desktop tenta usa-los apenas como fallback visual. Depois do registro do hardware, o desktop tambem tenta ler o perfil em `GET /api/v1/auth/me`, `/api/v1/me`, `/api/v1/account/me` ou `/api/v1/users/me`, sem bloquear o login caso esses endpoints nao existam.

Campos genericos com o nome do produto, como `AnalystBlaze`, sao ignorados como nome de usuario. A validacao de conta, plano e pagamento continua sendo responsabilidade do web/API.

Exemplo:

```text
analystblaze://auth?token=JWT_DO_USUARIO&username=Ana&plan=pro&has_paid_plan=1
```

Para o plano gratuito, use `starter`:

```text
analystblaze://auth?token=JWT_DO_USUARIO&username=Ana&plan=starter&has_paid_plan=0
```

O desktop usa o token para registrar o hardware na API e guarda as credenciais no cofre do sistema operacional.

No primeiro login de um computador, o desktop envia um fingerprint de hardware para `POST /api/v1/hardware/register`. A API cria o hardware e devolve um `hw_secret` apenas nessa primeira criacao. Se o mesmo computador ja estiver vinculado ao mesmo usuario, a API reutiliza o registro e oculta o segredo. Se o fingerprint ja pertencer a outra conta, a API bloqueia o login desktop com conflito para evitar duplicidade e troca de contas no mesmo PC.

Nao coloque secrets em variaveis `VITE_*`: elas entram no bundle publico.

## Auto-Update

O agente verifica atualizacoes em segundo plano (poucos minutos apos abrir, depois a cada ~8h) e via botao manual em Configuracoes. Ao detectar uma versao nova ele so avisa e baixa em segundo plano; instalar sempre exige clique em "Atualizar agora". O endpoint de manifesto e resolvido a partir da mesma `ANALYSTBLAZE_API_URL` usada pelo resto do agente (nunca duplicado), e todo pacote e verificado com a chave publica em `plugins.updater.pubkey` (`src-tauri/tauri.conf.json`) antes de instalar.

Para gerar chaves, publicar uma release e registrar o manifesto no server, veja [`RELEASING.md`](RELEASING.md).

## Documentacao

- [`RELEASING.md`](RELEASING.md): passo a passo de release, gestao da chave do updater e checklist de sanidade.
- [`docs/README.md`](docs/README.md): indice dos documentos tecnicos.
- [`docs/optimization-safety-matrix.md`](docs/optimization-safety-matrix.md): acoes reversiveis, irreversiveis, sensiveis e bloqueadas.
- [`docs/security-assumptions.md`](docs/security-assumptions.md): premissas de seguranca do desktop.
- [`docs/security-pentest.md`](docs/security-pentest.md): escopo da harness de seguranca.
- [`docs/windows-lab-validation.md`](docs/windows-lab-validation.md): validacao manual em VM descartavel.
- [`docs/adaptive-optimization-manager.md`](docs/adaptive-optimization-manager.md): desenho do gerente de otimizacao adaptativa.
- [`docs/windows-latency-optimizer-wave1.md`](docs/windows-latency-optimizer-wave1.md): plano de otimizacoes de latencia.
- [`docs/remediation-checklist.md`](docs/remediation-checklist.md): checklist de hardening.
