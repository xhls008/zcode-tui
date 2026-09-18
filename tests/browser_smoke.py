#!/usr/bin/env python3
"""Credential-free Browser Use PTY checks.

Default: fake CLI (no browser/network/model). --official: real installed kernel,
bundled Playwright and Chrome, local HTTP page and fake Anthropic SSE provider.
The real-browser success case explicitly uses yolo ONLY in this disposable HOME
and workspace with a deterministic model that emits the fixed browser calls below.
"""
import argparse
import codecs
import fcntl
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import sqlite3
import struct
import subprocess
import tempfile
import termios
import threading
import time
import uuid

import pyte

from browser_runtime_test import REPO, clean_env, install_wrapper

BIN = Path(os.environ.get("ZCODE_TEST_BIN", REPO / "target/release/zcode-tui")).resolve()
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b[()][A-Z0-9]")


def owned_processes(home):
    # Linux leak check scoped to this disposable workspace/profile, never a
    # blanket kill of the user's Chrome or Node processes.
    if not Path("/proc").exists():
        return []
    found = []
    for path in Path("/proc").glob("[0-9]*/cmdline"):
        try:
            if os.fsencode(home) in path.read_bytes() and int(path.parent.name) != os.getpid():
                found.append(int(path.parent.name))
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            pass
    return found


def check_cleanup(home):
    deadline = time.monotonic() + 5
    while owned_processes(home) and time.monotonic() < deadline:
        time.sleep(0.1)
    remaining = owned_processes(home)
    for pid in remaining:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    assert not remaining, f"Browser/kernel processes leaked after exit: {remaining}"


def drive_tui(home, wrapper, browser, marker, mode=None, extra_env=None, cancel_when=None):
    env = clean_env(home)
    env.update({"TERM": "xterm-256color", "NO_COLOR": "1", "ZCODE_TUI_NO_UPDATE_CHECK": "1",
                "ZCODE_TUI_ZCODE_BIN": str(wrapper), "ZCODE_TUI_SKYLINE": "off"})
    env.update(extra_env or {})
    cmd = [str(BIN), "--browser-use", "headless", "--browser-executable", str(browser)]
    if mode:
        cmd += ["--mode", mode]
    master, slave = pty.openpty()
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 120, 0, 0))
    proc = subprocess.Popen(cmd, stdin=slave, stdout=slave, stderr=slave, env=env,
                            cwd=home, start_new_session=True)
    os.close(slave)
    started = time.monotonic()
    raw = b""
    screen = pyte.Screen(120, 35)
    stream = pyte.Stream(screen)
    decoder = codecs.getincrementaldecoder("utf-8")("replace")
    snapshots = []
    sent = exiting = cancelled = result_seen = False
    try:
        while time.monotonic() - started < 60:
            if select.select([master], [], [], 0.05)[0]:
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                if b"\x1b[6n" in raw[-8:] + chunk:
                    os.write(master, b"\x1b[1;1R")
                raw += chunk
                stream.feed(decoder.decode(chunk))
                snapshots.append("\n".join(
                    "".join((screen.buffer[y][x].data or " ") for x in range(screen.columns))
                    for y in range(screen.lines)))
            elapsed = time.monotonic() - started
            if not sent and elapsed > 0.8:
                os.write(master, b"Check the isolated local browser page\r")
                sent = True
            if sent and cancel_when and cancel_when() and not cancelled:
                os.write(master, b"\x1b")
                cancelled = True
            plain = ANSI.sub("", raw.decode(errors="replace")) + "\n" + (snapshots[-1] if snapshots else "")
            result_seen |= marker in plain
            settled = any(status in plain for status in ["done (", "done with tool errors (",
                "browser unavailable", "zcode --prompt failed:", "zcode --prompt cancelled"])
            if marker == "browser-e2e-ok" and settled and "browser-tool-error" in plain:
                raise AssertionError(f"Official browser tool failed:\n{plain[-6000:]}")
            # DB previews can show the answer before the CLI has exited. Do not
            # turn the successful/error test into an accidental cancellation.
            if result_seen and settled and not exiting:
                os.write(master, b"/exit\r")
                exiting = True
            if proc.poll() is not None:
                break
        assert exiting, f"Missing terminal result {marker!r}:\n{plain[-6000:]}"
        proc.wait(timeout=5)
        assert proc.returncode == 0, proc.returncode
        return plain + "\n" + "\n".join(snapshots)
    finally:
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
        os.close(master)


def fake_checks():
    with tempfile.TemporaryDirectory(prefix="zcode-browser-fake-") as folder:
        home = Path(folder)
        chrome = home / "fake chrome"
        chrome.write_text("#!/bin/sh\nexit 0\n")
        chrome.chmod(0o700)
        cli = home / "kernel"
        cli.write_text("""#!/usr/bin/env python3
import json,os,sys,time
from pathlib import Path
if sys.argv[1:] == ['version']:
 print('0.16.5');sys.exit()
if sys.argv[1] == 'app-server':
 Path('unexpected-app-server').touch();sys.exit(1)
Path('args.json').write_text(json.dumps(sys.argv[1:]))
time.sleep(1.5)
if os.environ.get('FAKE_BROWSER_ERROR'):
 print('Managed headless Chromium is unavailable: launch failed.',file=sys.stderr,flush=True)
 sys.exit(1)
if os.environ.get('FAKE_BROWSER_HANG'):
 time.sleep(60)
print(json.dumps({'response':'browser-fake-ok','sessionId':'sess_browser'}),flush=True)
""")
        cli.chmod(0o700)
        plain = drive_tui(home, cli, chrome, "browser-fake-ok")
        assert "waiting for model/tools" in plain
        args = json.loads((home / "args.json").read_text())
        assert args[args.index("--browser-executable") + 1] == str(chrome)
        assert args[args.index("--mode") + 1] == "build"
        assert not (home / "unexpected-app-server").exists()
        print("PASS fake: preflight, progress without DB, safe default, no app-server")
        (home / "args.json").unlink()
        plain = drive_tui(home, cli, home / "missing", "browser unavailable")
        assert "executable missing" in plain
        assert not (home / "args.json").exists()
        print("PASS fake: missing executable rejected before CLI prompt")
        plain = drive_tui(home, cli, chrome, "zcode --prompt failed", extra_env={"FAKE_BROWSER_ERROR": "1"})
        assert "Chromium launch failed" in plain
        print("PASS fake: actionable launch error survives in transcript")
        plain = drive_tui(home, cli, chrome, "zcode --prompt cancelled",
                          extra_env={"FAKE_BROWSER_HANG": "1"},
                          cancel_when=lambda: (home / "args.json").exists())
        assert "cancelled" in plain
        print("PASS fake: cancellation remains responsive")


BOOTSTRAP = """const root = process.env.ZCODE_PLUGIN_ROOT ?? process.env.CLAUDE_PLUGIN_ROOT;
if (!root) throw new Error('Browser plugin root is unavailable in the node_repl host');
const { join } = await import('node:path'); const { pathToFileURL } = await import('node:url');
const { setupBrowserRuntime } = await import(pathToFileURL(join(root, 'scripts', 'browser-client.mjs')).href);
await setupBrowserRuntime({ globals: globalThis });
"""


class LocalModel(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.path == "/clicked":
            self.server.clicked.set()
            payload = b"ok"
        else:
            self.server.visited.set()
            time.sleep(1)  # Keep a real in-flight navigation visible to the TUI.
            payload = (f'<h1>Browser fixture {self.server.token}</h1>'
                       '<button onclick="document.querySelector(\'h1\').textContent=\'Checked '
                       + self.server.token + '\';fetch(\'/clicked\')">Run check</button>').encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        try:
            self.wfile.write(payload)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_POST(self):
        try:
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            results = [b for m in request.get("messages", [])
                       for b in (m.get("content", []) if isinstance(m.get("content"), list) else [])
                       if b.get("type") == "tool_result"]
            self.server.results = results
            failed = any(b.get("is_error") for b in results)
            if failed:
                content = [{"type": "text", "text": "browser-tool-error"}]
            else:
                step = len(results)
                browser_id = None
                if step:
                    descriptors = json.loads(results[0]["content"])
                    assert descriptors and descriptors[0]["metadata"]["headless"] == "true"
                    browser_id = descriptors[0]["id"]
                select_browser = f"const browser = await agent.browsers.get({json.dumps(browser_id)});\n"
                if step == 0:
                    code = "console.log(await agent.browsers.list());"
                elif step == 1:
                    code = select_browser + "console.log(await browser.tabs.list());"
                elif step == 2:
                    assert json.loads(results[1]["content"]) == []
                    code = select_browser + f"""const tab = await browser.tabs.new();
await tab.goto('http://127.0.0.1:{self.server.server_port}/');
await tab.playwright.waitForLoadState({{state:'domcontentloaded'}});
console.log(JSON.stringify({{tabId:tab.id,snapshot:await tab.playwright.domSnapshot()}}));"""
                elif step == 3:
                    observation = json.loads(results[2]["content"])
                    assert self.server.token in observation["snapshot"]
                    assert "Run check" in observation["snapshot"]
                    code = select_browser + "console.log(await browser.tabs.list());"
                elif step == 4:
                    observation = json.loads(results[2]["content"])
                    tabs = json.loads(results[3]["content"])
                    assert any(t["id"] == observation["tabId"] for t in tabs)
                    code = select_browser + f"""const tab = await browser.tabs.get({json.dumps(observation['tabId'])});
await tab.playwright.getByRole('button',{{name:'Run check',exact:true}}).click();
console.log(await tab.playwright.domSnapshot());"""
                else:
                    assert "Checked " + self.server.token in results[4]["content"]
                    assert self.server.clicked.is_set()
                    self.server.verified = True
                    code = None
                content = ([{"type": "tool_use", "id": f"browser_{step}", "name": "mcp__node_repl__js",
                             "input": {"code": BOOTSTRAP + code, "title": "验证本地浏览器", "timeout_ms": 30000}}]
                           if code else [{"type": "text", "text": "browser-e2e-ok"}])
            reason = "tool_use" if content[0]["type"] == "tool_use" else "end_turn"
            message = {"id": "msg_browser", "type": "message", "role": "assistant", "model": "browser-model",
                       "content": content, "stop_reason": reason, "stop_sequence": None,
                       "usage": {"input_tokens": 10, "output_tokens": 3}}
            if request.get("stream"):
                events = [{"type": "message_start", "message": {**message, "content": [], "stop_reason": None}}]
                for i, block in enumerate(content):
                    tool = block["type"] == "tool_use"
                    start = {**block, "input": {}} if tool else {"type": "text", "text": ""}
                    delta = ({"type": "input_json_delta", "partial_json": json.dumps(block["input"])} if tool
                             else {"type": "text_delta", "text": block["text"]})
                    events += [{"type": "content_block_start", "index": i, "content_block": start},
                               {"type": "content_block_delta", "index": i, "delta": delta},
                               {"type": "content_block_stop", "index": i}]
                events += [{"type": "message_delta", "delta": {"stop_reason": reason, "stop_sequence": None},
                            "usage": {"output_tokens": 3}}, {"type": "message_stop"}]
                payload = "".join(f"event: {e['type']}\ndata: {json.dumps(e)}\n\n" for e in events).encode()
            else:
                payload = json.dumps(message).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream" if request.get("stream") else "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception as error:
            self.server.errors.append(repr(error))
            self.send_error(500)


def official_check(kernel, browser, scenario):
    with tempfile.TemporaryDirectory(prefix="zcode-browser-official-") as folder:
        home = Path(folder)
        wrapper = install_wrapper(home, BIN)
        server = ThreadingHTTPServer(("127.0.0.1", 0), LocalModel)
        server.token = uuid.uuid4().hex
        server.clicked = threading.Event()
        server.visited = threading.Event()
        server.results, server.errors, server.verified = [], [], False
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        config = {"provider": {"compat": {"kind": "anthropic", "name": "Isolated browser check",
                  "options": {"baseURL": f"http://127.0.0.1:{server.server_port}", "apiKey": "not-a-real-key"},
                  "models": {"browser-model": {"name": "Local model"}}}},
                  "model": {"main": "compat/browser-model", "lite": "compat/browser-model"}}
        (home / ".zcode/cli").mkdir(parents=True)
        (home / ".zcode/cli/config.json").write_text(json.dumps(config))
        env = {"ZCODE_APP": str(kernel.parents[2])}
        success = scenario in ("success", "electron")
        if scenario == "electron":
            env["ZCODE_FORCE_SYSTEM_NODE"] = ""
        if scenario == "missing-runtime":
            env["ZCODE_TUI_BROWSER_RUNTIME"] = "off"
        marker = "browser-e2e-ok" if success else "browser-tool-error"
        if scenario == "cancel":
            marker = "zcode --prompt cancelled"
        try:
            plain = drive_tui(home, wrapper, browser, marker,
                              mode="build" if scenario == "approval" else "yolo", extra_env=env,
                              cancel_when=server.visited.is_set if scenario == "cancel" else None)
            assert not server.errors, server.errors
            if success:
                assert server.verified and server.clicked.is_set(), server.results
                assert "waiting for model/tools" in plain
                assert "mcp__node_repl__js" in plain
                with sqlite3.connect(home / ".zcode/cli/db/db.sqlite") as conn:
                    assert conn.execute("SELECT COUNT(*) FROM tool_usage").fetchone()[0] >= 5
            elif scenario == "missing-runtime":
                assert "official Playwright runtime unavailable" in plain
            elif scenario == "approval":
                assert "cannot ask for tool approval" in plain
            else:
                assert server.visited.is_set() and "cancelled" in plain
            print(f"PASS official: {scenario} (real kernel, isolated HOME, local model/page)")
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
            check_cleanup(home)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--official", action="store_true")
    parser.add_argument("--kernel", type=Path, default=os.environ.get("ZCODE_TEST_CJS"))
    parser.add_argument("--browser", type=Path, default="/usr/bin/google-chrome")
    args = parser.parse_args()
    fake_checks()
    if args.official:
        if not args.kernel or not args.kernel.is_file():
            parser.error("--official requires --kernel /path/to/resources/glm/zcode.cjs")
        for scenario in ["success", "electron", "missing-runtime", "approval", "cancel"]:
            official_check(args.kernel.resolve(), args.browser.resolve(), scenario)
