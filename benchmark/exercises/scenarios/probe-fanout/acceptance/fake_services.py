"""A real local HTTP service with controllable behaviour.

Acceptance drives concurrency and timeout behaviour against genuine sockets
rather than patched clients, so whatever concurrency design the agent chose is
exercised as it will actually run.
"""

import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse


class _Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        query = parse_qs(parsed.query)

        if parsed.path == "/slow":
            time.sleep(float((query.get("seconds") or ["5"])[0]))
            self._send(200, {"status": "eventually"})
            return
        if parsed.path == "/error":
            self._send(500, {"status": "broken"})
            return
        if parsed.path == "/teapot":
            self._send(418, {"status": "teapot"})
            return
        if parsed.path == "/ok":
            self._send(200, {"status": "ok"})
            return
        self._send(404, {"status": "unknown"})

    def _send(self, status, body):
        payload = json.dumps(body).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args):
        """Silence the default stderr access log."""


class _Server(ThreadingHTTPServer):
    daemon_threads = True


class Services:
    """A started service bound to an ephemeral port."""

    def __init__(self):
        self._server = _Server(("127.0.0.1", 0), _Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    @property
    def url(self):
        host, port = self._server.server_address[:2]
        return f"http://{host}:{port}"

    def stop(self):
        self._server.shutdown()
        self._server.server_close()
        self._thread.join(timeout=5)


def closed_port_url():
    """A URL nothing is listening on, for the unreachable path."""
    import socket

    probe = socket.socket()
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
    probe.close()
    return f"http://127.0.0.1:{port}"
