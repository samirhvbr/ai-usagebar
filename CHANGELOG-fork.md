# Changelog do fork — `samirhvbr/ai-usagebar`

Mudanças que **este fork** faz sobre o upstream (`akitaonrails/ai-usagebar`).
Formato [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/).

> O `CHANGELOG.md` da raiz é do **upstream** — não editar lá. As entradas do
> fork vivem aqui, agrupadas pela versão do `VERSION` (`<base>+fork.<N>`).
> Convenção completa em [`docs/FORK.md`](docs/FORK.md).

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
