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
    local dir
    local -a candidates=()
    for dir in "$HOME"/.local/opt/zcode/*/opt/ZCode; do
        [ -f "${dir}/resources/glm/zcode.cjs" ] && candidates+=("$dir")
    done
    if [ "${#candidates[@]}" -gt 0 ]; then
        printf '%s\n' "${candidates[@]}" | sort -V | tail -n 1
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
BROWSER_LOADER="${PREFIX}/lib/zcode-tui/browser-runtime.mjs"

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
    if [ -f "$BROWSER_LOADER" ] && grep -q "$MARKER" "$BROWSER_LOADER"; then
        rm -f "$BROWSER_LOADER"
        echo "removed ${BROWSER_LOADER}"
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

    mkdir -p "$BIN_DIR"
    install -m755 "${REPO_DIR}/target/release/zcode-tui" "$TUI_BIN"
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
    mkdir -p "$(dirname "$BROWSER_LOADER")"
    cat >"$BROWSER_LOADER" <<'BROWSER_RUNTIME'
// managed by zcode-tui install.sh; re-run install.sh instead of editing.
// Resolve only a missing playwright-core from the matching official desktop package.
// No downloads, patched kernel files, personal browser profiles, or global NODE_OPTIONS.
import { createRequire, register } from 'node:module';
import { isMainThread } from 'node:worker_threads';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import * as nodeFs from 'node:fs';
import { homedir } from 'node:os';
// Electron's patched fs treats app.asar as a virtual directory. Raw archive
// offsets must use original-fs; system Node already exposes the real bytes.
const fs = process.versions.electron ? createRequire(import.meta.url)('original-fs') : nodeFs;
let settings;
export function initialize(data) { settings = data; }
if (isMainThread && process.argv[1]) {
  // Browser Use's node_repl host is a second Node process whose argv[1] is
  // inside the seeded plugin cache, not the official Desktop package.  The
  // wrapper exports ZCODE_APP so nested MCP hosts still resolve the matching
  // app.asar instead of reporting a missing built-in browser runtime.
  const appDir = process.env.ZCODE_APP;
  const resources = appDir && fs.existsSync(join(appDir, 'resources'))
    ? join(appDir, 'resources')
    : dirname(dirname(fs.realpathSync(process.argv[1])));
  register(import.meta.url, { data: {
    resources,
    cache: join(process.env.XDG_CACHE_HOME || join(homedir(), '.cache'), 'zcode-tui', 'browser-runtime'),
  }});
}
function unpackRuntime() {
  const archive = join(settings.resources, 'app.asar');
  const fd = fs.openSync(archive, 'r');
  let staging;
  try {
    const prefix = Buffer.alloc(16);
    if (fs.readSync(fd, prefix, 0, 16, 0) !== 16) throw new Error('invalid ASAR header');
    const headerSize = prefix.readUInt32LE(4), jsonSize = prefix.readUInt32LE(12);
    if (jsonSize > 16 * 1024 * 1024 || headerSize < jsonSize + 8) throw new Error('invalid ASAR header size');
    const bytes = Buffer.alloc(jsonSize);
    if (fs.readSync(fd, bytes, 0, jsonSize, 16) !== jsonSize) throw new Error('incomplete ASAR header');
    const files = JSON.parse(bytes).files?.node_modules?.files?.['playwright-core'];
    if (!files?.files) throw new Error('official app.asar does not contain playwright-core');
    const key = createHash('sha256').update(archive).update(bytes).digest('hex');
    const target = join(settings.cache, key);
    const entry = join(target, 'index.mjs');
    if (fs.existsSync(entry) && fs.existsSync(join(target, '.complete'))) return pathToFileURL(entry).href;
    fs.mkdirSync(settings.cache, { recursive: true, mode: 0o700 });
    staging = fs.mkdtempSync(join(settings.cache, '.extract-'));
    const archiveSize = fs.fstatSync(fd).size;
    let total = 0;
    function extract(tree, relative = '') {
      for (const [name, node] of Object.entries(tree.files)) {
        if (!name || name === '.' || name === '..' || /[\\/]/.test(name) || node.link) throw new Error('invalid ASAR member');
        const rel = join(relative, name), output = join(staging, rel);
        if (node.files) { fs.mkdirSync(output); extract(node, rel); continue; }
        if (!Number.isSafeInteger(node.size) || node.size < 0 || (total += node.size) > 128 * 1024 * 1024) throw new Error('invalid runtime size');
        let content;
        if (node.unpacked) {
          content = fs.readFileSync(join(archive + '.unpacked', 'node_modules', 'playwright-core', rel));
        } else {
          const at = 8 + headerSize + Number(node.offset);
          if (!Number.isSafeInteger(at) || at < 8 + headerSize || at + node.size > archiveSize) throw new Error('invalid ASAR offset');
          content = Buffer.alloc(node.size);
          if (fs.readSync(fd, content, 0, node.size, at) !== node.size) throw new Error('incomplete runtime member');
        }
        if (content.length !== node.size) throw new Error('runtime size mismatch');
        if (node.integrity?.algorithm === 'SHA256' && createHash('sha256').update(content).digest('hex') !== node.integrity.hash) throw new Error('runtime integrity mismatch');
        fs.writeFileSync(output, content, { mode: 0o600 });
      }
    }
    extract(files);
    if (!fs.existsSync(join(staging, 'index.mjs'))) throw new Error('runtime entry point missing');
    fs.writeFileSync(join(staging, '.complete'), 'official bundled runtime\n');
    try { fs.renameSync(staging, target); staging = undefined; }
    catch (error) { if (!fs.existsSync(join(target, '.complete'))) throw error; }
    return pathToFileURL(entry).href;
  } finally {
    fs.closeSync(fd);
    if (staging) fs.rmSync(staging, { recursive: true, force: true });
  }
}
export async function resolve(specifier, context, nextResolve) {
  try { return await nextResolve(specifier, context); }
  catch (error) {
    if (specifier !== 'playwright-core' || error.code !== 'ERR_MODULE_NOT_FOUND') throw error;
    try { return { url: unpackRuntime(), shortCircuit: true }; }
    catch (cause) {
      console.error(`Browser Use: bundled runtime repair failed: ${cause.message}`);
      throw error;
    }
  }
}
BROWSER_RUNTIME
    chmod 644 "$BROWSER_LOADER"
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

# Privacy inspection must work without starting or even locating the kernel.
if [ "${1:-}" = check-zhipu-upload ]; then
    exec "$FALLBACK_TUI" "$@"
fi

find_app_dir() {
    if [ -n "${ZCODE_APP:-}" ] && [ -f "${ZCODE_APP}/resources/glm/zcode.cjs" ]; then
        printf '%s\n' "$ZCODE_APP"
        return 0
    fi
    if [ -f /opt/ZCode/resources/glm/zcode.cjs ]; then
        printf '%s\n' /opt/ZCode
        return 0
    fi
    local dir
    local -a candidates=()
    for dir in "$HOME"/.local/opt/zcode/*/opt/ZCode; do
        [ -f "${dir}/resources/glm/zcode.cjs" ] && candidates+=("$dir")
    done
    if [ "${#candidates[@]}" -gt 0 ]; then
        printf '%s\n' "${candidates[@]}" | sort -V | tail -n 1
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
        exec env \
            ZCODE_TUI_ZCODE_BIN="$0" \
            ZCODE_TUI_APP_SERVER="${ZCODE_TUI_APP_SERVER:-1}" \
            "$FALLBACK_TUI" "$@"
    fi
    echo "zcode: ZCode CLI kernel not found (checked \$ZCODE_APP, /opt/ZCode, ~/.local/opt/zcode/*/opt/ZCode)" >&2
    exit 127
fi

ZCODE_CJS="${APP_DIR}/resources/glm/zcode.cjs"
ELECTRON_BIN="${APP_DIR}/zcode"
# Keep the resolved package root available to Browser Use's nested node_repl
# host.  The helper must not infer resources from the plugin-cache argv path.
export ZCODE_APP="$APP_DIR"

# 3.12.x moved the bundled provider configuration outside resources/glm.
# Keep explicit user overrides, and let the kernel manage its own user cache.
if [ -z "${ZCODE_BUILTIN_PROVIDER_CONFIG_FILE:-}" ] &&
   [ -f "${APP_DIR}/resources/config/provider/zcode-builtin.json" ]; then
    export ZCODE_BUILTIN_PROVIDER_CONFIG_FILE="${APP_DIR}/resources/config/provider/zcode-builtin.json"
fi

# 3.12.3 replaced host-injected runtimeModel/provider registry with native
# provider config. Its standalone CLI owns migration + account authentication;
# do not inject legacy credentials into the incompatible Desktop protocol.
if grep -Fq '"workspace/readPresentation"' "$ZCODE_CJS" &&
   ! grep -Fq '"workspace/updateProviderRegistry"' "$ZCODE_CJS"; then
    export ZCODE_TUI_CLASSIC_REASON="Official 3.12.x provider-config protocol: classic CLI compatibility mode (no V4/model picker)."
    export ZCODE_TUI_APP_SERVER=0
    if [ "${ZCODE_TUI_CLIENT:-}" = 1 ] && [ "${1:-}" = app-server ]; then
        echo "zcode-tui: $ZCODE_TUI_CLASSIC_REASON" >&2
        exit 64
    fi
fi

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
        exec env \
            ZCODE_APP="$APP_DIR" \
            ZCODE_TUI_ZCODE_BIN="$0" \
            ZCODE_TUI_APP_SERVER="${ZCODE_TUI_APP_SERVER:-1}" \
            "$FALLBACK_TUI" "$@"
    fi
fi

# Only Browser Use needs the missing-module bridge. It extracts the matching
# official runtime into the user cache, never into /opt or the project.
BROWSER_ARGS=()
browser_requested=false
surface_requested=false
for arg in "$@"; do
    case "$arg" in
        --browser-use|--browser-use=*) browser_requested=true ;;
        --surface|--surface=*) surface_requested=true ;;
    esac
done
if [ "${ZCODE_TUI_BROWSER_RUNTIME:-}" != off ]; then
    for arg in "$@"; do
        case "$arg" in
            --browser-use|--browser-use=headless)
                helper="$(dirname "$0")/../lib/zcode-tui/browser-runtime.mjs"
                if [ -f "$helper" ]; then
                    BROWSER_ARGS=(--import "$helper")
                else
                    echo "Browser Use: runtime helper missing; re-run install.sh" >&2
                fi
                break ;;
        esac
    done
fi
# ZCode 3.14's headless Browser Use surface is explicit.  Older kernels do
# not advertise --surface, so only add it when the exact kernel exposes the
# option; callers may still override it explicitly.
if $browser_requested && ! $surface_requested && grep -Fq -- '--surface' "$ZCODE_CJS"; then
    set -- --surface terminal "$@"
fi
if electron_usable; then
    ELECTRON_RUN_AS_NODE=1 exec "$ELECTRON_BIN" "${BROWSER_ARGS[@]}" "$ZCODE_CJS" "$@"
fi
if node_is_recent; then
    exec node "${BROWSER_ARGS[@]}" "$ZCODE_CJS" "$@"
fi
echo "zcode: cannot run the CLI kernel: Electron at ${ELECTRON_BIN} failed to start" >&2
echo "       (missing desktop libraries on a headless box?) and no system node >= 22.5" >&2
echo "       was found (the kernel needs node:sqlite). Install Node.js >= 22.5 or the" >&2
echo "       Electron desktop libraries; ZCODE_FORCE_SYSTEM_NODE=1 forces system node." >&2
exit 127
WRAP
    sed -i.bak "s|__TUI_BIN__|${TUI_BIN}|" "$WRAPPER"
    rm -f "${WRAPPER}.bak"
    chmod +x "$WRAPPER"
    echo "installed ${WRAPPER}"
fi

case ":$PATH:" in
    *":${BIN_DIR}:"*) ;;
    *) echo "note: ${BIN_DIR} is not on PATH" ;;
esac
echo "done"
