"""mitmdump addon: record sync exchanges with their bodies decoded (R1933).

Loaded by the `mitmdump` compose service (spec-record profile). Each exchange
that passes through the reverse proxy becomes one JSON line in
`<record_dir>/exchanges.jsonl`, in the order the responses complete:

    {"seq": 1, "method": "POST", "path": "/sync/meta",
     "request": {"headers": {...}, "body": <body>},
     "response": {"status": 200, "headers": {...}, "body": <body>},
     "error": null}

- `headers` keeps only the `anki-*` headers and `content-type`, names in lower
  case; an `anki-sync` value that is JSON is stored parsed.
- `<body>` is `{"zstd": <bool>, ...}` plus exactly one of `json` (the parsed
  value), `text` (UTF-8 that is not JSON, or any `text/*` body) or `bytes` and
  `sha256` (anything else, such as a collection upload or a media zip, which is
  not stored). A zstd body is decompressed first, across frames.
- A flow that ends without a response (a reset, a timeout) has `response:
  null` and `error` set to mitmproxy's message.
- With `record_actor_steps` (the `sync-recorder` service, a forward proxy for
  the oracle actors of an actor scenario, specs/conformance/sync-ops.md S9),
  each request path starts with `/<actor>/<step index>`: the addon removes it
  before the request is forwarded, and the line gets two more members,
  `actor` (a string) and `step` (an integer); `path` is the forwarded path. A
  request without the prefix cannot be attributed: it is forwarded, but not
  recorded, like any exchange that could not be written.

Timestamps, session keys and host keys are recorded as they were; consumers
compare semantically and ignore them.

`exchanges.jsonl` is created, empty, when the proxy starts; if it cannot be,
mitmdump stops, so `tools/mg sync-rig up --record` fails instead of proxying
without recording. `<record_health>` reports the recorder's state as
`{"ok", "recording", "written", "errors", "last_error"}`: `ok` turns false for
good at the first exchange that could not be recorded (its `seq` is then
missing from the file). The service's health check and `tools/mg sync-rig`
read it.
"""

import hashlib
import io
import json
import logging
import os
import re
import threading

import zstandard
from mitmproxy import ctx

ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"

# `/<actor>/<step index>` before the sync path (sync-ops.md S2.1, S9).
ACTOR_STEP = re.compile(r"^/(meguri(?:_(?:[2-9]|[1-9][0-9]+))?|client_[a-z])/(0|[1-9][0-9]*)(/.*)$")


def decode_body(raw, content_type):
    raw = raw or b""
    zstd = raw[:4] == ZSTD_MAGIC
    data = raw
    if zstd:
        reader = zstandard.ZstdDecompressor().stream_reader(io.BytesIO(raw), read_across_frames=True)
        data = reader.read()
    out = {"zstd": zstd}
    # A text/* body (an error message such as "403") stays text even when it
    # happens to parse as JSON.
    if not (content_type or "").lower().startswith("text/"):
        try:
            out["json"] = json.loads(data)
            return out
        except ValueError:
            pass
    try:
        out["text"] = data.decode("utf-8")
    except UnicodeDecodeError:
        out["bytes"] = len(data)
        out["sha256"] = hashlib.sha256(data).hexdigest()
    return out


def kept_headers(headers):
    kept = {}
    for name, value in headers.items():
        name = name.lower()
        if not (name.startswith("anki-") or name == "content-type"):
            continue
        if name == "anki-sync":
            try:
                value = json.loads(value)
            except ValueError:
                pass
        kept[name] = value
    return kept


def exchange(seq, flow, actor_steps=False):
    request = flow.request
    response = flow.response
    attributed = {}
    if actor_steps:
        prefix = flow.metadata.get("record_sync_prefix")
        if prefix is None:
            raise ValueError("the request path has no /<actor>/<step index> prefix")
        attributed = {"actor": prefix[0], "step": prefix[1]}
    return attributed | {
        "seq": seq,
        "method": request.method,
        "path": request.path,
        "request": {
            "headers": kept_headers(request.headers),
            "body": decode_body(request.raw_content, request.headers.get("content-type")),
        },
        "response": None
        if response is None
        else {
            "status": response.status_code,
            "headers": kept_headers(response.headers),
            "body": decode_body(response.raw_content, response.headers.get("content-type")),
        },
        "error": None if flow.error is None else str(flow.error),
    }


class RecordSync:
    def __init__(self):
        self.lock = threading.Lock()
        self.seq = 0
        self.path = None
        self.health = {"ok": False, "written": 0, "errors": 0, "last_error": None}

    def load(self, loader):
        loader.add_option("record_dir", str, "/recordings", "directory that receives exchanges.jsonl")
        loader.add_option(
            "record_health", str, "/tmp/record-sync-health.json", "file that reports the recorder's state"
        )
        loader.add_option(
            "record_actor_steps",
            bool,
            False,
            "strip /<actor>/<step index> from each request path and record actor and step",
        )

    def report(self):
        path = ctx.options.record_health
        with open(path + ".tmp", "w", encoding="utf-8") as out:
            json.dump(dict(self.health, recording=self.path), out, sort_keys=True)
        os.replace(path + ".tmp", path)

    def fail(self, message):
        self.health["ok"] = False
        self.health["last_error"] = message
        logging.error("record_sync: %s", message)
        self.report()

    def running(self):
        self.path = os.path.join(ctx.options.record_dir, "exchanges.jsonl")
        try:
            os.makedirs(ctx.options.record_dir, exist_ok=True)
            with open(self.path, "a", encoding="utf-8"):
                pass
        except OSError as err:
            self.fail(f"cannot write {self.path}: {err}")
            ctx.master.shutdown()
            return
        self.health["ok"] = True
        self.report()

    def write(self, flow):
        with self.lock:
            self.seq += 1
            try:
                line = json.dumps(
                    exchange(self.seq, flow, ctx.options.record_actor_steps), sort_keys=True, ensure_ascii=False
                )
                with open(self.path, "a", encoding="utf-8") as out:
                    out.write(line + "\n")
            except Exception as err:  # a gap in the recording, whatever the cause
                self.health["errors"] += 1
                self.fail(f"exchange {self.seq} ({flow.request.method} {flow.request.path}) not recorded: {err!r}")
                return
            self.health["written"] += 1
            self.report()

    def request(self, flow):
        if not ctx.options.record_actor_steps:
            return
        match = ACTOR_STEP.match(flow.request.path)
        if match is None:
            return
        flow.metadata["record_sync_prefix"] = (match.group(1), int(match.group(2)))
        flow.request.path = match.group(3)

    def response(self, flow):
        self.write(flow)

    def error(self, flow):
        if flow.response is None:
            self.write(flow)


addons = [RecordSync()]
