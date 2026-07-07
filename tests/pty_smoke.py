#!/usr/bin/env python3
"""pty smoke: drives the real release binary against the real kernel.

Manual gate, not CI: needs a configured kernel (~/.zcode/cli/config.json)
and network/model access. Build first: cargo build --release
Run: python3 tests/pty_smoke.py
"""
import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(REPO, "target", "release", "zcode-tui")

import tempfile
SPIKE = tempfile.mkdtemp(prefix="zcode-smoke-work-")
with open(os.path.join(SPIKE, "notes.txt"), "w") as fh:
    fh.write("zcode-tui smoke notes:\nThe quick brown fox jumps over the lazy dog.\n")
with open(os.path.join(SPIKE, "data.txt"), "w") as fh:
    fh.write("alpha\nbeta\n")
results = []

import re
ANSI = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b[()][0-9A-B]")

import pyte


def screen_seen(raw, needle, step=2048):
    """True if `needle` was visible on the emulated terminal screen at any
    point while replaying `raw` (bytes) in `step`-sized chunks.

    Substring checks on the raw pty stream are unreliable for STATUS BAR text:
    ratatui diff-renders, so updating e.g. "streaming (app-server)" in place to
    "done (8.3s)" skips unchanged cells (the shared 'e') and the byte stream
    never carries the new text contiguously. Only a terminal emulation sees
    what the user sees. Transcript lines are full-row writes, so plain
    substring checks stay fine for those.
    """
    screen = pyte.Screen(120, 40)
    stream = pyte.ByteStream(screen)
    for at in range(0, len(raw), step):
        stream.feed(raw[at : at + step])
        if needle in "\n".join(screen.display):
            return True
    return False


def strip_ansi(raw):
    """ratatui interleaves cursor-move escapes between CJK cells, so
    substrings only match after stripping them."""
    return ANSI.sub("", raw)


def run_pty(env_extra, cwd, script, timeout=90, args=None):
    """script: list of (delay_before_s, bytes_to_send). Returns full output.
    args: extra CLI arguments for the binary (e.g. ["--mode", "plan"])."""
    env = dict(os.environ)
    env["ZCODE_TUI_NO_UPDATE_CHECK"] = "1"
    env["ZCODE_TUI_ZCODE_BIN"] = os.path.expanduser("~/.local/bin/zcode")
    # Force the text skyline so the graphics-protocol probe is skipped: this
    # dumb pty can neither render a graphics protocol nor answer the probe's
    # DSR terminator, which would otherwise leave a thread blocked reading
    # stdin for 2s and racing the event loop for scripted input. Real terminals
    # answer DSR in ms; the true-image path can only be verified by a human in
    # kitty/sixel/iTerm2. Scenarios may override.
    env["ZCODE_TUI_SKYLINE"] = "braille"
    env.update(env_extra)
    master, slave = pty.openpty()
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    proc = subprocess.Popen(
        [BIN] + (args or []), stdin=slave, stdout=slave, stderr=slave,
        env=env, cwd=cwd, close_fds=True,
    )
    os.close(slave)
    buf = b""
    deadline = time.time() + timeout
    pending = list(script)
    next_at = time.time() + pending[0][0] if pending else None
    while time.time() < deadline:
        if pending and time.time() >= next_at:
            _, data = pending.pop(0)
            os.write(master, data)
            next_at = time.time() + pending[0][0] if pending else None
        ready, _, _ = select.select([master], [], [], 0.1)
        if ready:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                break
            if not chunk:
                break
            buf += chunk
        if proc.poll() is not None and not pending:
            break
    if proc.poll() is None:
        proc.kill()
    os.close(master)
    # Raw bytes kept for pyte screen-state checks (see screen_seen).
    run_pty.last_raw = buf
    return buf.decode("utf-8", errors="replace")


def check(name, cond, detail=""):
    results.append((name, bool(cond), detail))
    print(f"{'PASS' if cond else 'FAIL'}  {name}{'  [' + detail + ']' if detail and not cond else ''}",
          flush=True)


# ---- scenario 1: live tool chips + summary render + watermark + cancel ----
print("== scenario 1: real prompt with live progress ==", flush=True)
out = run_pty(
    {}, SPIKE,
    [
        (1.5, b"Read notes.txt and data.txt, then summarize both in one sentence."),
        (0.5, b"\r"),
        (75.0, b"/exit"),   # wait for the turn, then leave
        (0.5, b"\r"),
    ],
    timeout=95,
)
plain = strip_ansi(out)
check(
    "s1: tool chip appeared (completed check next to tool name)",
    "✓ Read" in plain or "✓ Bash" in plain or "✓ Glob" in plain,
)
check("s1: prompt completed (done status)", "done (" in plain)
check("s1: context watermark in footer", "ctx " in plain and "%" in plain)
check("s1: no raw summary JSON leaked to transcript", '"sessionId"' not in plain)
check(
    "s1: response text rendered",
    "notes" in plain.lower() or "alpha" in plain.lower() or "fox" in plain.lower(),
)

# ---- scenario 2: cancel path (process-group kill unchanged) ----
print("== scenario 2: cancel mid-run ==", flush=True)
out = run_pty(
    {}, SPIKE,
    [
        (1.5, b"List every file then explain each in detail."),
        (0.5, b"\r"),
        (4.0, b"\x1b"),      # Esc: cancel the running job
        (4.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=40,
)
check("s2: job cancelled", "cancelled" in strip_ansi(out))

# ---- scenario 3: unauth screen on a fresh HOME (db missing degrade too) ----
print("== scenario 3: unauth screen, fresh HOME ==", flush=True)
fresh_home = tempfile.mkdtemp(prefix="zcode-smoke-home-")
out = run_pty(
    {"HOME": fresh_home}, SPIKE,
    [
        (2.5, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=20,
)
plain = strip_ansi(out)
check("s3: purple wordmark shown", "178;108;196" in out)
check("s3: skyline strip shown (ZhiPU on horizon, braille or wire)", "ZhiPU" in plain)
check("s3: not-configured headline", "not configured" in plain)
check("s3: coding-plan login paths listed", "bigmodel-coding-plan-api-key" in plain)
check("s3: no crash with missing db (banner rendered)", "ZCODE" in plain or "zcode" in plain)

# ---- scenario 4: unauth screen honours NO_COLOR ----
print("== scenario 4: unauth screen NO_COLOR ==", flush=True)
out = run_pty(
    {"HOME": fresh_home, "NO_COLOR": "1"}, SPIKE,
    [
        (2.5, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=20,
)
check("s4: art still present", "ZhiPU" in strip_ansi(out))
check("s4: no brand purple escape", "178;108;196" not in out)


# ---- scenario 5: /sessions picker (user story: 找回昨天的会话) ----
print("== scenario 5: /sessions picker ==", flush=True)
out = run_pty(
    {}, SPIKE,
    [
        (2.5, b"/sessions"),
        (0.8, b"\r"),      # submit the command -> picker opens
        (1.0, b"\r"),      # Enter resumes the selected session
        (1.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=25,
)
plain = strip_ansi(out)
check("s5: picker overlay shown", "sessions · Enter resumes" in plain)
check("s5: Enter resumes a session", "resuming sess" in plain)

# ---- scenario 6: Ctrl+R reverse search (user story: 只记得一个词) ----
print("== scenario 6: Ctrl+R reverse search ==", flush=True)
out = run_pty(
    {}, SPIKE,
    [
        (2.5, b"\x12"),     # Ctrl+R
        (0.5, b"read"),
        (0.8, b"\r"),      # recall best match into the composer
        (0.8, b"\x15"),     # Ctrl+U clear input (avoid submitting a prompt)
        (0.5, b"/exit"),
        (0.4, b"\r"),
    ],
    timeout=25,
)
plain = strip_ansi(out)
check("s6: search overlay shown", "reverse search:" in plain)
# ratatui diff-renders cells: a status sharing a prefix with the previous one
# only emits the divergent tail, so probe a suffix plus the recalled text.
check(
    "s6: match recalled from kernel-persisted history",
    "from history" in plain and "Read notes.txt" in plain,
)

# ---- scenario 7: folding long output (user story: 长输出不刷屏) ----
print("== scenario 7: fold + Ctrl+O ==", flush=True)
out = run_pty(
    {}, SPIKE,
    [
        (2.0, b"! seq 1 120"),
        (0.5, b"\r"),
        (3.0, b"\x0f"),     # Ctrl+O expand
        (1.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=30,
)
plain = strip_ansi(out)
check("s7: long output folded with hidden count", "+112 lines" in plain and "Ctrl+O" in plain)
check("s7: Ctrl+O expands", "expanded (Ctrl+O folds back)" in plain)

# ---- scenario 8: ui config color override (user story: 换掉强调色) ----
print("== scenario 8: config accent override ==", flush=True)
cfg = os.path.join(SPIKE, "tui-config")
with open(cfg, "w") as fh:
    fh.write("accent = #ff8800\n")
out = run_pty(
    {"ZCODE_TUI_CONFIG": cfg}, SPIKE,
    [
        (2.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=15,
)
check("s8: custom accent color rendered", "38;2;255;136;0" in out)

# ---- scenario 9: mouse scroll does not break anything ----
print("== scenario 9: mouse wheel ==", flush=True)
out = run_pty(
    {}, SPIKE,
    [
        (2.0, b"\x1b[<64;10;10M\x1b[<64;10;10M"),  # SGR wheel-up twice
        (1.0, b"\x1b[<65;10;10M"),                  # wheel-down
        (1.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=15,
)
check("s9: no panic under mouse events", "panicked" not in strip_ansi(out))

# ---- scenario 10: app-server true streaming (user story: 单轮问答也流式) ----
# Opt-in ZCODE_TUI_APP_SERVER=1: the reply must stream token-by-token straight
# into the transcript (status bar reads "streaming (app-server)"), not land in
# one block at the end. Needs the real kernel + model.
print("== scenario 10: app-server streaming (opt-in) ==", flush=True)
out = run_pty(
    {"ZCODE_TUI_APP_SERVER": "1"}, SPIKE,
    [
        (1.5, b"List the numbers 1 through 20, one per line, nothing else."),
        (0.5, b"\r"),
        (100.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=120,
)
plain = strip_ansi(out)
raw = run_pty.last_raw
# Status-bar text needs the pyte screen check: ratatui's diff rendering can
# split in-place status updates so the raw stream never carries them whole.
check("s10: app-server path active (streaming status)", screen_seen(raw, "streaming (app-server)"))
check("s10: turn completed", screen_seen(raw, "done ("))
check("s10: answer rendered", "20" in plain)

# ---- scenario 11: seamless downgrade when app-server is unusable ----
# A fake `zcode` whose `app-server` subcommand exits immediately forces the
# streaming handshake to fail; the process must downgrade once and answer via
# --prompt. Deterministic (no model): the fake returns a --json summary.
print("== scenario 11: app-server downgrade -> --prompt ==", flush=True)
fake_bin = os.path.join(SPIKE, "fake-zcode")
with open(fake_bin, "w") as fh:
    fh.write(
        "#!/bin/sh\n"
        "case \"$1\" in\n"
        "  app-server) exit 0 ;;\n"
        "  version) echo 0.15.0 ;;\n"
        "  *) printf '%s\\n' "
        "'{\"type\":\"result\",\"response\":\"downgrade fallback ok\",\"sessionId\":\"sess_fake\"}' ;;\n"
        "esac\n"
    )
os.chmod(fake_bin, 0o755)
out = run_pty(
    {"ZCODE_TUI_APP_SERVER": "1", "ZCODE_TUI_ZCODE_BIN": fake_bin}, SPIKE,
    [
        (1.5, b"hello"),
        (0.5, b"\r"),
        (3.0, b"again"),
        (0.5, b"\r"),
        (3.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=20,
)
plain = strip_ansi(out)
check("s11: downgrade notice shown", "falling back to --prompt" in plain)
check("s11: --prompt fallback answered", "downgrade fallback ok" in plain)
check("s11: downgrade announced once (permanent)", plain.count("falling back to --prompt") == 1)

# ---- scenario 12: app-server tool chips + foldable tool output ----
# Opt-in streaming with a tool-triggering prompt: the tool call must land in
# the transcript as a foldable entry (name · duration + output), the long
# output must fold with Ctrl+O, and the turn must finalize (done). Real kernel.
print("== scenario 12: app-server tool chip + output folding ==", flush=True)
tool_dir = tempfile.mkdtemp(prefix="zcode-smoke-tool-")
with open(os.path.join(tool_dir, "rows.txt"), "w") as fh:
    fh.write("".join(f"row {i}: value {i * 7}\n" for i in range(1, 31)))
out = run_pty(
    {"ZCODE_TUI_APP_SERVER": "1"}, tool_dir,
    [
        (1.5, b"Read rows.txt (show the full contents), then name the pattern in one sentence."),
        (0.5, b"\r"),
        (100.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=120,
)
plain = strip_ansi(out)
check("s12: tool call persisted to transcript", "• Read" in plain or "• Bash" in plain)
check("s12: long tool output folded with Ctrl+O", "Ctrl+O" in plain and "lines" in plain)
# Status-bar text: pyte screen check (diff rendering; see screen_seen).
check("s12: turn finalized (no hang)", screen_seen(run_pty.last_raw, "done ("))

# ---- scenario 13: plan-mode permission approval overlay + Esc declines ----
# Plan mode gates file writes behind interaction/requestUserInput. The TUI
# must surface the approval overlay; Esc declines (session/stop) and the turn
# must NOT hang until the 600s backstop. Real kernel; the model needs 30-100s
# to reach the gated tool.
print("== scenario 13: plan-mode approval overlay (app-server) ==", flush=True)
perm_dir = tempfile.mkdtemp(prefix="zcode-smoke-perm-")
out = run_pty(
    {"ZCODE_TUI_APP_SERVER": "1"}, perm_dir,
    [
        (1.5, b"Create a file named perm.txt containing hello. Just create it."),
        (0.5, b"\r"),
        (120.0, b"\x1b"),   # Esc: decline the approval (or no-op if missed)
        (6.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=145,
    args=["--mode", "plan"],   # plan gates the write -> interaction request
)
plain = strip_ansi(out)
raw = run_pty.last_raw
check("s13: approval overlay appeared", screen_seen(raw, "Enter answers"))
check("s13: declined without hanging", "declined" in plain and "cancelled" in plain)
check("s13: plan gating held (no file)", not os.path.exists(os.path.join(perm_dir, "perm.txt")))

# ---- scenario 14: session controls — /model picker + /compact round-trip ----
print("== scenario 14: /model + /compact (app-server) ==", flush=True)
out = run_pty(
    {"ZCODE_TUI_APP_SERVER": "1"}, SPIKE,
    [
        (1.5, b"Reply with exactly: ok"),
        (0.5, b"\r"),
        (45.0, b"/model"),
        (0.5, b"\r"),      # open the picker
        (1.5, b"\r"),      # Enter re-selects the current model (harmless)
        (2.0, b"/compact"),
        (0.5, b"\r"),
        (12.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=90,
)
raw = run_pty.last_raw
check("s14: model picker listed kernel models", screen_seen(raw, "Enter selects"))
check("s14: compact round-trip acknowledged", screen_seen(raw, "compacted"))

# ---- scenario 15: steer — typing mid-turn steers instead of queueing ----
print("== scenario 15: steer mid-turn (app-server) ==", flush=True)
out = run_pty(
    {"ZCODE_TUI_APP_SERVER": "1"}, SPIKE,
    [
        (1.5, b"Count slowly from 1 to 30, one number per line."),
        (0.5, b"\r"),
        (6.0, b"Stop counting and just say STEERED."),
        (0.5, b"\r"),
        (60.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=95,
)
plain = strip_ansi(out)
raw = run_pty.last_raw
check("s15: steer marker in transcript", "steering the running turn" in plain)
check("s15: input not queued", not screen_seen(raw, "queued ("))
check("s15: turn completed after steer", screen_seen(raw, "done ("))

# ---- scenario 16: build-mode Write approval (permission request) ----
# Default (build) mode gates side-effect tools behind
# interaction/requestPermission (options with ready-made response objects).
# Enter approves "Allow once" and the Write must land within the SAME turn.
print("== scenario 16: build-mode Write approval (app-server) ==", flush=True)
wperm_dir = tempfile.mkdtemp(prefix="zcode-smoke-wperm-")
out = run_pty(
    {"ZCODE_TUI_APP_SERVER": "1"}, wperm_dir,
    [
        (1.5, b"Create a file named w.txt containing hi. Just do it."),
        (0.5, b"\r"),
        (60.0, b"\r"),     # Enter: Allow once
        (60.0, b"/exit"),
        (0.5, b"\r"),
    ],
    timeout=130,
)
plain = strip_ansi(out)
raw = run_pty.last_raw
check("s16: permission overlay with options", screen_seen(raw, "Allow once"))
check("s16: approved and turn completed", screen_seen(raw, "done ("))
check("s16: write landed after approval",
      os.path.exists(os.path.join(wperm_dir, "w.txt")))

failed = [name for name, ok, _ in results if not ok]
print(f"\n{len(results) - len(failed)}/{len(results)} checks passed", flush=True)
sys.exit(1 if failed else 0)
