# AnalystBlaze Desktop

Aplicativo desktop Tauri com front-end React/Vite adaptado do `analystblaze-core`.

## Comandos

```bash
npm install
npm run dev
npm run tauri dev
npm run build
```

Em maquinas com Smart App Control ativo, `npm run tauri dev` passa pelo wrapper `scripts/tauri-sac-router.ps1`, que reutiliza os scripts de assinatura local ja existentes.

## Ambiente

Copie `.env.example` quando precisar configurar endpoints:

- `VITE_ANALYSTBLAZE_TELEMETRY_URL`: endpoint opcional para lotes de telemetria de interface.
- `VITE_ANALYSTBLAZE_INSIGHTS_URL`: endpoint opcional para insights.
- `ANALYSTBLAZE_API_URL` e `ANALYSTBLAZE_WEB_URL`: usados pelo agente Rust/Tauri em runtime.

Sem endpoint de telemetria, os eventos ficam salvos localmente e limitados pelo tamanho maximo configurado.

## Autenticacao desktop

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
