#!/usr/bin/env bash
#
# One-shot installer for ai-usagebar (macOS + Linux/GNOME). Detects the OS and:
#   git pull → cargo install (binary) → build + install the desktop app →
#   enable it to start on login/reboot.
#   Windows: use install.ps1 instead.
#
# Usage:  ./install.sh              (from anywhere; cd's to its own dir)
#         SKIP_PULL=1 ./install.sh  (don't touch git)
#
# Body lives in main() so a `git pull` that rewrites this very file mid-run
# can't splice old/new lines (bash parses the functions up front).

set -euo pipefail

# Update the checkout — best-effort, with diagnostics when the pull fails.
# The pull is the flakiest step (missing/wrong remote, auth, or no git at all),
# so we test each precondition and, on failure, point at the likely culprit
# instead of silently moving on.
update_repo() {
    if [ "${SKIP_PULL:-0}" = "1" ]; then
        echo "› git pull pulado (SKIP_PULL=1)."; return 0
    fi
    if ! command -v git >/dev/null 2>&1; then
        echo "⚠ git não está no PATH — pulando o update. Instale o git, ou rode com SKIP_PULL=1."; return 0
    fi
    if [ ! -d .git ]; then
        echo "⚠ isto não é um checkout git — pulando o update."; return 0
    fi
    if [ -z "$(git remote 2>/dev/null)" ]; then
        echo "⚠ nenhum remote configurado — não dá pra atualizar."
        echo "    conserto:  git remote add origin <URL-do-fork>"
        return 0
    fi

    local branch remote url
    branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo HEAD)"
    remote="$(git config "branch.${branch}.remote" 2>/dev/null || echo origin)"
    url="$(git remote get-url "$remote" 2>/dev/null || echo '?')"

    echo "› git pull (${remote}/${branch})…"
    if git pull --ff-only "$remote" "$branch"; then
        echo "  ✓ atualizado."
    else
        echo "  ⚠ git pull FALHOU — seguindo com o que já está no disco. Cheque:" >&2
        echo "     • remote   → ${remote}: ${url}          (git remote -v)" >&2
        echo "     • alcança? → git ls-remote ${remote}    (rede / auth / URL errada)" >&2
        echo "     • local    → git status                 (mudança não commitada / conflito)" >&2
    fi
    return 0
}

main() {
    local dir os uuid
    dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    cd "$dir"

    command -v cargo >/dev/null 2>&1 || {
        echo "✗ cargo/rust não encontrado. Instale em https://rustup.rs e rode de novo." >&2
        exit 1
    }

    update_repo

    echo "› cargo install (binário)…"
    cargo install --path . --force

    os="$(uname -s)"
    case "$os" in
        Darwin)
            echo "› macOS detectado — app da menu bar"
            ( cd macos && ./build.sh )
            pkill -f ai-usagebar-menubar 2>/dev/null || true   # derruba cópia antiga
            ( cd macos && ./install-agent.sh )                 # LaunchAgent: sobe no login
            echo "✓ macOS pronto — a barra sobe sozinha no login."
            ;;
        Linux)
            echo "› Linux detectado — extensão do GNOME Shell"
            ( cd gnome-extension && ./install.sh )
            uuid="ai-usagebar@akitaonrails.github.io"
            if command -v gnome-extensions >/dev/null 2>&1; then
                gnome-extensions enable "$uuid" 2>/dev/null \
                    && echo "✓ Extensão habilitada — carrega em todo login." \
                    || echo "⚠ Não consegui habilitar via CLI; ative: gnome-extensions enable $uuid"
            else
                echo "⚠ 'gnome-extensions' não encontrado — habilite após recarregar o Shell."
            fi
            echo
            echo "→ Recarregue o GNOME Shell pra ver agora:"
            echo "    X11     → Alt+F2, digite 'r', Enter"
            echo "    Wayland → logout/login"
            ;;
        *)
            echo "✗ SO não suportado por este script: $os (só macOS e Linux/GNOME)." >&2
            echo "  Windows: rode  install.ps1  →  powershell -ExecutionPolicy Bypass -File install.ps1" >&2
            exit 1
            ;;
    esac

    echo
    echo "✓ Concluído — versão instalada: $(tr -d '\n' < VERSION 2>/dev/null || echo '?')"
}

main "$@"
