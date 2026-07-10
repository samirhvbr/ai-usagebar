# Convenção do fork — `samirhvbr/ai-usagebar`

Como este fork se organiza sobre o upstream (`akitaonrails/ai-usagebar`).
Objetivo: você sempre saber **o que está rodando** e **o que é nosso vs do akita**.

## Versionamento

Fonte da verdade: o arquivo **`VERSION`** na raiz, no formato:

```
<base-upstream>+fork.<N>
```

- **base-upstream** — a versão do akita em que estamos sincronizados (hoje `0.7.2`).
- **N** — a iteração do nosso fork sobre essa base. Reinicia em `1` quando
  sincronizamos uma base nova (ex.: ao subir pra `0.8.0`, vira `0.8.0+fork.1`).

Exemplo atual: `0.7.2+fork.1`.

> **Não** bumpar o `version` do `Cargo.toml`: ele espelha o upstream, e o
> release/AUR do akita depende disso (ver a "Release checklist" no `CLAUDE.md`).
> Por isso `ai-usagebar --version` mostra a **base** (0.7.2); o `VERSION` mostra
> a iteração do fork. Pra saber o que roda: `cat VERSION`.

## Ritual de push (organiza os pushs)

A cada mudança relevante que for pro remoto:

1. Fazer as mudanças.
2. **Bump `VERSION`** (N+1).
3. Adicionar a entrada em **`CHANGELOG-fork.md`** sob a nova versão
   (Added / Changed / Fixed).
4. Se mexeu no Rust: `cargo test && cargo clippy --all-targets -- -D warnings`.
   Se mexeu no GNOME: `node --check gnome-extension/extension.js`.
5. Commit + push na branch de trabalho.

## Mapa de arquivos — o que é nosso vs upstream

| Categoria | Arquivos | Vai pro upstream? |
|---|---|---|
| **Fork-only** (nunca sobe pro akita) | `src/shvia/`, `.claude/`, `VERSION`, `CHANGELOG-fork.md`, `docs/FORK.md`, `docs/UPSTREAM_PLAN.md` | ❌ Não |
| **Candidato a upstream** | `macos/`, `gnome-extension/`, `windows-tray/`, fix de re-login UX (widget/TUI) | ✅ Via PRs — ver `docs/UPSTREAM_PLAN.md` |
| **Upstream puro** (só mexer via sync) | `CHANGELOG.md`, `Cargo.toml`, `packaging/`, núcleo em `src/` | ⬆️ Vem do akita |

## Sincronizar com o upstream

```bash
git remote add upstream https://github.com/akitaonrails/ai-usagebar.git   # 1x
git fetch upstream
git merge upstream/main        # ou cherry-pick de commits específicos
```

Depois: atualizar a **base** no `VERSION` e resetar `N` pra `1`, registrar no
`CHANGELOG-fork.md`, rodar o gate, commit + push.

## Propor mudanças nossas pro upstream

Não mandar um PR gigante. Fatiar por app/feature, começando pelo bugfix pequeno.
Passo a passo, checklist de limpeza por PR (URLs do fork, PT→EN, licença do
Windows tray) e o que fica só no fork: **`docs/UPSTREAM_PLAN.md`**.
