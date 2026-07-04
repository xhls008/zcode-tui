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


def strip_ansi(raw):
    """ratatui interleaves cursor-move escapes between CJK cells, so
    substrings only match after stripping them."""
    return ANSI.sub("", raw)


def run_pty(env_extra, cwd, script, timeout=90):
    """script: list of (delay_before_s, bytes_to_send). Returns full output."""
    env = dict(os.environ)
    env["ZCODE_TUI_NO_UPDATE_CHECK"] = "1"
    env["ZCODE_TUI_ZCODE_BIN"] = os.path.expanduser("~/.local/bin/zcode")
    env.update(env_extra)
    master, slave = pty.openpty()
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    proc = subprocess.Popen(
        [BIN], stdin=slave, stdout=slave, stderr=slave,
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
check("s3: skyline strip shown", "鸟巢" in plain and "天坛" in plain and "长城" in plain)
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
check("s4: art still present", "鸟巢" in strip_ansi(out))
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

failed = [name for name, ok, _ in results if not ok]
print(f"\n{len(results) - len(failed)}/{len(results)} checks passed", flush=True)
sys.exit(1 if failed else 0)
