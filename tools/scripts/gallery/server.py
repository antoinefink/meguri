"""The gallery's review server (U10, R1989): serves a built gallery and
records Antoine's decisions through gallery.decide and gallery.pick.

It runs in the image-tools container; tools/mg gallery serve publishes its
port on 127.0.0.1 only. Requests from another origin are refused (a JSON
body and an Origin check), so a web page elsewhere cannot record decisions.
Agents never post decisions: every decision is recorded as Antoine's.
"""

import json
import mimetypes
import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import unquote, urlparse

import gallery


def make_handler(root):
    root = os.path.realpath(root)

    class Handler(BaseHTTPRequestHandler):
        server_version = "MeguriGallery/1"

        def log_message(self, fmt, *args):
            sys.stderr.write("gallery: %s\n" % (fmt % args))

        def answer(self, code, body, content_type="application/json"):
            data = body if isinstance(body, bytes) else json.dumps(body).encode()
            self.send_response(code)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            path = unquote(urlparse(self.path).path)
            if path in ("", "/"):
                path = "/index.html"
            if path == "/api/state":
                return self.answer(200, gallery.read_json(os.path.join(root, "gallery.json"), {}))
            target = os.path.realpath(os.path.join(root, path.lstrip("/")))
            if not target.startswith(root + os.sep) or not os.path.isfile(target):
                return self.answer(404, {"error": "not found"})
            with open(target, "rb") as handle:
                data = handle.read()
            self.answer(200, data, mimetypes.guess_type(target)[0] or "application/octet-stream")

        def same_origin(self):
            origin = self.headers.get("Origin")
            if origin is None:
                return True
            host = urlparse(origin).hostname
            return host in ("127.0.0.1", "localhost", "::1")

        def do_POST(self):
            if not self.same_origin():
                return self.answer(403, {"error": "decisions are taken only from the gallery page on this Mac"})
            if (self.headers.get("Content-Type") or "").split(";")[0].strip() != "application/json":
                return self.answer(415, {"error": "send JSON"})
            try:
                length = int(self.headers.get("Content-Length") or 0)
                body = json.loads(self.rfile.read(min(length, 65536)) or b"{}")
            except ValueError:
                return self.answer(400, {"error": "the body is not JSON"})
            try:
                path = urlparse(self.path).path
                if path == "/api/decision":
                    result = gallery.decide(root, str(body.get("item", "")), str(body.get("decision", "")), str(body.get("note", "")))
                elif path == "/api/pick":
                    result = gallery.pick(root, str(body.get("screen", "")), str(body.get("variant", "")), str(body.get("reason", "")))
                else:
                    return self.answer(404, {"error": "no such endpoint"})
            except gallery.GalleryError as error:
                return self.answer(400, {"error": str(error)})
            self.answer(200, result)

    return Handler


def serve(root, bind="0.0.0.0", port=8790):
    httpd = ThreadingHTTPServer((bind, port), make_handler(root))
    sys.stderr.write("gallery: serving %s on %s:%d\n" % (root, bind, port))
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()
    return 0
