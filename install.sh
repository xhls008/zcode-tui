#!/usr/bin/env bash
# Build and install zcode-tui plus the `zcode` CLI wrapper.
#
#   ./install.sh                 build + install binary and wrapper
#   ./install.sh --prefix DIR    install under DIR/bin (default ~/.local)
#   ./install.sh --no-wrapper    install only the zcode-tui binary
#   ./install.sh --uninstall     remove installed binary and managed wrapper
#
# Re-run after `git pull` or local changes to update the installation.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${HOME}/.local"
INSTALL_WRAPPER=true
UNINSTALL=false
MARKER="managed by zcode-tui install.sh"

usage() {
    sed -n '2,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# Locate the ZCode desktop app dir holding resources/glm/zcode.cjs. Checked:
# $ZCODE_APP, /opt/ZCode (deb install), ~/.local/opt/zcode/<ver>/opt/ZCode.
detect_app_dir() {
    if [ -n "${ZCODE_APP:-}" ] && [ -f "${ZCODE_APP}/resources/glm/zcode.cjs" ]; then
        printf '%s\n' "$ZCODE_APP"
        return 0
    fi
    if [ -f /opt/ZCode/resources/glm/zcode.cjs ]; then
        printf '%s\n' /opt/ZCode
        return 0
    fi
    local dir latest=""
    for dir in "$HOME"/.local/opt/zcode/*/opt/ZCode; do
        [ -f "${dir}/resources/glm/zcode.cjs" ] && latest="$dir"
    done
    if [ -n "$latest" ]; then
        printf '%s\n' "$latest"
        return 0
    fi
    return 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            PREFIX="$2"
            shift
            ;;
        --prefix=*) PREFIX="${1#*=}" ;;
        --no-wrapper) INSTALL_WRAPPER=false ;;
        --uninstall) UNINSTALL=true ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
    shift
done

BIN_DIR="${PREFIX}/bin"
TUI_BIN="${BIN_DIR}/zcode-tui"
WRAPPER="${BIN_DIR}/zcode"

if $UNINSTALL; then
    if [ -f "$TUI_BIN" ]; then
        rm -f "$TUI_BIN"
        echo "removed ${TUI_BIN}"
    fi
    if [ -f "$WRAPPER" ] && grep -q "$MARKER" "$WRAPPER"; then
        rm -f "$WRAPPER"
        echo "removed ${WRAPPER}"
    elif [ -f "$WRAPPER" ]; then
        echo "kept ${WRAPPER} (not managed by this script)"
    fi
    exit 0
fi

command -v cargo >/dev/null 2>&1 || {
    echo "error: cargo not found; install Rust from https://rustup.rs" >&2
    exit 1
}

echo "building release binary..."
cargo build --release --quiet --manifest-path "${REPO_DIR}/Cargo.toml"

install -Dm755 "${REPO_DIR}/target/release/zcode-tui" "$TUI_BIN"
echo "installed ${TUI_BIN} ($("$TUI_BIN" --version))"

APP_DIR="$(detect_app_dir || true)"
if [ -n "$APP_DIR" ]; then
    echo "found ZCode CLI kernel: ${APP_DIR}/resources/glm/zcode.cjs"
else
    echo "note: no ZCode CLI kernel found (checked \$ZCODE_APP, /opt/ZCode,"
    echo "      ~/.local/opt/zcode/*/opt/ZCode); zcode --prompt will not work"
    echo "      until the desktop package is installed or ZCODE_APP is set."
fi

if $INSTALL_WRAPPER; then
    if [ -f "$WRAPPER" ] && ! grep -q "$MARKER" "$WRAPPER"; then
        if [ -z "$APP_DIR" ]; then
            echo "keeping existing unmanaged ${WRAPPER} untouched: it may point at a"
            echo "working ZCode install this script could not find. Set ZCODE_APP=..."
            echo "and re-run to replace it with the managed wrapper."
            INSTALL_WRAPPER=false
        else
            BACKUP="${WRAPPER}.bak.$(date +%Y%m%d%H%M%S)"
            cp "$WRAPPER" "$BACKUP"
            echo "existing unmanaged wrapper backed up to ${BACKUP}"
        fi
    fi
fi

if $INSTALL_WRAPPER; then
    cat >"$WRAPPER" <<'WRAP'
#!/usr/bin/env bash
# managed by zcode-tui install.sh; re-run install.sh instead of editing.
#
# Routes the `zcode` command to the official CLI kernel shipped inside the
# ZCode desktop package, and falls back to the Rust zcode-tui when the
# official `tui` subcommand is missing @zcode/tui. The app dir is probed at
# runtime so package relocations and version bumps keep working.
set -u

FALLBACK_TUI="${ZCODE_FALLBACK_TUI:-__TUI_BIN__}"

find_app_dir() {
    if [ -n "${ZCODE_APP:-}" ] && [ -f "${ZCODE_APP}/resources/glm/zcode.cjs" ]; then
        printf '%s\n' "$ZCODE_APP"
        return 0
    fi
    if [ -f /opt/ZCode/resources/glm/zcode.cjs ]; then
        printf '%s\n' /opt/ZCode
        return 0
    fi
    local dir latest=""
    for dir in "$HOME"/.local/opt/zcode/*/opt/ZCode; do
        [ -f "${dir}/resources/glm/zcode.cjs" ] && latest="$dir"
    done
    if [ -n "$latest" ]; then
        printf '%s\n' "$latest"
        return 0
    fi
    return 1
}

wants_tui=false
if [ "$#" -eq 0 ] || [ "${1:-}" = "tui" ]; then
    wants_tui=true
fi

APP_DIR="$(find_app_dir)" || APP_DIR=""

if [ -z "$APP_DIR" ]; then
    if $wants_tui && [ -x "$FALLBACK_TUI" ]; then
        exec env ZCODE_TUI_ZCODE_BIN="$0" "$FALLBACK_TUI" "$@"
    fi
    echo "zcode: ZCode CLI kernel not found (checked \$ZCODE_APP, /opt/ZCode, ~/.local/opt/zcode/*/opt/ZCode)" >&2
    exit 127
fi

ZCODE_CJS="${APP_DIR}/resources/glm/zcode.cjs"
ELECTRON_BIN="${APP_DIR}/zcode"

# The kernel needs node:sqlite (Node >= 22.5); prefer the Electron-embedded
# Node over the system node.
run_node() {
    if [ -x "$ELECTRON_BIN" ]; then
        ELECTRON_RUN_AS_NODE=1 "$ELECTRON_BIN" "$@"
    else
        node "$@"
    fi
}

if $wants_tui && [ -x "$FALLBACK_TUI" ]; then
    # Probe whether the official TUI package resolves before handing the
    # terminal over; a broken import means the package still lacks it.
    if ! (cd "$(dirname "$ZCODE_CJS")" &&
        run_node --input-type=module -e "import('@zcode/tui').then(()=>process.exit(0),()=>process.exit(3))" >/dev/null 2>&1); then
        exec env ZCODE_TUI_ZCODE_BIN="$0" "$FALLBACK_TUI" "$@"
    fi
fi

if [ -x "$ELECTRON_BIN" ]; then
    ELECTRON_RUN_AS_NODE=1 exec "$ELECTRON_BIN" "$ZCODE_CJS" "$@"
fi
exec node "$ZCODE_CJS" "$@"
WRAP
    sed -i "s|__TUI_BIN__|${TUI_BIN}|" "$WRAPPER"
    chmod +x "$WRAPPER"
    echo "installed ${WRAPPER}"
fi

case ":$PATH:" in
    *":${BIN_DIR}:"*) ;;
    *) echo "note: ${BIN_DIR} is not on PATH" ;;
esac
echo "done"
