# SPDX-License-Identifier: AGPL-3.0-or-later
"""Media fetched from a URL (specs/conformance/editor-ops.md E1-E5).

setup.http_fixtures: [{path, status?, content_type?, text | fill?, delay_ms?,
    byte_delay_ms?, redirect_to?}]: served by a loopback HTTP server the session
    starts (a session resource, so a resumed segment serves them too); a
    redirect status without redirect_to is sent without Location
setup.url_files: [{name, text | fill}]: written to <session dir>/url-files
add_media_from_url (op): {url} -> {name: the name the call returned};
    rejection fetch_failed

The op is Anki's single collection-library call that the page editor uses for
a fetch (download, derive a name, add the file). Its response carries either
the name or an error text; the text is localized and never recorded (R1887),
so every error text is the one kind fetch_failed. A failure of the add itself
is a backend error and stays a run error, as for add_media.

`{fixture}` at the start of a URL becomes the server's origin
(http://127.0.0.1:<port>) and `{files}` the file URL of the url-files folder;
any other URL is invalid_scenario (no host or port in scenarios, R1876).

The server is a ThreadingHTTPServer on loopback in the worker process (the
backend call releases the GIL). A request matches the entry whose path equals
the request target before its first `?`, byte for byte; any other path is 404
with no body. Waits are time.sleep slices (monotonic, never faked), never a
timed lock or event; a client that gave up (Anki's 30 s limit) breaks the
pipe, which the handler ignores. Request logging is off: worker stderr
reaches implementer-readable logs.
"""

from __future__ import annotations

import os
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

from ..errors import Rejected, invalid
from ..registry import op, session_resource, setup_section
from .media import _contents, _data, _is_int, _name, _one_of_text_fill, _write

FIXTURE = "{fixture}"
FILES = "{files}"
FILES_DIR = "url-files"
MAX_DELAY_MS = 35_000  # editor-ops.md E2
MAX_BYTE_DELAY_MS = 15_000
REDIRECTS = (301, 302, 303, 307, 308)
SLICE_SECS = 0.05
ENTRY_KEYS = {"path", "status", "content_type", "text", "fill", "delay_ms", "byte_delay_ms", "redirect_to"}


def _path(value: Any, what: str) -> str:
    """E2: an origin-relative path as a client sends it: ASCII from ! to ~,
    starting with /, no ? or #."""
    if (
        not isinstance(value, str)
        or not value.startswith("/")
        or any(not ("!" <= c <= "~") for c in value)
        or "?" in value
        or "#" in value
    ):
        raise invalid(f"{what} is a path of printable ASCII starting with /, without ? and #")
    return value


def _fixture(value: Any) -> dict:
    if not isinstance(value, dict) or "path" not in value or set(value) - ENTRY_KEYS:
        raise invalid("an http_fixtures entry is {path, status?, content_type?, text | fill?, delay_ms?, byte_delay_ms?, redirect_to?}")
    path = _path(value["path"], "an http_fixtures path")
    if "text" in value and "fill" in value:
        raise invalid(f"http_fixtures {path}: at most one of text and fill")
    body = _data(_contents(value)) if ("text" in value or "fill" in value) else b""
    redirect = value.get("redirect_to")
    if redirect is not None:
        redirect = _path(redirect, f"http_fixtures {path}: redirect_to")
    status = value.get("status", 302 if redirect is not None else 200)
    if not _is_int(status) or not 200 <= status <= 599:
        raise invalid(f"http_fixtures {path}: status is an integer from 200 to 599")
    if redirect is not None and status not in REDIRECTS:
        # A redirect status without redirect_to is sent with no Location.
        raise invalid(f"http_fixtures {path}: redirect_to goes only with a status of {REDIRECTS}")
    if status == 204 and body:
        raise invalid(f"http_fixtures {path}: a 204 response has no body")
    content_type = value.get("content_type")
    if content_type is not None and (
        not isinstance(content_type, str) or not content_type or any(not (" " <= c <= "~") for c in content_type)
        or content_type != content_type.strip()
    ):
        raise invalid(f"http_fixtures {path}: content_type is printable ASCII without leading or trailing spaces")
    waits = {}
    for key, limit in (("delay_ms", MAX_DELAY_MS), ("byte_delay_ms", MAX_BYTE_DELAY_MS)):
        ms = value.get(key, 0)
        if not _is_int(ms) or not 0 <= ms <= limit:
            raise invalid(f"http_fixtures {path}: {key} is an integer from 0 to {limit}")
        waits[key] = ms
    return {"path": path, "status": status, "content_type": content_type, "body": body,
            "redirect_to": redirect, **waits}


def _fixtures(entries: Any) -> dict[str, dict]:
    if not isinstance(entries, list):
        raise invalid("setup.http_fixtures is a list of entries")
    parsed: dict[str, dict] = {}
    for e in entries:
        f = _fixture(e)
        if f["path"] in parsed:
            raise invalid(f"setup.http_fixtures lists {f['path']} twice")
        parsed[f["path"]] = f
    for f in parsed.values():
        if f["redirect_to"] is not None and f["redirect_to"] not in parsed:
            raise invalid(f"http_fixtures {f['path']}: redirect_to {f['redirect_to']} is not a fixture path")
    return parsed


@setup_section("http_fixtures", order=95)
def http_fixtures(session, entries):
    # Checked here so that a bad list fails as the setup; the server itself
    # is the session resource below.
    _fixtures(entries)


@setup_section("url_files", order=96)
def url_files(session, entries):
    if not isinstance(entries, list):
        raise invalid("setup.url_files is a list of {name, text | fill}")
    folder = os.path.join(session.dir, FILES_DIR)
    os.makedirs(folder, exist_ok=True)
    seen: set[str] = set()
    for e in entries:
        if not isinstance(e, dict) or "name" not in e or set(e) - {"name", "text", "fill"} or not _one_of_text_fill(e):
            raise invalid("a url_files entry is {name, text | fill}")
        name = _name(e["name"])
        if name in seen:
            raise invalid(f"setup.url_files lists {name!r} twice")
        seen.add(name)
        _write(os.path.join(folder, name), _contents(e))


class _Server(ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False

    def __init__(self, fixtures: dict[str, dict]):
        self.fixtures = fixtures
        self.stopping = False
        super().__init__(("127.0.0.1", 0), _Handler)

    def handle_error(self, request, client_address):
        # A client that gave up while a fixture waited: nothing to report.
        pass


class _Handler(BaseHTTPRequestHandler):
    server: _Server
    protocol_version = "HTTP/1.0"

    def log_message(self, format, *args):  # noqa: A002 - BaseHTTPRequestHandler's name
        pass

    def _wait(self, ms: int) -> bool:
        """Sleep ms in slices; False once the session closes the server."""
        end = time.monotonic() + ms / 1000
        while (left := end - time.monotonic()) > 0:
            if self.server.stopping:
                return False
            time.sleep(min(SLICE_SECS, left))
        return not self.server.stopping

    def do_GET(self):  # noqa: N802 - BaseHTTPRequestHandler's name
        fixture = self.server.fixtures.get(self.path.split("?", 1)[0])
        if fixture is None:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if not self._wait(fixture["delay_ms"]):
            return
        self.send_response(fixture["status"])
        if fixture["content_type"] is not None:
            self.send_header("Content-Type", fixture["content_type"])
        if fixture["redirect_to"] is not None:
            host, port = self.server.server_address[:2]
            self.send_header("Location", f"http://{host}:{port}{fixture['redirect_to']}")
        body = fixture["body"]
        if fixture["status"] != 204:
            self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if not fixture["byte_delay_ms"]:
            self.wfile.write(body)
            return
        for i in range(len(body)):
            self.wfile.flush()
            if not self._wait(fixture["byte_delay_ms"]):
                return
            self.wfile.write(body[i:i + 1])


class FixtureServer:
    def __init__(self, fixtures: dict[str, dict]):
        self.server = _Server(fixtures)
        host, port = self.server.server_address[:2]
        self.origin = f"http://{host}:{port}"
        self.thread = threading.Thread(target=self.server.serve_forever, name="http-fixtures", daemon=True)
        self.thread.start()

    def close(self) -> None:
        self.server.stopping = True
        self.server.shutdown()
        self.server.server_close()


@session_resource("http_fixtures")
def fixture_server(session):
    entries = (session.scenario.get("setup") or {}).get("http_fixtures")
    return None if entries is None else FixtureServer(_fixtures(entries))


def _url(session, value: Any) -> str:
    if not isinstance(value, str):
        raise invalid("add_media_from_url url is a string")
    if value.startswith(FIXTURE + "/"):
        server = session.resources.get("http_fixtures")
        if server is None:
            raise invalid("a {fixture} URL needs setup.http_fixtures")
        return server.origin + value[len(FIXTURE):]
    if value.startswith(FILES + "/"):
        folder = os.path.abspath(os.path.join(session.dir, FILES_DIR))
        return "file://" + urllib.parse.quote(folder) + value[len(FILES):]
    raise invalid("add_media_from_url url starts with {fixture}/ or {files}/ (no host or port in scenarios)")


@op("add_media_from_url", rejects=("fetch_failed",))
def add_media_from_url(session, args):
    if not isinstance(args, dict) or set(args) != {"url"}:
        raise invalid("add_media_from_url takes {url}")
    url = _url(session, args["url"])
    response = session.col._backend.add_media_from_url(url=url)
    if response.HasField("error"):
        raise Rejected("fetch_failed")
    if not response.HasField("filename"):
        raise RuntimeError("the fetch answered neither a name nor an error")
    return {"name": response.filename}

