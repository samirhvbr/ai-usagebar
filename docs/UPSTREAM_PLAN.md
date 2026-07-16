# Plano de upstreaming — fork `samirhvbr/ai-usagebar` → `akitaonrails/ai-usagebar`

> Documento de organização do fork antes de propor PRs ao upstream.
> Gerado a partir da revisão completa de 2026-07-09 (código, histórico,
> licenciamento e varredura de conteúdo fork-specific).

## Estado do fork após a organização de 2026-07-09

A branch de trabalho agora contém, nesta ordem:

- **Base upstream v0.7.2** (merge de `ad0dd54`) — inclui o fix do
  `refreshToken` vazio (PR #11 do upstream, Leonardo Godoy): Claude Code
  ≥ 2.1.x no macOS deixa `refreshToken` vazio no Keychain; a v0.7.1
  postava esse vazio no token endpoint, recebia HTTP 400 e congelava o
  widget num snapshot velho. **Era exatamente o bug observado no MacBook.**
  A v0.7.2 pula o refresh quando não há refresh token (o Claude Code
  rotaciona o access token sozinho) e envia o `User-Agent: claude-code/…`
  que o endpoint de usage exige.
- **Audit fixes recuperados** das branches `feat/` (estavam órfãos, fora do
  `main`): `c63ce4c` (macOS: plist gerado pelo `install-agent.sh` com
  `xml_escape`, sem `KeepAlive` — Cmd-Q não ressuscita mais o app; template
  `.plist` deletado) e `2cd886c` (GNOME: correções de auditoria).
- **Nossas features** (autoria Samir, 23/06–01/07): app de menu bar macOS,
  extensão GNOME Shell, tray Windows (vendorizado de EaeDave), UX de
  re-login em widget/TUI/macOS, vendor ShvIA (fork-only) e config `.claude/`
  (fork-only).

Gate: `cargo test` verde, `clippy -D warnings` limpo. (`cargo machete`
indisponível no container — rodar localmente antes de release.)

## O que vai para o upstream (fatiado, nesta ordem)

### PR 1 — UX de re-login no widget + TUI (menor risco, maior valor)
Conteúdo: `660f63b` (TUI) + `c797a47` (widget) + `is_reauth_error` em
`src/anthropic/mod.rs`.
Racional: na versão publicada, um `invalid_grant` real (refresh token
expirado/rotacionado — cenário Linux/arquivo) congela o widget sem dizer o
porquê. O PR adiciona o aviso "Sign-in expired — run `claude`" no tooltip,
TUI e classe crítica na barra.
Nota de compatibilidade com a v0.7.2: são complementares — a v0.7.2 trata
`refreshToken` **vazio** (transient, silencioso, correto); nosso aviso trata
refresh token **inválido/expirado** (exige ação humana). Os testes cobrem
ambos.
Checklist antes de abrir:
- [ ] Rebase sobre o `main` do upstream (pós-v0.7.2).
- [ ] Entrada no `CHANGELOG.md` em `[Unreleased]` (Added/Fixed).
- [ ] Sem bump de versão (o mantenedor corta releases).

### PR 2 — App de menu bar macOS (`macos/`)
Conteúdo: squash de `c3e8f26` + `c5d9923` + `e5d556a` + `7f108d1` +
`3527572` + `3ef6567` + audit fixes `c63ce4c` + **re-login 1-clique no
dropdown** (item que aparece só no estado de sessão expirada e dispara o
login do vendor no Terminal; reusa `oauthScript`/`runInTerminal`).
Antes era necessário (achados da revisão):
- [ ] `macos/INSTALL.md:19` — trocar clone URL `samirhvbr` → `akitaonrails`.
- [ ] Traduzir strings de UI PT-BR → EN (`Preferências…`, `Sair`,
  `Atualizar agora`, `Sessão expirada…`, prompts do script de login em
  `oauthScript`, seção Vendors). Upstream é 100% EN.
- [ ] Depende do PR 1 (o app lê o marcador `re-login` do widget) — abrir
  depois que o PR 1 for aceito, ou incluir a dependência na descrição.

### PR 3 — Extensão GNOME Shell (`gnome-extension/`)
Conteúdo: squash de `fd374e9` (inicial — reescrever a mensagem "Auto-Commit")
+ `368ed36` + `ca86a15` + audit fixes `2cd886c`.
Pontos positivos da revisão: zero referências ao fork, zero shvia, zero
segredos; `metadata.json` (uuid/url) **já aponta para akitaonrails** —
pronto para upstream.
Antes era necessário:
- [ ] Traduzir UI PT-BR → EN (`prefs.js` inteiro + menu/erros do
  `extension.js`; inventário completo na revisão de 2026-07-09).
- [ ] Headers de licença nos `.js` (extensões GNOME convencionam GPL-2.0+;
  repo é MIT — decidir com o mantenedor qual licença a extensão carrega e
  registrar no PR).
- [ ] Typo `nao encontrado` → `não encontrado` (irrelevante se traduzir).

### PR 4 — Tray Windows (`windows-tray/`) — por último
Conteúdo: `1453884` + `d05424e` + `9519d55`.
Bloqueador real (provenance): o código é vendorizado de
[EaeDave/ai-usagebar](https://github.com/EaeDave/ai-usagebar) (MIT). O
crédito existe só em prosa (README/TESTING). Antes de upstreamar:
- [ ] Adicionar `windows-tray/LICENSE` com o MIT + copyright do EaeDave
  (MIT exige que o aviso de copyright acompanhe o código).
- [ ] `<Authors>/<Copyright>` no `AiUsagebarTray.csproj`.
- [ ] `TESTING.md:36` — clone URL `samirhvbr` → `akitaonrails`; remover o
  framing "this fork".
- [ ] `README.md:93` — trocar path de exemplo com username pessoal
  (`C:\Users\David\…`) por genérico.
- [ ] `RELEASE_NOTES.md` — é artefato do CI do fork (URLs de screenshot
  apontam pro repo do EaeDave); deixar fora do PR.
- [ ] Nits: versão `app.manifest` 0.1.0.0 vs csproj 0.1.1; `using`s não
  usados em `UsageData.cs:1-2`.

### Antes dos PRs 2–4: abrir uma issue/discussion
Os três apps expandem o escopo do projeto (Waybar/Linux → desktop
multi-plataforma) e a carga de manutenção. Perguntar ao mantenedor se quer
os apps no repo principal antes de abrir os PRs grandes. Precedente a favor:
ele já mergeou suporte Windows da comunidade (PR #8) e o fix do PR #11.
O PR 1 (bugfix) pode ir direto, sem issue.

## O que NUNCA vai para o upstream (fork-only)

- `src/shvia/` + integrações (vendor de gateway self-hosted pessoal) —
  `2f1970b`.
- `.claude/` (perfis/settings do Claude Code) — `d0277d9`, `4fb1d2f`.
- Seção "git pull first" do `CLAUDE.md` — `738d81c`.
- `DESKTOP.md`: útil, mas reescrever URLs (`samirhvbr` em `DESKTOP.md:30`)
  se for junto dos PRs 2–4.

## Mecânica de cada PR (cross-fork)

1. Criar branch a partir do `main` do **upstream** (não do fork):
   `git fetch upstream && git checkout -b feat/<nome> upstream/main`.
2. Cherry-pick/squash dos commits listados, aplicar o checklist do PR.
3. `cargo test && cargo clippy --all-targets -- -D warnings && cargo machete`.
4. Push para o fork, abrir PR `samirhvbr:feat/<nome>` → `akitaonrails:main`,
   citando este plano e, nos PRs 2–4, a issue de escopo.

## Pendência na máquina (MacBook do Samir)

O fix do congelamento (v0.7.2) e o do LaunchAgent (audit) agora estão nesta
branch — reinstalar a partir dela:

```bash
cd ~/ai-usagebar && git fetch origin && git checkout claude/ia-usagebar-restart-issue-4m24g8 && git pull
cargo install --path . --force        # binário 0.7.2 + fixes
cd macos && ./build.sh                # recompila o app da menu bar
pkill -f ai-usagebar-menubar 2>/dev/null
./install-agent.sh                    # regrava o plist (novo formato) e carrega
```
