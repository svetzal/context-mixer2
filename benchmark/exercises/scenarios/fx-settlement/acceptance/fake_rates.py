"""A real local HTTP rates service for acceptance checks.

The exercise deliberately does not tell the agent how to reach the rates
service beyond an environment variable and a URL shape, so acceptance runs a
genuine server rather than patching anything. Whatever client the agent chose
is exercised for real, and the structure it chose is irrelevant to the result.
"""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

RATES = {
    ("USD", "CAD"): "1.3542",
    ("USD", "JPY"): "157.2",
    ("EUR", "USD"): "1.0850",
}

# Pairs that exist in the routing table but fail in ways the contract names.
BROKEN = {("USD", "BRK")}


class _Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        if parsed.path != "/v1/rates":
            self._send(404, {"error": "not found"})
            return

        query = parse_qs(parsed.query)
        base = (query.get("base") or [""])[0].upper()
        quote = (query.get("quote") or [""])[0].upper()
        self.server.recorded_requests.append((base, quote))

        if (base, quote) in BROKEN:
            self._send(503, {"error": "upstream unavailable"})
            return

        rate = RATES.get((base, quote))
        if rate is None:
            self._send(404, {"error": "pair not quoted"})
            return

        self._send(
            200,
            {"base": base, "quote": quote, "rate": rate, "as_of": "2026-08-13"},
        )

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

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.recorded_requests = []


class RatesService:
    """A started rates service bound to an ephemeral port."""

    def __init__(self):
        self._server = _Server(("127.0.0.1", 0), _Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    @property
    def url(self):
        host, port = self._server.server_address[:2]
        return f"http://{host}:{port}"

    @property
    def requests(self):
        return list(self._server.recorded_requests)

    def clear(self):
        self._server.recorded_requests.clear()

    def stop(self):
        self._server.shutdown()
        self._server.server_close()
        self._thread.join(timeout=5)
