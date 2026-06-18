#!/usr/bin/env python3
# Minimal static file server that adds permissive CORS headers, so the vendor origin X can serve its
# wasm / engine / coordinator / loader to a cross-origin integrator page (Y). Usage:
#   python3 cors-server.py <port> <directory>
import functools
import http.server
import sys

port = int(sys.argv[1])
directory = sys.argv[2]


class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        # Allow any origin to fetch these assets (module imports, importScripts, wasm, worker bootstrap).
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cross-Origin-Resource-Policy", "cross-origin")
        super().end_headers()


# Serve .wasm and .js with correct MIME types (module workers / WebAssembly.instantiateStreaming care).
Handler.extensions_map.update({".wasm": "application/wasm", ".js": "text/javascript", ".mjs": "text/javascript"})

httpd = http.server.ThreadingHTTPServer(("", port), functools.partial(Handler, directory=directory))
print(f"serving {directory} on http://localhost:{port}/  (CORS: *)")
httpd.serve_forever()
