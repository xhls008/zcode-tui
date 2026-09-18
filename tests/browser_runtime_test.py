#!/usr/bin/env python3
"""Offline checks for the installed Browser Use loader; Node >=22.5, no npm."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]


def clean_env(home):
    env = {"PATH": os.environ["PATH"], "HOME": str(home), "LANG": "C.UTF-8",
           "XDG_CACHE_HOME": str(home / ".cache"),
           "XDG_CONFIG_HOME": str(home / ".config"),
           "XDG_DATA_HOME": str(home / ".local/share"),
           "TMPDIR": str(home / "tmp"), "ZCODE_FORCE_SYSTEM_NODE": "1"}
    (home / "tmp").mkdir(exist_ok=True)
    return env


def install_wrapper(home, binary=None):
    prefix = home / "prefix with spaces"
    (prefix / "bin").mkdir(parents=True)
    target = prefix / "bin/zcode-tui"
    if binary:
        shutil.copy2(binary, target)
    else:
        target.write_text("#!/bin/sh\necho zcode-tui-test\n")
        target.chmod(0o700)
    subprocess.run(["bash", str(REPO / "install.sh"), "--prefix", str(prefix), "--no-build"],
                   env=clean_env(home), check=True, capture_output=True, text=True, timeout=20)
    return prefix / "bin/zcode"


def write_archive(path, marker="bundled", corrupt=False):
    data = f"export const marker = {json.dumps(marker)};\n".encode()
    node = {"size": len(data), "offset": "0", "integrity": {
        "algorithm": "SHA256", "hash": hashlib.sha256(data).hexdigest()}}
    header = json.dumps({"files": {"node_modules": {"files": {
        "playwright-core": {"files": {"index.mjs": node}}}}}}).encode()
    padding = b"\0" * (-len(header) % 4)
    size = len(header) + len(padding) + 8
    path.write_bytes(struct.pack("<4I", 4, size, size - 4, len(header)) + header + padding
                     + (b"!" + data[1:] if corrupt else data))


class RuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="zcode-runtime-test-")
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.wrapper = install_wrapper(self.home)
        self.resources = self.home / "app/resources"
        (self.resources / "glm").mkdir(parents=True)
        (self.resources / "glm/zcode.cjs").write_text(
            "import('playwright-core').then(m => console.log(m.marker))"
            ".catch(e => {console.error(e.message); process.exitCode=1});\n")
        self.env = clean_env(self.home)
        self.env["ZCODE_APP"] = str(self.resources.parent)

    def run_kernel(self, *args):
        return subprocess.run([str(self.wrapper), *args], env=self.env,
                              text=True, capture_output=True, timeout=15)

    def test_extracts_and_reuses_matching_runtime_with_spaces(self):
        write_archive(self.resources / "app.asar")
        for _ in range(2):
            result = self.run_kernel("--browser-use", "headless")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), "bundled")
        self.assertEqual(len(list((self.home / ".cache/zcode-tui/browser-runtime").glob("*/.complete"))), 1)
        self.assertFalse((self.resources / "glm/node_modules").exists())

    def test_upgrade_uses_new_archive_not_stale_runtime(self):
        write_archive(self.resources / "app.asar", "old")
        self.assertEqual(self.run_kernel("--browser-use=headless").stdout.strip(), "old")
        write_archive(self.resources / "app.asar", "new")
        self.assertEqual(self.run_kernel("--browser-use=headless").stdout.strip(), "new")

    def test_existing_runtime_is_not_overridden(self):
        package = self.resources / "glm/node_modules/playwright-core"
        package.mkdir(parents=True)
        (package / "package.json").write_text('{"type":"module","exports":"./index.mjs"}')
        (package / "index.mjs").write_text('export const marker = "native";')
        result = self.run_kernel("--browser-use", "headless")
        self.assertEqual(result.stdout.strip(), "native", result.stderr)
        self.assertFalse((self.home / ".cache/zcode-tui/browser-runtime").exists())

    def test_no_browser_and_explicit_opt_out_do_not_install_hook(self):
        write_archive(self.resources / "app.asar")
        self.assertNotEqual(self.run_kernel("version").returncode, 0)
        self.env["ZCODE_TUI_BROWSER_RUNTIME"] = "off"
        self.assertNotEqual(self.run_kernel("--browser-use", "headless").returncode, 0)
        self.assertFalse((self.home / ".cache/zcode-tui/browser-runtime").exists())

    def test_missing_or_corrupt_package_is_actionable_not_silently_replaced(self):
        result = self.run_kernel("--browser-use", "headless")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("bundled runtime repair failed", result.stderr)
        write_archive(self.resources / "app.asar", corrupt=True)
        result = self.run_kernel("--browser-use", "headless")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("integrity mismatch", result.stderr)
        self.assertFalse(list((self.home / ".cache").rglob(".complete")))

    def test_new_provider_config_route_and_explicit_override(self):
        config = self.resources / "config/provider/zcode-builtin.json"
        config.parent.mkdir(parents=True)
        config.write_text('{}')
        (self.resources / "glm/zcode.cjs").write_text(
            '// "workspace/readPresentation"\n'
            'console.log(JSON.stringify({builtin:process.env.ZCODE_BUILTIN_PROVIDER_CONFIG_FILE, '
            'appServer:process.env.ZCODE_TUI_APP_SERVER}));')
        result = self.run_kernel("version")
        self.assertEqual(json.loads(result.stdout), {"builtin": str(config), "appServer": "0"})
        self.env["ZCODE_BUILTIN_PROVIDER_CONFIG_FILE"] = "/explicit/config.json"
        self.assertEqual(json.loads(self.run_kernel("version").stdout)["builtin"], "/explicit/config.json")
        self.env["ZCODE_TUI_CLIENT"] = "1"
        result = self.run_kernel("app-server")
        self.assertEqual(result.returncode, 64)
        self.assertIn("classic CLI", result.stderr)

    def test_upload_check_bypasses_kernel_detection_and_execution(self):
        (self.resources / "glm/zcode.cjs").write_text('throw Error("must not run");')
        self.env["ZCODE_APP"] = "/not-installed"
        result = self.run_kernel("check-zhipu-upload", "--json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "zcode-tui-test")

    def test_uninstall_removes_managed_helper(self):
        subprocess.run(["bash", str(REPO / "install.sh"), "--prefix", str(self.wrapper.parent.parent), "--uninstall"],
                       env=self.env, check=True, capture_output=True, timeout=15)
        self.assertFalse(self.wrapper.exists())
        self.assertFalse((self.wrapper.parent.parent / "lib/zcode-tui/browser-runtime.mjs").exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
