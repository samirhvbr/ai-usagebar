#!/usr/bin/env bash
# Checa se o upstream (akitaonrails/ai-usagebar) tem novidades e guia o sync.
#
# Não muda nada: só busca, compara versões e lista o que é do upstream vs o que
# é nosso (pra preservar no merge). O merge em si é manual — ver docs/FORK.md.
#
# Uso:  ./scripts/sync-upstream.sh
set -euo pipefail

UPSTREAM_URL="https://github.com/akitaonrails/ai-usagebar.git"
REMOTE="upstream"
BRANCH="main"

cd "$(git rev-parse --show-toplevel)"

# 1. Garante o remote do upstream.
if ! git remote get-url "$REMOTE" >/dev/null 2>&1; then
  echo "→ adicionando remote '$REMOTE' ($UPSTREAM_URL)"
  git remote add "$REMOTE" "$UPSTREAM_URL"
fi

echo "→ buscando $REMOTE…"
git fetch --quiet --tags "$REMOTE"

ver() { grep -m1 '^version' "$1" 2>/dev/null | sed 's/.*"\(.*\)".*/\1/'; }
FORK_VER=$(cat VERSION 2>/dev/null || echo '?')
BASE_VER=$(ver Cargo.toml)
UP_VER=$(git show "$REMOTE/$BRANCH:Cargo.toml" | ver /dev/stdin)
UP_TAG=$(git describe --tags --abbrev=0 "$REMOTE/$BRANCH" 2>/dev/null || echo "$UP_VER")

echo
echo "  fork (VERSION):    $FORK_VER   (base Cargo.toml: $BASE_VER)"
echo "  upstream:          $UP_VER   (tag: $UP_TAG)"
echo

MB=$(git merge-base HEAD "$REMOTE/$BRANCH")
BEHIND=$(git rev-list --count "$MB..$REMOTE/$BRANCH")

if [ "$BEHIND" -eq 0 ]; then
  echo "✓ Em dia com o upstream. Nada a fazer."
  exit 0
fi

echo "⚠ $BEHIND commits novos no upstream. Releases desde a nossa base:"
git log --oneline "$MB..$REMOTE/$BRANCH" | grep -iE 'v[0-9]+\.[0-9]+\.[0-9]+' | head -20 | sed 's/^/    /'
echo
echo "Nossos deltas (não estão no upstream — PRESERVAR ao mergear):"
git log --oneline --no-merges "$REMOTE/$BRANCH..HEAD" | head -30 | sed 's/^/    /'
echo
echo "Pra sincronizar (num working tree limpo — ver docs/FORK.md):"
echo "    git merge $REMOTE/$BRANCH --no-commit --no-ff"
echo "    # resolver conflitos preservando os deltas acima"
echo "    cargo test && cargo clippy --all-targets -- -D warnings"
echo "    # bump VERSION -> ${UP_VER}+fork.1, entrada no CHANGELOG-fork.md, commit"
