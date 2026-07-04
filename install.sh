#!/usr/bin/env bash
# Build and install zcode-tui plus the `zcode` CLI wrapper.
#
#   ./install.sh                 build + install binary and wrapper
#   ./install.sh --prefix DIR    install under DIR/bin (default ~/.local)
#   ./install.sh --no-wrapper    install only the zcode-tui binary
#   ./install.sh --no-build      skip cargo; wrap an existing zcode-tui binary
#   ./install.sh --uninstall     remove installed binary and managed wrapper
#
# Re-run after `git pull` or local changes to update the installation.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${HOME}/.local"
INSTALL_WRAPPER=true
BUILD=true
UNINSTALL=false
MARKER="managed by zcode-tui install.sh"

usage() {
    sed -n '2,10p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
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
        --no-build) BUILD=false ;;
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

if $BUILD; then
    command -v cargo >/dev/null 2>&1 || {
        echo "error: cargo not found; install Rust from https://rustup.rs" >&2
        echo "       (or download a release binary to ${TUI_BIN} and re-run with --no-build)" >&2
        exit 1
    }

    echo "building release binary..."
    cargo build --release --quiet --manifest-path "${REPO_DIR}/Cargo.toml"

    install -Dm755 "${REPO_DIR}/target/release/zcode-tui" "$TUI_BIN"
    echo "installed ${TUI_BIN} ($("$TUI_BIN" --version))"
else
    if [ ! -x "$TUI_BIN" ]; then
        echo "error: --no-build needs an existing ${TUI_BIN}; download it from" >&2
        echo "       https://github.com/xhls008/zcode-tui/releases first" >&2
        exit 1
    fi
    echo "using existing ${TUI_BIN} ($("$TUI_BIN" --version))"
fi

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

# The kernel needs node:sqlite (Node >= 22.5). Prefer the Electron-embedded
# Node, but on headless boxes Electron can fail to start even with
# ELECTRON_RUN_AS_NODE=1 (its desktop shared libraries are still linked at
# load time), so probe it once per run and fall back to a recent-enough
# system node. ZCODE_FORCE_SYSTEM_NODE=1 skips Electron entirely.
ELECTRON_USABLE=""
electron_usable() {
    if [ -z "$ELECTRON_USABLE" ]; then
        if [ -z "${ZCODE_FORCE_SYSTEM_NODE:-}" ] && [ -x "$ELECTRON_BIN" ] &&
            ELECTRON_RUN_AS_NODE=1 "$ELECTRON_BIN" -e "" >/dev/null 2>&1; then
            ELECTRON_USABLE=yes
        else
            ELECTRON_USABLE=no
        fi
    fi
    [ "$ELECTRON_USABLE" = yes ]
}

node_is_recent() {
    command -v node >/dev/null 2>&1 || return 1
    node -e 'const [maj, min] = process.versions.node.split(".").map(Number); process.exit(maj > 22 || (maj === 22 && min >= 5) ? 0 : 1)' 2>/dev/null
}

run_node() {
    if electron_usable; then
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

if electron_usable; then
    ELECTRON_RUN_AS_NODE=1 exec "$ELECTRON_BIN" "$ZCODE_CJS" "$@"
fi
if node_is_recent; then
    exec node "$ZCODE_CJS" "$@"
fi
echo "zcode: cannot run the CLI kernel: Electron at ${ELECTRON_BIN} failed to start" >&2
echo "       (missing desktop libraries on a headless box?) and no system node >= 22.5" >&2
echo "       was found (the kernel needs node:sqlite). Install Node.js >= 22.5 or the" >&2
echo "       Electron desktop libraries; ZCODE_FORCE_SYSTEM_NODE=1 forces system node." >&2
exit 127
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
