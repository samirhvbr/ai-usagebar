#!/usr/bin/env bash
#
# One-shot installer for ai-usagebar. Detects the OS and does the whole dance:
#   git pull → cargo install (binary) → build + install the desktop app for
#   this platform → enable it to start on login/reboot.
#
#   macOS         → menu bar app (LaunchAgent, RunAtLoad)
#   Linux (GNOME) → GNOME Shell extension (enabled = loads every login)
#
# Usage:  ./install.sh          (from anywhere; it cd's to its own dir)
#         SKIP_PULL=1 ./install.sh   (don't touch git)
#
# The whole body lives in main() so a `git pull` that updates this very file
# mid-run can't splice old/new lines together (bash parses main() up front).

set -euo pipefail

main() {
    local dir os uuid
    dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    cd "$dir"

    command -v cargo >/dev/null 2>&1 || {
        echo "✗ cargo/rust não encontrado. Instale em https://rustup.rs e rode de novo." >&2
        exit 1
    }

    # 1. Update the checkout (best-effort; a dirty tree shouldn't abort the install).
    if [ "${SKIP_PULL:-0}" != "1" ] && [ -d .git ]; then
        echo "› git pull…"
        git pull --ff-only || echo "  ⚠ git pull pulou (resolva à mão se precisar); seguindo com o que está no disco."
    fi

    # 2. Build + install the binaries (ai-usagebar + ai-usagebar-tui → ~/.cargo/bin).
    echo "› cargo install (binário)…"
    cargo install --path . --force

    os="$(uname -s)"
    case "$os" in
        Darwin)
            echo "› macOS detectado — app da menu bar"
            ( cd macos && ./build.sh )
            pkill -f ai-usagebar-menubar 2>/dev/null || true   # derruba cópia manual antiga
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
                    || echo "⚠ Não consegui habilitar via CLI; ative em: gnome-extensions enable $uuid"
            else
                echo "⚠ 'gnome-extensions' não encontrado — habilite manualmente depois de recarregar o Shell."
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
