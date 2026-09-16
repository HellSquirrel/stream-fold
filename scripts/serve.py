#!/usr/bin/env python3
"""Dev server for www/: like `python3 -m http.server`, plus `Cache-Control:
no-cache` so a plain reload always revalidates the shim, the bundles and
the generated fragments. Without it a browser may keep yesterday's
`logfold.mjs` through a normal reload. Usage: `scripts/serve.py [port]`."""
import http.server, os, sys

class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                      ".mjs": "text/javascript", ".js": "text/javascript", ".wasm": "application/wasm"}

    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

    def log_message(self, fmt, *args):  # quiet; errors still print via log_error
        if args and str(args[1]).startswith(("4", "5")):
            super().log_message(fmt, *args)

port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "www"))
print(f"serving www/ at http://127.0.0.1:{port}/ (no-cache)")
http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
