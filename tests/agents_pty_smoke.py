#!/usr/bin/env python3
"""Deterministic 80/120-column PTY smoke test for the Agent Inspector."""

import fcntl
import os
import pty
import re
import select
import struct
import subprocess
import tempfile
import termios
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(REPO, "target", "release", "zcode-tui")
ANSI = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b[()][0-9A-B]")


FAKE_ZCODE = r'''#!/usr/bin/env python3
import json
import os
import sys

if len(sys.argv) < 2 or sys.argv[1] != "app-server":
    print("0.16.3")
    raise SystemExit(0)

cancel_scenario = os.environ.get("ZCODE_TUI_TEST_CANCEL") == "1"
create_count = 0
send_count = 0

for line in sys.stdin:
    request = json.loads(line)
    ident = request.get("id")
    method = request.get("method")
    if method == "session/create":
        create_count += 1
        session_id = "parent-pty" if create_count == 1 else "new-pty"
        result = {"session": {"sessionId": session_id, "status": "idle"},
                  "slashCommands": [], "todos": []}
    elif method == "session/subscribe":
        result = {"sessionId": "parent-pty", "eventSeq": 0, "events": []}
    elif method == "v4/conversation/subscribe":
        print(json.dumps({"id": ident, "error": {"code": -32601,
              "message": "Method not found"}}), flush=True)
        continue
    elif method == "session/subagents":
        result = {"revision": 3, "childSessionIds": ["child-pty"], "running": [],
                  "ended": {"total": 1, "items": [{
                      "childSessionId": "child-pty", "agentId": "agent-pty",
                      "toolCallId": "call-agent", "subagentType": "Explore",
                      "title": "reviewer", "summary": "checked architecture",
                      "status": "success"}]}}
    elif method == "session/usage":
        result = {"totalTokens": 42000, "inputTokens": 30000,
                  "outputTokens": 8000, "reasoningTokens": 4000,
                  "cacheReadTokens": 18000, "modelRequestCount": 12}
    elif method == "session/read":
        context_used = 12000 if send_count <= 1 else (
            13000 if create_count == 1 else 14000
        )
        total_tokens = 16000 if send_count <= 1 else 17000
        result = {"snapshot": {"projection": {
            "contextUsed": context_used, "contextWindow": 200000,
            "totalTokenCount": total_tokens}}}
    elif method == "session/send":
        send_count += 1
        result = {"accepted": True, "sessionId": "parent-pty", "stateRevision": 1}
        print(json.dumps({"id": ident, "result": result}), flush=True)
        if cancel_scenario:
            delta = "partial-before-esc" if send_count == 1 else (
                "continued-parent-session" if create_count == 1 else "wrong-new-session"
            )
            print(json.dumps({"method": "session/event", "params": {
                "type": "model.streaming", "payload": {
                    "kind": "text_delta", "delta": delta, "done": False}}}), flush=True)
            if send_count > 1:
                context_used = 13000 if create_count == 1 else 14000
                print(json.dumps({"method": "state.updated", "params": {
                    "sessionId": "parent-pty", "reason": "prompt_completed",
                    "patch": {"status": "idle", "projection": {
                        "contextUsed": context_used}}}}), flush=True)
            continue
        print(json.dumps({"method": "session/event", "params": {
            "type": "background_task_started", "payload": {
                "taskId": "task-pty-exact", "toolCallId": "call-bash",
                "toolName": "Bash", "title": "build check", "status": "running",
                "command": "cargo check", "cancellable": True}}}), flush=True)
        print(json.dumps({"method": "session/event", "params": {
            "type": "model.streaming", "payload": {
                "kind": "text_delta", "delta": "ready", "done": False}}}), flush=True)
        print(json.dumps({"method": "state.updated", "params": {
            "sessionId": "parent-pty", "reason": "prompt_completed",
            "patch": {"status": "idle", "context": {
                "contextUsed": 12000, "contextWindow": 200000}}}}), flush=True)
        continue
    elif method == "session/stop":
        result = {"stopped": True, "sessionId": "parent-pty"}
        print(json.dumps({"id": ident, "result": result}), flush=True)
        print(json.dumps({"method": "session/event", "params": {
            "type": "turn.completed", "payload": {
                "response": "", "tokenCount": 0, "toolCallCount": 0,
                "duration": 0, "resultType": "cancelled"}}}), flush=True)
        continue
    else:
        result = {}
    print(json.dumps({"id": ident, "result": result}), flush=True)
'''


def run(width, switch_background=False, cancel_scenario=False):
    temp = tempfile.mkdtemp(prefix="zcode-agents-pty-")
    fake = os.path.join(temp, "zcode")
    with open(fake, "w", encoding="utf-8") as handle:
        handle.write(FAKE_ZCODE)
    os.chmod(fake, 0o755)

    env = dict(os.environ)
    env.update({
        "ZCODE_TUI_ZCODE_BIN": fake,
        "ZCODE_TUI_NO_UPDATE_CHECK": "1",
        "ZCODE_TUI_SKYLINE": "off",
        "NO_COLOR": "1",
    })
    if cancel_scenario:
        env["ZCODE_TUI_TEST_CANCEL"] = "1"
    master, slave = pty.openpty()
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 28, width, 0, 0))
    process = subprocess.Popen(
        [BIN], stdin=slave, stdout=slave, stderr=slave, cwd=temp, env=env, close_fds=True
    )
    os.close(slave)
    raw = b""
    deadline = time.time() + 18
    sent_hello = False
    sent_agents = False
    sent_tab = False
    sent_escape = False
    sent_second = False
    dsr_tail = b""
    while time.time() < deadline:
        ready, _, _ = select.select([master], [], [], 0.1)
        if ready:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                break
            if not chunk:
                break
            raw += chunk
            probe = dsr_tail + chunk
            for _ in range(probe.count(b"\x1b[6n")):
                os.write(master, b"\x1b[1;1R")
            dsr_tail = probe[-3:]
            visible = ANSI.sub("", raw.decode("utf-8", errors="replace")).replace("\r", "")
            screen = re.sub(r"\s+", "", visible)
            if not sent_hello and "ctx--·tok--" in screen:
                os.write(master, b"interrupt me\r" if cancel_scenario else b"hello\r")
                sent_hello = True
            elif cancel_scenario and not sent_escape and "ctx12k/200k" in screen:
                os.write(master, b"\x1b")
                sent_escape = True
            elif cancel_scenario and sent_escape and not sent_second and "sessionpreserved" in screen:
                os.write(master, b"continue here\r")
                sent_second = True
            elif cancel_scenario and sent_second and "continued-parent-session" in screen:
                break
            elif not cancel_scenario and sent_hello and not sent_agents and "tok42k" in screen:
                os.write(master, b"/agents\r")
                sent_agents = True
            elif (
                not cancel_scenario
                and switch_background
                and sent_agents
                and not sent_tab
                and "ParentAgent" in screen
            ):
                os.write(master, b"\t")
                sent_tab = True
    process.kill()
    process.wait(timeout=2)
    os.close(master)

    visible_writes = ANSI.sub("", raw.decode("utf-8", errors="replace")).replace("\r", "")
    return re.sub(r"\s+", "", visible_writes)


def require(label, condition, detail=""):
    print(f"{'PASS' if condition else 'FAIL'}  {label}", flush=True)
    if not condition:
        if detail:
            print(detail[-3000:], flush=True)
        raise SystemExit(1)


if not os.path.exists(BIN):
    raise SystemExit("build the release binary first: cargo build --release")

narrow = run(80)
require("80-column Agents tab shows parent", "ParentAgent" in narrow, narrow)
require("80-column Agents tab shows Subagent", "reviewer" in narrow)
require("80-column Inspector keeps parent input target", "inputtarget:parent" in narrow, narrow)
require("80-column footer shows current context", "ctx12k/200k(6%)" in narrow, narrow)
require("80-column footer shows refreshed tokens", "tok42k" in narrow, narrow)

wide = run(120, switch_background=True)
require("120-column Background tab shows Bash work", "buildcheck" in wide)
require(
    "120-column eligible task exposes cancel action",
    "xcancels" in wide,
    wide,
)
require("120-column Inspector keeps parent input target", "inputtarget:parent" in wide)

cancelled = run(100, cancel_scenario=True)
require("Esc reports preserved parent session", "sessionpreserved" in cancelled, cancelled)
require("ctx and tok refresh before thinking ends", "tok16k" in cancelled, cancelled)
require(
    "prompt after Esc continues the same session",
    "continued-parent-session" in cancelled and "wrong-new-session" not in cancelled,
    cancelled,
)

print("11/11 Agent Inspector, live usage, and cancel-continuity PTY checks passed", flush=True)
