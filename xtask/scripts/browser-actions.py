#!/usr/bin/env python3
"""Give wasm browser tests trusted WebDriver presses in their own Chrome session.

The runner uses this loopback proxy as CHROMEDRIVER_REMOTE. Tests request only
coordinates and a press count; the proxy sends W3C pointer actions to the same
session. No DOM event is manufactured and no double-click interval is invented.
Only the previously approved cached ChromeDriver is used, never downloaded here.
This is test infrastructure outside every shipped web/package asset.
"""
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

SESSION = None
DRIVER = None


def request(method, path, body=None):
    """Preserve driver status and JSON, including action failures."""
    req = urllib.request.Request(DRIVER + path, data=body, method=method,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


class Proxy(http.server.BaseHTTPRequestHandler):
    """Relay runner commands and the narrow same-session action request."""

    def do_GET(self):
        """Only the test origin can request trusted presses from browser JS."""
        if self.path.startswith("/press?"):
            if self.headers.get("Origin") != "http://127.0.0.1:8088" or not SESSION:
                self.reply(403, b'{"error":"test session required"}')
                return
            fields = urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query)
            try:
                x, y = int(fields["x"][0]), int(fields["y"][0])
                count = int(fields["count"][0])
                if count not in (2, 3):
                    raise ValueError("only double/triple presses")
                # Consecutive actions let Chrome use its normal OS click-count policy.
                actions = [{"type": "pointerMove", "duration": 0,
                            "origin": "viewport", "x": x, "y": y}]
                for _ in range(count):
                    actions += [{"type": "pointerDown", "button": 0},
                                {"type": "pointerUp", "button": 0}]
                payload = {"actions": [{"type": "pointer", "id": "review-mouse",
                           "parameters": {"pointerType": "mouse"}, "actions": actions}]}
                status, body = request("POST", f"/session/{SESSION}/actions",
                                       json.dumps(payload).encode())
                self.reply(status, body)
            except (ValueError, KeyError) as error:
                self.reply(400, json.dumps({"error": str(error)}).encode())
        else:
            self.relay("GET")

    def do_POST(self):
        """Capture the runner-owned session ID rather than opening another browser."""
        self.relay("POST")

    def do_DELETE(self):
        """The runner retains its normal session-disposal path."""
        self.relay("DELETE")

    def relay(self, method):
        """Forward ordinary WebDriver protocol bytes unchanged."""
        global SESSION
        size = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(size) if size else None
        status, data = request(method, self.path, body)
        if method == "POST" and self.path == "/session" and status == 200:
            parsed = json.loads(data)
            # The runner's legacy capabilities request can return the Wire form.
            SESSION = parsed.get("sessionId") or parsed.get("value", {}).get("sessionId")
        self.reply(status, data)

    def reply(self, status, body):
        """CORS is restricted to the existing browser test origin."""
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "http://127.0.0.1:8088")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        """No private URLs, session IDs or per-action chatter enter test output."""


def main():
    """Start an owned driver/proxy, run wasm-pack, then reap both on every exit."""
    global DRIVER
    cached = sorted((Path.home() / "Library/Caches/.wasm-pack").glob("chromedriver-*/chromedriver"))
    executable = os.environ.get("CHROMEDRIVER") or (str(cached[-1]) if cached else None)
    if not executable:
        raise RuntimeError("approved cached ChromeDriver missing; no installation attempted")
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    DRIVER = f"http://127.0.0.1:{port}"
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 8094), Proxy)
    with open("target/m8-actions-driver.log", "w") as log:
        driver = subprocess.Popen([executable, f"--port={port}"], stdout=log, stderr=log)
        try:
            for _ in range(100):
                try:
                    if request("GET", "/status")[0] == 200:
                        break
                except urllib.error.URLError:
                    time.sleep(0.05)
            else:
                raise RuntimeError("owned ChromeDriver did not become ready")
            threading.Thread(target=server.serve_forever, daemon=True).start()
            env = dict(os.environ, CHROMEDRIVER_REMOTE="http://127.0.0.1:8094",
                       PUZZLE_TEST_ACTIONS_URL="http://127.0.0.1:8094/press")
            return subprocess.run(["wasm-pack", *sys.argv[1:]], env=env).returncode
        finally:
            # A runner crash must close only the session this proxy opened.
            if SESSION:
                try:
                    request("DELETE", f"/session/{SESSION}")
                except urllib.error.URLError:
                    pass
            server.server_close()
            driver.terminate()
            driver.wait(timeout=10)


if __name__ == "__main__":
    sys.exit(main())
