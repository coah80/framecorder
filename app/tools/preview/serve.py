#!/usr/bin/env python3
"""Serves app/ui with tauri mocked, to look at the app in a browser.

    python3 tools/preview/serve.py [port]      then open http://localhost:8765/?s=connected
"""
import http.server
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
UI = HERE.parent.parent / "ui"
MOCK = '<script src="/__mock.js"></script>\n  <script type="module"'


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(UI), **kwargs)

    def send(self, body, kind):
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/__mock.js":
            return self.send((HERE / "mock.js").read_bytes(), "text/javascript")
        if path in ("/", "/index.html"):
            page = (UI / "index.html").read_text().replace('<script type="module"', MOCK, 1)
            return self.send(page.encode(), "text/html; charset=utf-8")
        return super().do_GET()

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    print(f"http://localhost:{port}/?s=connected")
    http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
