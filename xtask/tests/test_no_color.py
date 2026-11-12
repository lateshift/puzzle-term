#!/usr/bin/env python3
"""Real xtask → Cargo → native app/bridge → PTY shell environment regression.

Run explicitly with `python3 xtask/tests/test_no_color.py` on macOS: the desktop
cases briefly open self-closing windows. No user dotfiles or shell settings are
read. Only each subprocess's environment changes; the runner stays untouched.
The bridge uses an ephemeral loopback port and the existing authenticated
protocol. Its private token never enters test output or files.
"""
import base64
import os
from pathlib import Path
import re
import selectors
import signal
import socket
import subprocess
import tempfile
import time
import unittest
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[2]
# Includes the original offending value, an empty value, and a non-boolean value.
STATES = (None, "", "1", "false", "user-choice")


def wait_for_report(process, path):
    """Wait for a complete shell witness, bounded independently of GPU startup."""
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if path.exists():
            return path.read_bytes()
        if process.poll() is not None:
            raise AssertionError("child exited before its shell reported NO_COLOR")
        time.sleep(0.01)
    raise AssertionError("shell did not report NO_COLOR within 30 seconds")


def bridge_url(process):
    """Read startup privately; never include a token-bearing line in failures."""
    deadline = time.monotonic() + 120
    data = b""
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        while time.monotonic() < deadline:
            for key, _ in selector.select(0.1):
                chunk = os.read(key.fileobj.fileno(), 4096)
                if not chunk:
                    raise AssertionError("bridge exited before reporting its bound port")
                data += chunk
                match = re.search(rb"ws://127\.0\.0\.1:[0-9]+/pty\?token=[0-9a-f]+", data)
                if match:
                    return match.group().decode("ascii")
    raise AssertionError("bridge did not start within 120 seconds")


def start_bridge_shell(url):
    """Authenticate a real WebSocket and send protocol-v1 hello to spawn the PTY."""
    parsed = urlsplit(url)
    peer = socket.create_connection((parsed.hostname, parsed.port), timeout=5)
    try:
        key = base64.b64encode(os.urandom(16)).decode("ascii")
        request = (
            f"GET {parsed.path}?{parsed.query} HTTP/1.1\r\n"
            f"Host: {parsed.netloc}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
            "Origin: http://127.0.0.1:8080\r\n\r\n"
        )
        peer.sendall(request.encode("ascii"))
        response = b""
        while b"\r\n\r\n" not in response:
            chunk = peer.recv(4096)
            if not chunk:
                raise AssertionError("bridge closed before WebSocket upgrade")
            response += chunk
        if not response.startswith(b"HTTP/1.1 101 "):
            raise AssertionError("bridge denied the test WebSocket upgrade")
        payload = bytes((3, 1, 0))  # hello, version 1, dark appearance
        mask = os.urandom(4)
        masked = bytes(value ^ mask[i % 4] for i, value in enumerate(payload))
        peer.sendall(bytes((0x82, 0x80 | len(payload))) + mask + masked)
        return peer
    except BaseException:
        peer.close()
        raise


def stop(process):
    """Stop only this test's new process group, including Cargo's bridge child."""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=10)


@unittest.skipUnless(os.uname().sysname == "Darwin", "live desktop regression uses macOS Metal")
class NoColorInheritance(unittest.TestCase):
    """Both real launch paths preserve unset, empty, and supplied values exactly."""

    def check_path(self, mode):
        for value in STATES:
            with self.subTest(mode=mode, no_color=value), tempfile.TemporaryDirectory() as directory:
                path = Path(directory)
                shell = path / "shell"
                report = path / "report"
                # Ignore -l from the production login-shell argv; no profiles are read.
                # Atomic rename prevents the reader from seeing a partial two-line value.
                shell.write_text(
                    '#!/bin/sh\n'
                    'printf "%s\\n%s" "${NO_COLOR+x}" "${NO_COLOR-}" > "$PUZZLE_ENV_OUT.tmp"\n'
                    'mv "$PUZZLE_ENV_OUT.tmp" "$PUZZLE_ENV_OUT"\n'
                    'printf "environment reported\\n"\nexit 0\n'
                )
                shell.chmod(0o700)
                env = os.environ.copy()
                env.update(SHELL=str(shell), PUZZLE_ENV_OUT=str(report))
                env.pop("NO_COLOR", None)
                if value is not None:
                    env["NO_COLOR"] = value
                args = ["cargo", "xtask", mode]
                if mode == "bridge":
                    args += ["--port", "0", "--shell", str(shell)]
                with (path / "stderr").open("wb") as stderr:
                    process = subprocess.Popen(
                        args, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                        stderr=stderr, start_new_session=True,
                    )
                    peer = None
                    try:
                        if mode == "bridge":
                            peer = start_bridge_shell(bridge_url(process))
                        actual = wait_for_report(process, report)
                        expected = b"\n" if value is None else b"x\n" + value.encode()
                        self.assertEqual(actual, expected)
                        if mode == "run":
                            self.assertEqual(process.wait(timeout=30), 0)
                    finally:
                        if peer is not None:
                            peer.close()
                        stop(process)
                        process.stdout.close()

    def test_desktop_shell_preserves_no_color(self):
        self.check_path("run")

    def test_bridge_shell_preserves_no_color(self):
        self.check_path("bridge")


if __name__ == "__main__":
    unittest.main(verbosity=2)
