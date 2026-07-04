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

failed = [name for name, ok, _ in results if not ok]
print(f"\n{len(results) - len(failed)}/{len(results)} checks passed", flush=True)
sys.exit(1 if failed else 0)
