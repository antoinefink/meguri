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

Timestamps, session keys and host keys are recorded as they were; consumers
compare semantically and ignore them.
"""

import hashlib
import io
import json
import os
import threading

import zstandard
from mitmproxy import ctx

ZSTD_MAGIC = b"\x28\xb5\x2f\xfd"


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


def exchange(seq, flow):
    request = flow.request
    response = flow.response
    return {
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

    def load(self, loader):
        loader.add_option("record_dir", str, "/recordings", "directory that receives exchanges.jsonl")

    def running(self):
        os.makedirs(ctx.options.record_dir, exist_ok=True)
        self.path = os.path.join(ctx.options.record_dir, "exchanges.jsonl")

    def write(self, flow):
        with self.lock:
            self.seq += 1
            line = json.dumps(exchange(self.seq, flow), sort_keys=True, ensure_ascii=False)
            with open(self.path, "a", encoding="utf-8") as out:
                out.write(line + "\n")

    def response(self, flow):
        self.write(flow)

    def error(self, flow):
        if flow.response is None:
            self.write(flow)


addons = [RecordSync()]
