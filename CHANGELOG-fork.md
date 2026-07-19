# Changelog do fork — `samirhvbr/ai-usagebar`

Mudanças que **este fork** faz sobre o upstream (`akitaonrails/ai-usagebar`).
Formato [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).

> O `CHANGELOG.md` da raiz é do **upstream** — não editar lá. As entradas do
> fork vivem aqui, agrupadas pela versão do `VERSION` (`<base>+fork.<N>`).
> Convenção completa em [`docs/FORK.md`](docs/FORK.md).

## [0.12.0+fork.9] — 2026-07-19

Base upstream: **v0.12.0**.

### Added
- **Vendor Anthropic (API)** — controle financeiro da conta de **API/Console**
  da Anthropic, distinta da conta do **Claude Code** (que é OAuth e já existe
  como vendor `anthropic`). Complementa a família de vendors de saldo do fork
  (Kilo/Novita/Moonshot/Grok).
  - Mostra o **gasto do mês** (month-to-date) via a Admin API
    `GET /v1/organizations/cost_report`, opcionalmente contra um
    `monthly_limit` configurável → barra `$1.34 / $1000 · 0%`; sem limite,
    mostra `$1.34/mo`.
  - Autentica com uma **Admin key** do Console (`sk-ant-admin01-…`, header
    `x-api-key`) — diferente da API key de inferência e do login OAuth.
    Inserível pelo **Settings do TUI** ou `[anthropic_api] api_key`.
  - O `amount` do cost_report vem em **centavos** (decimal string, confirmado
    nos docs oficiais); convertido p/ dólares (÷100). Paginação seguida via
    `has_more`/`next_page`.
  - Ao contrário dos outros vendors de saldo (que mostram o **restante**), o
    **saldo de créditos pré-pagos NÃO é exposto por API** (só no dashboard do
    Console); por isso este reporta o **consumido** (gasto do mês), não o saldo.
  - A Admin key só existe para contas de **organização**: é preciso configurar
    uma org antes (Console → Settings → Organization); `.../settings/admin-keys`
    dá 404 em contas individuais. Documentado no `config.example.toml`; o tooltip
    dá a dica em erros 401/403.
  - **macOS:** linha **Anthropic (API)** logo abaixo de **Anthropic (Claude)**
    no menu; opt-in (só aparece com key ativa). Reusa o último fetch do cache.

## [0.12.0+fork.8] — 2026-07-17

Base upstream: **v0.12.0** (sincronizado com `upstream/main` @ PR #22).

Sync do upstream via `git merge upstream/main --no-ff`. O Akita mergeou os
**nossos** PRs #20 (`feat/scoped-desktop-surfaces`) e #21
(`feat/desktop-meta-marker`), então a versão refinada desse trabalho volta
pelo upstream; o merge concilia isso com os deltas fork-only.

### Added
- **Vendor Kimi Code** (upstream, PR #22 do `pvpmartins`): quota semanal + janela
  5h rolling. Registrado em todo o core (enum `VendorId::Kimi`, `KimiConfig`,
  `VendorSnapshot::Kimi`, TUI, widget, `config.example.toml`) **ao lado** do nosso
  vendor fork-only `shvia` — os dois coexistem; `shvia` continua por último na
  ordem canônica e habilitado por default.
- **Marcadores de meta/pace nas barras desktop** (nosso PR #21, via upstream):
  campos `{*_elapsed}` + sentinela `__aiub_end__` no FORMAT do GNOME/macOS.

### Changed
- **Desktop (GNOME + macOS) convergido para a versão refinada do upstream** dos
  nossos PRs #20/#21 (barras Fable model-scoped com fallback sonnet correto +
  markers de pace), **preservando os deltas fork-only**: header de versão do fork
  no dropdown e sinais de saúde `stale`/`re-login`.
- **FORMAT do GNOME/macOS reindexado**: `{version}` passou para o índice **16**
  (logo antes do sentinela `__aiub_end__`, agora 17), pois os campos
  `{*_elapsed}` do upstream ocupam 13–15. `gnome-extension/marker-logic.js`
  (`FORMAT`/`FIELD`) e seu teste de tabela atualizados; o binário (`render.rs`,
  do upstream) já emite `{version}` **e** os campos de elapsed.

### Notes
- Validado no clone: `cargo test` (361 ok), `cargo clippy --all-targets -D
  warnings` (limpo), `node --check` + `marker-logic.test.mjs` (ok). O app **macOS
  não pôde ser compilado no ambiente do sync** (sem `swiftc`) — merge conciliado
  por revisão de código; convém um build/smoke real do menu bar antes de confiar.

## [0.12.0+fork.7] — 2026-07-10

Base upstream: **v0.12.0**.

### Fixed
- **macOS: mensagem de erro acionável para o token vazio do "trusted-device".**
  Quando o arquivo não existe **e** o item do Keychain está com `accessToken`
  vazio (estado "trusted-device" do Claude Code recente), o widget mostrava
  `I/O error at ~/.claude/.credentials.json / No such file` — enganoso, porque o
  problema real é o token vazio, não o arquivo. Agora o `creds::read_default_with`
  detecta esse caso específico (arquivo ausente/ilegível + Keychain presente mas
  vazio) e devolve uma mensagem **acionável, em inglês** (candidata a PR pro
  upstream — atinge qualquer usuário macOS do akita): um `/login` simples vê "já
  logado" e não regrava o token; o conserto é `claude` → `/logout` → `/login`.
  Descoberto no MacBook do dev — forçar logout+login repopulou o token e a barra
  (Session / Weekly / **Fable** / Extra) voltou a funcionar no Mac, igual ao
  Linux. Teste hermético cobre o caso; contrato do arquivo presente-mas-inválido
  fica intacto.

### Changed
- **`install.sh`: health-check aponta o conserto `/logout` + `/login`.** A
  mensagem do token vazio (fork.6) agora, além de identificar o estado
  "trusted-device", diz exatamente como resolver — em vez de tratá-lo como
  irreversível.

## [0.12.0+fork.6] — 2026-07-10

Base upstream: **v0.12.0**.

### Changed
- **`install.sh`: health-check de credenciais no macOS agora detecta o token
  VAZIO do "trusted-device flow".** O check do fork.5 só via se o item do
  Keychain existia (sem `-w`) — mas o Claude Code recente no macOS mantém o item
  `Claude Code-credentials` e grava `accessToken:""` / `refreshToken:""`
  (trusted-device flow). Item presente + token vazio dava um "✓ credenciais
  achadas" enganoso enquanto a barra ficava vazia. Agora o check lê o valor
  (`-w`) e distingue os três casos: token presente (OK), item presente mas token
  vazio (avisa que é o trusted-device flow e que não há token pro menu bar ler —
  por isso o Linux funciona e o Mac não), ou nada (pede `claude` / `/login`). O
  token fica só numa variável local e nunca é impresso. O `-w` não adiciona
  prompt de Keychain que o próprio app da menu bar já não dispare (ele lê com
  `-w` também).

## [0.12.0+fork.5] — 2026-07-10

Base upstream: **v0.12.0**.

### Fixed
- **macOS: leitura do Keychain robusta a conta divergente.** `keychain::read_raw`
  filtrava por `-a $USER`; se o item `Claude Code-credentials` foi gravado com
  outra conta (varia entre builds/logins do Claude Code), o `security` não achava
  → o widget mostrava um "arquivo ~/.claude/.credentials.json não existe" enganoso
  e a barra ficava vazia. Agora tenta com a conta e **cai pra busca só por
  serviço** (normalmente há um único item). *Candidato a PR pro upstream — o bug
  é da árvore dele.*

### Added
- **`install.sh`: health-check de credenciais no macOS.** Depois de instalar,
  confere se as credenciais do Claude existem (Keychain ou arquivo) e, se não,
  avisa na hora pra rodar `claude` / `/login` — em vez de deixar a barra vazia
  sem explicação. Checa só a existência do item (sem `-w`), então não dispara
  prompt de Keychain.

## [0.12.0+fork.4] — 2026-07-10

Base upstream: **v0.12.0**.

### Fixed
- **macOS: "NSMenuItem" no dropdown.** Quando o binário não devolvia um snapshot
  (rate-limit / loading / erro), o app deixava as linhas do dropdown vazias e o
  macOS as renderizava como "NSMenuItem". Agora as linhas nascem escondidas e
  ficam escondidas até chegar dado de verdade; o header mostra "Carregando…" ou
  a mensagem do binário nesse meio-tempo.

### Changed
- **Instaladores testam o git de verdade.** `install.sh` e `install.ps1` agora
  checam `git` no PATH, remote configurado e branch/remote, e — se o `git pull`
  falhar — apontam o provável culpado (remote/URL, rede/auth, conflito local) em
  vez de seguir mudo. `install.ps1` também confere `$LASTEXITCODE` de cada
  ferramenta nativa (cargo/dotnet) pra falhar claro.

## [0.12.0+fork.3] — 2026-07-10

Base upstream: **v0.12.0**.

### Added
- **`install.sh` na raiz** — instalador único (macOS + Linux/GNOME): detecta o
  SO e faz tudo (git pull → `cargo install` → build do app da plataforma →
  instalar → habilitar no login/reboot). macOS via LaunchAgent
  (`macos/install-agent.sh`), Linux via `gnome-extensions enable`. O corpo
  fica em `main()` pra um `git pull` que atualize o próprio script não
  emendar linhas velhas/novas no meio da execução.
- **`install.ps1` na raiz** — o mesmo pro **Windows** (PowerShell): git pull →
  `cargo install` → `dotnet publish -c Release` (tray self-contained, embute o
  backend do lado) → copia pra `%LOCALAPPDATA%\Programs\ai-usagebar-tray` →
  registra auto-start no `HKCU\…\Run` (valor `AiUsagebarTray`, o mesmo do toggle
  do app) → lança. Rodar: `powershell -ExecutionPolicy Bypass -File install.ps1`.

## [0.12.0+fork.2] — 2026-07-10

Base upstream: **v0.12.0**.

### Added
- **Versão do fork visível no header** dos apps de desktop. Novo placeholder
  `{version}` no binário (embute o arquivo `VERSION` em tempo de compilação),
  e GNOME/macOS/Windows mostram `vX.Y.Z+fork.N` (dim) ao lado do plano
  ("Max 20x") no dropdown — pra sempre saber o que está rodando sem `--version`.

## [0.12.0+fork.1] — 2026-07-10

Base upstream: **v0.12.0** (sync completo v0.7.2 → v0.12.0 via
`sync/upstream-0.8.0`). O upstream trouxe: desktop integrations (o merge do
akita dos nossos apps macOS/GNOME), multi-conta Anthropic (`--account` +
`[[anthropic.accounts]]`), abas por conta no TUI, e os **limites semanais por
modelo (Fable)** do PR #19. Nossos deltas preservados no merge: backoff de
429, re-login 1-clique (macOS), sinais ⏸/re-login (GNOME), vendor ShvIA,
docs do fork.

### Added
- **Barra "Fable" no lugar da "Sonnet only"** nos três apps de desktop. Novos
  placeholders `{scoped_label}` / `{scoped_pct}` / `{scoped_reset}` /
  `{scoped_bar}` expõem a 1ª janela por-modelo da API (`limits[]` — hoje
  "Fable"); GNOME, macOS e Windows preferem essa janela e caem pro Sonnet
  antigo se ela não vier. Nome da linha é dinâmico (o que a API mandar).
- **macOS: badge ⏸ de desatualizado** (paridade com o GNOME) — ⏸ vermelho na
  menu bar + "⏸ Desatualizado — sem conexão com a conta" no dropdown quando o
  binário serve cache velho; antes o app engolia o marcador e dados congelados
  pareciam saudáveis.

### Fixed
- Testes nossos adaptados às APIs novas do upstream (assinatura `CredsTarget`
  no fetch, campo `scoped` no snapshot, fixture de tabs do TUI desliga o
  vendor fork-only shvia).

## [0.7.2+fork.3] — 2026-07-10

### Added
- `scripts/sync-upstream.sh` — checa se o upstream (akita) tem releases novos,
  mostra o gap de versão e lista os nossos deltas a preservar num merge. Evita
  a surpresa de descobrir tarde que o upstream avançou (na 1ª rodada ele já
  estava no v0.12.0). `docs/FORK.md` aponta pra ele.

## [0.7.2+fork.2] — 2026-07-10

Base upstream: **v0.7.2** (o sync pro v0.8.0 ficou pendente — o ambiente
bloqueou puxar o repo do upstream; dá pra fazer na máquina do dev depois. O
v0.8.0 **não** corrige o 429, então não é bloqueador).

### Fixed
- **Backoff de rate-limit (HTTP 429).** O widget não recuava depois de um 429 —
  reintentava a cada poll (~30s), o que mantinha o rate-limit do endpoint de uso
  **saturado** e ele nunca recuperava (era a causa real do "7d 0%" preso no
  Linux). Agora um 429 arma um backoff (~5 min, arquivo `.backoff` no cache):
  durante a janela serve o cache **sem tocar na rede**, e o `.backoff` é limpo no
  primeiro fetch bem-sucedido. Regressão testada (o 2º fetch dentro da janela não
  chama a rede).

## [0.7.2+fork.1] — 2026-07-10

Base upstream: **v0.7.2**.

### Added
- **macOS:** re-login em 1 clique no dropdown da menu bar quando a sessão
  expira (roda o login do vendor no Terminal e re-checa em 5s).
- **GNOME:** o painel agora surfa os sinais de saúde do binário — antes engolidos:
  `⏸` = desatualizado ("sem conexão com a conta"); `re-login` = sessão expirada
  (estado "⚠ login" em vez de mostrar números velhos).
- `docs/UPSTREAM_PLAN.md` — plano fatiado de PRs pro upstream.
- Convenção do fork: `VERSION`, este changelog e `docs/FORK.md`.

### Fixed
- **Congelamento do widget em dado velho** — via sync do upstream **v0.7.2**:
  pula o refresh quando `refreshToken` está vazio (Claude Code ≥ 2.1.x), evitando
  o HTTP 400 que zerava o snapshot.

### Changed
- Recuperados audit fixes órfãos das branches `feat/`: macOS `install-agent.sh`
  gera o plist sem `KeepAlive` (⌘Q não ressuscita o app; `RunAtLoad` mantém a
  volta no login); correções de auditoria da extensão GNOME.
