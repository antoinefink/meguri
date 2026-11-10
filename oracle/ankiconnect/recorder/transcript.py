# SPDX-License-Identifier: AGPL-3.0-or-later
"""The pure part of the recorder: what servers/transcripts asks of an oracle
scenario, without Anki or sockets.

- check_scenario: T3 keys, and only what the recorder can run (T4, T6, T7).
- Tokens: T10.1 entity tokens over the snapshots the add-on takes after setup
  and after every exchange; T10.2 substitution in requests.
- request_bytes: T6.2 and T6.3.
- parse_reply: T8 from the raw reply bytes.
- Normalizer: T10.3 (environment values), then T10.1 (ids at T10.5's
  positions, error strings), and the T10.1 warnings for ids left in place.

Errors are RunError(kind, ...): `invalid_scenario`, `unknown_ref`,
`unsupported_step` (a form the recorder does not run yet) and the run errors
of T7.4. Their fields are recorder-authored, never Anki's text.
"""

from __future__ import annotations

import base64
import binascii
import hashlib
import json
import os
import re
from typing import Any, Callable

PORT = 8765
ID_MIN = 1_000_000_000  # T10.1: smaller ids are never tokens
KINDS = ("n", "c", "d", "m")  # notes, cards, decks, note types
KIND_NAMES = {"n": "note", "c": "card", "d": "deck", "m": "note type"}

SCENARIO_KEYS = {
    "id", "description", "tags", "server", "source", "clock", "compare",
    "server_config", "host_env", "setup", "steps", "equivalence",
}
CLOCK_KEYS = {"tz", "start", "mode"}
# T4: the members the add-on has; the others are declared only.
RECORDER_CONFIG = {"cors_origins": "webCorsOriginList", "deny_origins": "ignoreOriginList", "api_key": "apiKey"}
DECLARED_CONFIG = {"read_only", "extra_origins", "media_dir", "readable_folders", "enabled", "port"}
CONFIG_DEFAULTS = {"cors_origins": ["http://localhost"], "deny_origins": [], "api_key": None}
# T4: fixed whatever the scenario says.
FIXED_CONFIG = {"webBindAddress": "127.0.0.1", "webBindPort": PORT, "apiLogPath": None}

HTTP_KEYS = {"method", "target", "headers", "body", "connection", "no_host"}
DECLARED_HTTP = {"server", "address", "http_version", "raw", "expect_continue", "chunks", "declared_length"}
STEP_KINDS = ("http", "advance", "observe", "media_dir_write", "permission", "interaction")
DECLARED_STEPS = {
    "close", "pipeline", "open", "close_wait", "apply_config", "config", "lifecycle", "restart_host",
    "helper", "fault", "revalidate", "resolve", "events", "sockets", "logs", "media_dir_scan", "wait",
    "ws_open", "ws_send", "ws_expect", "status", "parallel",
}
PERMISSION_ANSWERS = ("allow", "deny", "deny_ignore")
TOKEN = re.compile(r"^#([ncdm])([1-9][0-9]*)$")
_SUBST = re.compile(r"\$\$\{|\$\{([^}]*)\}|\$\{")
_METHOD = re.compile(r"^[!#$%&'*+.^_`|~0-9A-Za-z-]+$")
_HEADER_NAME = _METHOD
_DIGITS = re.compile(r"[0-9]{10,}")
_FILE_NAME = re.compile(r"^[^/\x00]+$")

# T8: the compared headers.
COMPARED_HEADERS = (
    "content-type", "access-control-allow-origin", "access-control-allow-headers",
    "access-control-allow-methods", "access-control-allow-private-network", "allow",
    "upgrade", "sec-websocket-accept",
)

# T10.5: action -> [(kind, pointer pattern into the result)]. "" is the result
# itself; "#keys" marks getDeckStats' member names.
ID_POSITIONS: dict[str, list[tuple[str, str]]] = {
    "findNotes": [("n", "/*")],
    "cardsToNotes": [("n", "/*")],
    "findCards": [("c", "/*")],
    "guiBrowse": [("c", "/*")],
    "getDecks": [("c", "/*/*")],
    "addNote": [("n", "")],
    "guiAddCards": [("n", "")],
    "addNotes": [("n", "/*")],
    "guiSelectedNotes": [("n", "/*")],
    "notesInfo": [("n", "/*/noteId"), ("c", "/*/cards/*")],
    "notesModTime": [("n", "/*/noteId")],
    "cardsInfo": [("n", "/*/note"), ("c", "/*/cardId")],
    "cardsModTime": [("c", "/*/cardId")],
    "deckNamesAndIds": [("d", "/*")],
    "createDeck": [("d", "")],
    "getDeckStats": [("d", "#keys"), ("d", "/*/deck_id")],
    "modelNamesAndIds": [("m", "/*")],
    "findModelsById": [("d", "/*/did"), ("m", "/*/id")],
    "findModelsByName": [("d", "/*/did"), ("m", "/*/id")],
    "createModel": [("d", "/did"), ("m", "/id")],
}
# T10.3: note type objects whose tmpls/flds ids are random.
MODEL_LISTS = ("findModelsById", "findModelsByName")
MODEL_ONE = ("createModel",)
BIDI_MARKS = {0x2068: None, 0x2069: None}


class RunError(Exception):
    def __init__(self, kind: str, **fields: Any) -> None:
        super().__init__(kind)
        self.kind = kind
        self.fields = fields

    def to_json(self) -> dict:
        return {"kind": self.kind, **self.fields}


def invalid(detail: str, **fields: Any) -> RunError:
    return RunError("invalid_scenario", detail=detail, **fields)


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def _mapping(v: Any, what: str, step: int | None = None) -> dict:
    if not isinstance(v, dict):
        raise invalid(f"{what} is a mapping", **({} if step is None else {"step": step}))
    return v


# --- the scenario -----------------------------------------------------------------


def check_scenario(scenario: Any, expected_id: str | None) -> None:
    """T3 for an oracle scenario, and the forms the recorder runs."""
    _mapping(scenario, "a transcript scenario")
    unknown = sorted(set(scenario) - SCENARIO_KEYS)
    if unknown:
        raise invalid(f"unknown keys: {', '.join(unknown)}")
    sid = scenario.get("id")
    if not isinstance(sid, str) or not sid.startswith("servers/ankiconnect/"):
        raise invalid("id is servers/ankiconnect/<path> (T2)")
    if expected_id is not None and sid != expected_id:
        raise invalid(f"id {sid!r} is not the file's path {expected_id!r} (T2)")
    if scenario.get("server") != "ankiconnect":
        raise invalid("the recorder runs server: ankiconnect only")
    if scenario.get("source") != "oracle":
        raise invalid("the recorder runs source: oracle only; declared scenarios have no golden (T2)")
    for key in ("host_env", "equivalence"):
        if key in scenario:
            raise invalid(f"{key} is not allowed in an oracle ankiconnect scenario (T3)")
    tags = scenario.get("tags", [])
    if not isinstance(tags, list) or not all(isinstance(t, str) for t in tags):
        raise invalid("tags is a list of strings")
    clock = _mapping(scenario.get("clock"), "clock")
    if set(clock) - CLOCK_KEYS or "tz" not in clock or "start" not in clock:
        raise invalid("clock is {tz, start, mode?} (conformance/dsl D5)")
    if clock.get("mode", "frozen") not in ("frozen", "step"):
        raise invalid("clock.mode is frozen or step")
    if not isinstance(scenario.get("setup", {}), dict):
        raise invalid("setup is a mapping (T5)")
    config = _mapping(scenario.get("server_config", {}), "server_config")
    for member in sorted(config):
        if member in DECLARED_CONFIG:
            raise invalid(f"server_config.{member} is not available on the recorder: the scenario must be declared (T4)")
        if member not in RECORDER_CONFIG:
            raise invalid(f"unknown server_config member {member!r} (T4)")
    for member in ("cors_origins", "deny_origins"):
        value = config.get(member, [])
        if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
            raise invalid(f"server_config.{member} is a list of strings")
    if not (config.get("api_key") is None or isinstance(config.get("api_key"), str)):
        raise invalid("server_config.api_key is a string or null")
    steps = scenario.get("steps")
    if not isinstance(steps, list) or not steps:
        raise invalid("steps is a non-empty list (T3)")
    exchanges = 0
    for index, step in enumerate(steps):
        exchanges += check_step(step, index)
    if not exchanges:
        raise invalid("steps hold at least one exchange (T3)")


def check_step(step: Any, index: int) -> int:
    """1 for an exchange, 0 for another step the recorder runs."""
    if not isinstance(step, dict) or not step:
        raise invalid("a step is a mapping", step=index)
    keys = set(step)
    if "http" in keys:
        if keys - {"http", "expect"}:
            raise invalid("an exchange step is {http, expect?}", step=index)
        if "expect" in keys:
            raise invalid("expect is for declared scenarios only (T13)", step=index)
        check_http(step["http"], index)
        return 1
    if len(keys) != 1:
        raise invalid("a step other than an exchange has exactly one key", step=index)
    (name,) = keys
    args = step[name]
    if name in DECLARED_STEPS:
        raise invalid(f"{name} is for declared scenarios only (T7.3)", step=index)
    if name not in STEP_KINDS:
        if name == "await":
            raise RunError("unsupported_step", step=index, detail="async exchanges are not recorded yet")
        raise invalid(f"unknown step {name!r}", step=index)
    if name == "observe" and not isinstance(args, list):
        raise invalid("observe takes a list of observation requests", step=index)
    if name == "permission":
        answer = _mapping(args, "permission", index).get("answer")
        if set(args) != {"answer"} or answer not in PERMISSION_ANSWERS + ("none",):
            raise invalid("permission is {answer: allow | deny | deny_ignore | none}", step=index)
        if answer == "none":
            raise invalid("permission answer none is declared only: the add-on would wait forever (T7.2)", step=index)
    if name == "interaction":
        if set(_mapping(args, "interaction", index)) != {"kind", "answer"}:
            raise invalid("interaction is {kind, answer} (T7.5)", step=index)
    if name == "media_dir_write":
        check_media_write(args, index)
    if name == "advance" and not isinstance(args, str):
        raise invalid("advance takes a duration (conformance/dsl D7.1)", step=index)
    return 0


def check_http(http: Any, index: int) -> None:
    _mapping(http, "http", index)
    for member in sorted(set(http)):
        if member in DECLARED_HTTP:
            raise invalid(f"http.{member} is not available in oracle scenarios (T6)", step=index)
        if member == "async":
            raise RunError("unsupported_step", step=index, detail="async exchanges are not recorded yet")
        if member not in HTTP_KEYS:
            raise invalid(f"unknown http member {member!r}", step=index)
    method = http.get("method", "POST")
    if not isinstance(method, str) or not _METHOD.match(method):
        raise invalid("http.method is an HTTP token", step=index)
    if not isinstance(http.get("target", "/"), str):
        raise invalid("http.target is a string", step=index)
    if http.get("connection", "fresh") != "fresh":
        raise invalid("oracle scenarios use only fresh connections (T6.4)", step=index)
    if not isinstance(http.get("no_host", False), bool):
        raise invalid("http.no_host is a boolean", step=index)
    headers = http.get("headers", [])
    if not isinstance(headers, list):
        raise invalid("http.headers is a list of [name, value]", step=index)
    for h in headers:
        if not (isinstance(h, list) and len(h) == 2 and isinstance(h[0], str) and _HEADER_NAME.match(h[0])):
            raise invalid("each header is [name, value] with a token name", step=index)
        if not isinstance(h[1], str):
            raise invalid("header values other than strings (fill) are declared only (T6.1)", step=index)
        if h[0].lower() == "transfer-encoding":
            raise invalid("oracle scenarios never send Transfer-Encoding (T12)", step=index)
    if "body" in http:
        check_body(http["body"], index)


def check_body(body: Any, index: int, what: str = "body") -> None:
    forms = ("empty", "json", "text", "base64", "fixture", "fill")
    if not isinstance(body, dict) or len(body) != 1 or next(iter(body)) not in forms:
        raise invalid(f"{what} is one of {{{', '.join(forms)}}} (T6.3)", step=index)
    ((form, value),) = body.items()
    if form == "empty" and value is not True:
        raise invalid(f"{what}.empty is true", step=index)
    if form in ("text", "base64") and not isinstance(value, str):
        raise invalid(f"{what}.{form} is a string", step=index)
    if form == "fixture":
        if not isinstance(value, dict) or set(value) != {"path", "sha256"} or not all(isinstance(v, str) for v in value.values()):
            raise invalid(f"{what}.fixture is {{path, sha256}}", step=index)
        path = value["path"]
        if path.startswith("/") or ".." in path.split("/") or not path.startswith("servers/"):
            raise invalid(f"{what}.fixture.path is a path under conformance/fixtures/servers/", step=index)
    if form == "fill":
        if not isinstance(value, dict) or not {"size", "byte"} <= set(value) <= {"size", "byte", "prefix"}:
            raise invalid(f"{what}.fill is {{size, byte, prefix?}}", step=index)
        if not (_is_int(value["size"]) and value["size"] >= 0 and _is_int(value["byte"]) and 0 <= value["byte"] <= 255):
            raise invalid(f"{what}.fill.size is >= 0 and fill.byte 0..255", step=index)
        if not isinstance(value.get("prefix", ""), str):
            raise invalid(f"{what}.fill.prefix is a string", step=index)


def check_media_write(args: Any, index: int) -> None:
    _mapping(args, "media_dir_write", index)
    contents = [k for k in ("text", "base64", "fixture") if k in args]
    if set(args) - {"name", "text", "base64", "fixture"} or len(contents) != 1:
        raise invalid("media_dir_write is {name, text | base64 | fixture} (T7.1)", step=index)
    name = args.get("name")
    if not isinstance(name, str) or not _FILE_NAME.match(name) or name in (".", ".."):
        raise invalid("media_dir_write.name is a plain file name", step=index)
    check_body({contents[0]: args[contents[0]]}, index, "media_dir_write")


def addon_config(scenario: dict, defaults: dict) -> dict:
    """The add-on's config document for T4's server_config."""
    config = dict(defaults)
    given = {**CONFIG_DEFAULTS, **scenario.get("server_config", {})}
    for member, key in RECORDER_CONFIG.items():
        config[key] = given[member]
    config.update(FIXED_CONFIG)
    return config


# --- entity tokens (T10.1) ---------------------------------------------------------


class Tokens:
    """Per kind, the tokened entities in token order: [real id, identity,
    alive]. A token is never reused; an entity that is gone or whose identity
    changed loses its token, and its id may get a new one."""

    def __init__(self) -> None:
        self.entries: dict[str, list[list]] = {k: [] for k in KINDS}

    @staticmethod
    def identities(snapshot: dict) -> dict[str, dict[int, Any]]:
        return {
            "n": {row[0]: row[1] for row in snapshot["notes"]},
            "c": {row[0]: [row[1], row[2]] for row in snapshot["cards"]},
            "d": {row[0]: row[1] for row in snapshot["decks"]},
            "m": {row[0]: row[1] for row in snapshot["notetypes"]},
        }

    def start(self, snapshot: dict, setup_notes: list[int], setup_cards: list[int]) -> None:
        """After setup: its notes and cards in creation order (conformance/dsl
        D7.1), then everything else in ascending id order."""
        ids = self.identities(snapshot)
        for kind, created in (("n", setup_notes), ("c", setup_cards)):
            for real in created:
                if real not in ids[kind]:
                    raise RunError("internal_error", detail=f"a setup {KIND_NAMES[kind]} is not in the collection")
                self.entries[kind].append([real, ids[kind][real], True])
        self._add_new(ids)

    def update(self, snapshot: dict) -> None:
        """After an exchange: retire what is gone or changed, then number what
        has no token, in ascending id order per kind."""
        ids = self.identities(snapshot)
        for kind in KINDS:
            for entry in self.entries[kind]:
                if entry[2] and ids[kind].get(entry[0], object()) != entry[1]:
                    entry[2] = False
        self._add_new(ids)

    def _add_new(self, ids: dict[str, dict[int, Any]]) -> None:
        for kind in KINDS:
            alive = {e[0] for e in self.entries[kind] if e[2]}
            for real in sorted(i for i in ids[kind] if i not in alive and i >= ID_MIN):
                self.entries[kind].append([real, ids[kind][real], True])

    def real(self, token: str) -> int:
        m = TOKEN.match(token)
        if not m:
            raise invalid(f"{token!r} is not a token")
        kind, n = m.group(1), int(m.group(2))
        entries = self.entries[kind]
        if n > len(entries):
            raise RunError("unknown_ref", ref=token)
        if not entries[n - 1][2]:
            raise RunError("unknown_ref", ref=token, detail="that entity no longer exists")
        return entries[n - 1][0]

    def token(self, kind: str, real: int) -> str | None:
        for n, (rid, _, alive) in enumerate(self.entries[kind], 1):
            if alive and rid == real:
                return f"#{kind}{n}"
        return None

    def any_kind(self, real: int) -> list[str]:
        return [t for t in (self.token(k, real) for k in KINDS) if t]

    def alive_ids(self) -> set[int]:
        return {e[0] for k in KINDS for e in self.entries[k] if e[2]}

    def logical(self, kind: str) -> list[int]:
        """Real ids by token number (gone ones keep their last id), for the
        driver's #nN/#cN references in observations."""
        return [e[0] for e in self.entries[kind]]


class ClockCheck:
    """Every id Anki allocates is the frozen clock's milliseconds plus a small
    offset, and every note and card mod lies between clock.start and the
    clock now (T10.1 "How the recorder's ids fall"). A new id or a mod outside
    that is a clock read that escaped libfaketime: clock_escaped, never a
    golden."""

    SPAN_MS = 10_000_000

    def __init__(self, start: int) -> None:
        self.start = start
        self.seen: dict[str, set[int]] = {k: set() for k in KINDS}

    def check(self, snapshot: dict, now: int, step: int | None) -> None:
        rows = {"n": snapshot["notes"], "c": snapshot["cards"], "d": snapshot["decks"], "m": snapshot["notetypes"]}
        where = {} if step is None else {"step": step}
        for kind in KINDS:
            for row in rows[kind]:
                real = row[0]
                if real >= ID_MIN and real not in self.seen[kind]:
                    if not now * 1000 <= real < now * 1000 + self.SPAN_MS:
                        raise RunError("clock_escaped", entity=KIND_NAMES[kind], id=real, clock_ms=now * 1000,
                                       detail="a new id is not the frozen clock's", **where)
                    self.seen[kind].add(real)
                if kind in ("n", "c"):
                    mod = row[-1]
                    if not (_is_int(mod) and self.start <= mod <= now):
                        raise RunError("clock_escaped", entity=KIND_NAMES[kind], mod=mod, clock=now,
                                       detail="a mod is outside clock.start..now", **where)


# --- requests (T6.2, T6.3, T10.2) --------------------------------------------------


class Env:
    """What substitution and normalization need from this run."""

    def __init__(self, tokens: Tokens, clock_start: int, port: int = PORT) -> None:
        self.tokens = tokens
        self.clock_start = clock_start
        self.port = port
        self.fixture_base: str | None = None
        self.media_dir: str | None = None
        self.profile: str | None = None
        self.tts_needle: str | None = None
        self.fixtures_root: str | None = None


def substitute(text: str, env: Env, step: int) -> str:
    def repl(m: re.Match) -> str:
        whole = m.group(0)
        if whole == "$${":
            return "${"
        name = m.group(1)
        if name is None:
            raise invalid("an unclosed ${ in a string (T10.2; write $${ for a literal)", step=step)
        if TOKEN.match(name):
            return str(env.tokens.real(name))
        if name == "port":
            return str(env.port)
        if name == "fixture_base":
            if env.fixture_base is None:
                raise invalid("${fixture_base} needs setup.http_fixtures", step=step)
            return env.fixture_base
        raise invalid(f"unknown substitution ${{{name}}} (T10.2)", step=step)

    try:
        return _SUBST.sub(repl, text)
    except RunError as exc:
        exc.fields.setdefault("step", step)
        raise


def substitute_json(value: Any, env: Env, step: int) -> Any:
    if isinstance(value, str):
        if TOKEN.match(value):
            try:
                return env.tokens.real(value)
            except RunError as exc:
                exc.fields.setdefault("step", step)
                raise
        return substitute(value, env, step)
    if isinstance(value, list):
        return [substitute_json(v, env, step) for v in value]
    if isinstance(value, dict):
        return {substitute(k, env, step): substitute_json(v, env, step) for k, v in value.items()}
    return value


def compact_json(value: Any) -> bytes:
    """T6.3's json form: compact, members in the given order, only the JSON
    controls, quote and backslash escaped."""
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode("utf-8")


def fixture_bytes(spec: dict, env: Env, step: int) -> bytes:
    if env.fixtures_root is None:
        raise RunError("fixture_unavailable", step=step, detail="no fixtures folder is mounted (ORACLE_FIXTURES)")
    path = os.path.join(env.fixtures_root, spec["path"])
    try:
        with open(path, "rb") as f:
            data = f.read()
    except OSError:
        raise RunError("fixture_unavailable", step=step, path=spec["path"]) from None
    if hashlib.sha256(data).hexdigest() != spec["sha256"].lower():
        raise RunError("fixture_mismatch", step=step, path=spec["path"])
    return data


def body_bytes(body: dict | None, env: Env, step: int) -> bytes:
    if body is None:
        return b""
    ((form, value),) = body.items()
    if form == "empty":
        return b""
    if form == "json":
        return compact_json(substitute_json(value, env, step))
    if form == "text":
        return substitute(value, env, step).encode("utf-8")
    if form == "base64":
        try:
            return base64.b64decode(value, validate=True)
        except (binascii.Error, ValueError):
            raise invalid("body.base64 is not valid base64", step=step) from None
    if form == "fixture":
        return fixture_bytes(value, env, step)
    if form == "fill":
        prefix = value.get("prefix", "").encode("utf-8")
        if len(prefix) > value["size"]:
            raise invalid("fill.prefix is longer than fill.size", step=step)
        return prefix + bytes([value["byte"]]) * (value["size"] - len(prefix))
    raise invalid(f"unknown body form {form!r}", step=step)


def request_json(http: dict) -> Any:
    """The request's JSON value as written (tokens unresolved), for T10.5."""
    body = http.get("body")
    if isinstance(body, dict) and "json" in body:
        return body["json"]
    return None


def request_bytes(http: dict, env: Env, step: int) -> bytes:
    """T6.2: request line, Host first unless given or no_host, the headers in
    order, Content-Length when the body has bytes and none is given."""
    method = http.get("method", "POST")
    target = substitute(http.get("target", "/"), env, step)
    if any(c in target for c in "\r\n "):
        raise invalid("http.target holds a space or line break", step=step)
    headers = [(n, substitute(v, env, step)) for n, v in http.get("headers", [])]
    for _, v in headers:
        if "\r" in v or "\n" in v:
            raise invalid("a header value holds a line break", step=step)
    body = body_bytes(http.get("body"), env, step)
    lines = [f"{method} {target} HTTP/1.1"]
    names = {n.lower() for n, _ in headers}
    if "host" not in names and not http.get("no_host", False):
        lines.append(f"Host: 127.0.0.1:{env.port}")
    lines.extend(f"{n}: {v}" for n, v in headers)
    if body and "content-length" not in names:
        lines.append(f"Content-Length: {len(body)}")
    return ("\r\n".join(lines) + "\r\n\r\n").encode("utf-8") + body


# --- replies (T8) --------------------------------------------------------------------


def _strict_constant(name: str) -> Any:
    raise ValueError(name)


def body_value(data: bytes) -> dict:
    if not data:
        return {"empty": True}
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        return {"base64": base64.b64encode(data).decode("ascii")}
    try:
        return {"json": json.loads(text, parse_constant=_strict_constant)}
    except ValueError:
        return {"text": text}


def split_head(data: bytes) -> tuple[bytes, bytes] | None:
    end = data.find(b"\r\n\r\n")
    if end < 0:
        return None
    return data[:end], data[end + 4:]


def parse_head(head: bytes) -> tuple[int, list[tuple[str, str]]]:
    lines = head.split(b"\r\n")
    status = lines[0].decode("latin-1").split(" ", 2)
    if len(status) < 2 or not status[0].startswith("HTTP/") or not status[1].isdigit() or len(status[1]) != 3:
        raise RunError("bad_framing", detail="the reply's status line is not HTTP/x.y <code>")
    headers = []
    for line in lines[1:]:
        name, sep, value = line.decode("latin-1").partition(":")
        if not sep:
            raise RunError("bad_framing", detail="a reply header line has no colon")
        headers.append((name.strip(" \t").lower(), value.strip(" \t")))
    return int(status[1]), headers


def content_length(headers: list[tuple[str, str]]) -> int | None:
    values = [v for n, v in headers if n == "content-length"]
    if not values:
        return None
    if len(set(values)) != 1 or not values[0].isdigit():
        raise RunError("bad_framing", detail="the reply's Content-Length is not one decimal value")
    return int(values[0])


def reply_value(status: int, headers: list[tuple[str, str]], body: bytes, closed: bool) -> dict:
    compared: dict[str, str] = {}
    for name, value in headers:
        if name in COMPARED_HEADERS:
            compared[name] = f"{compared[name]}, {value}" if name in compared else value
    return {"status": status, "headers": compared, "body": body_value(body), "closed": closed}


# --- normalization (T10) --------------------------------------------------------------


def _walk(value: Any, fn: Callable[[Any, str], Any], pointer: str = "") -> Any:
    """Rebuild value bottom-up with fn(leaf or container, pointer)."""
    if isinstance(value, dict):
        value = {k: _walk(v, fn, f"{pointer}/{_escape(k)}") for k, v in value.items()}
    elif isinstance(value, list):
        value = [_walk(v, fn, f"{pointer}/{i}") for i, v in enumerate(value)]
    return fn(value, pointer)


def _escape(key: str) -> str:
    return key.replace("~", "~0").replace("/", "~1")


def _at(value: Any, segments: list[str], fn: Callable[[Any], Any]) -> Any:
    """value with fn applied at every match of the segments (* any one)."""
    if not segments:
        return fn(value)
    head, rest = segments[0], segments[1:]
    if isinstance(value, dict):
        return {k: (_at(v, rest, fn) if head in ("*", k) else v) for k, v in value.items()}
    if isinstance(value, list):
        return [(_at(v, rest, fn) if head == "*" or head == str(i) else v) for i, v in enumerate(value)]
    return value


def _segments(pattern: str) -> list[str]:
    return [s.replace("~1", "/").replace("~0", "~") for s in pattern.split("/")[1:]] if pattern else []


def is_error_reply(value: Any) -> bool:
    return isinstance(value, dict) and set(value) == {"result", "error"} and isinstance(value["error"], str)


def _version(request: Any) -> int | None:
    if not isinstance(request, dict):
        return None
    version = request.get("version", 4)
    return version if _is_int(version) else None


class Normalizer:
    def __init__(self, env: Env) -> None:
        self.env = env

    # T10.3 strings that hold environment values, in every output.
    def _strings(self, value: Any) -> Any:
        def fn(v: Any, _p: str) -> Any:
            if not isinstance(v, str):
                return v
            if self.env.media_dir is not None and v == self.env.media_dir:
                return "<media-dir>"
            if self.env.fixture_base:
                v = v.replace(self.env.fixture_base, "<fixture>")
            return v

        return _walk(value, fn)

    def observation(self, value: Any) -> Any:
        """observe outputs: their own rules (the driver's) and T10.3's fixture
        and media folder strings; T10.5 names no position in them."""
        return self._strings(value)

    def reply(self, reply: dict, request: Any) -> tuple[dict, list[str]]:
        """The normalized reply and the pointers (into the body) of integers
        left in place that equal a tokened entity's id (T10.1's warning)."""
        body = reply["body"]
        if "json" not in body:
            return reply, []
        value = body["json"]
        value = self._mod(value)
        value = self._strings(value)
        value = self._actions(value, request)
        warnings = self._leftover_ids(value)
        return {**reply, "body": {"json": value}}, warnings

    def _mod(self, value: Any) -> Any:
        start = self.env.clock_start

        def fn(v: Any, _p: str) -> Any:
            if isinstance(v, dict) and "mod" in v and _is_int(v["mod"]):
                return {**v, "mod": f"@{v['mod'] - start}"}
            return v

        return _walk(value, fn)

    def _actions(self, value: Any, request: Any) -> Any:
        """T10.3's per-action rules and T10.1 for one (sub-)reply."""
        if not isinstance(request, dict) or not isinstance(request.get("action"), str):
            return value
        action, version = request["action"], _version(request)
        if is_error_reply(value):
            return {**value, "error": self._error_text(value["error"])}
        if version is None:
            return value
        prefix = [] if version <= 4 else ["result"]
        if version > 4 and not (isinstance(value, dict) and "result" in value):
            return value

        def at(pattern: str, fn: Callable[[Any], Any]) -> None:
            nonlocal value
            value = _at(value, prefix + _segments(pattern), fn)

        if action == "multi":
            subs = ((request.get("params") or {}).get("actions") if isinstance(request.get("params"), dict) else None)
            if isinstance(subs, list):
                def each(result: Any) -> Any:
                    if not isinstance(result, list):
                        return result
                    return [self._actions(r, subs[i]) if i < len(subs) else r for i, r in enumerate(result)]

                at("", each)
            return value
        if action == "getMediaDirPath":
            at("", lambda v: "<media-dir>" if isinstance(v, str) else v)
        profile = self.env.profile
        if profile is not None:
            same = lambda v: "<profile>" if v == profile else v  # noqa: E731
            if action == "getActiveProfile":
                at("", same)
            elif action == "getProfiles":
                at("/*", same)
            elif action == "notesInfo":
                at("/*/profile", same)
        rand = lambda v: "#rand" if _is_int(v) else v  # noqa: E731
        if action in MODEL_LISTS:
            at("/*/tmpls/*/id", rand)
            at("/*/flds/*/id", rand)
        elif action in MODEL_ONE:
            at("/tmpls/*/id", rand)
            at("/flds/*/id", rand)
        if action == "cardsInfo":
            at("/*/nextReviews/*", lambda v: v.translate(BIDI_MARKS) if isinstance(v, str) else v)
            needle = self.env.tts_needle
            if needle:
                fix = lambda v: v.replace(needle, "[tts: lang not set]") if isinstance(v, str) else v  # noqa: E731
                at("/*/question", fix)
                at("/*/answer", fix)
        for kind, pattern in ID_POSITIONS.get(action, ()):
            if pattern == "#keys":
                at("", lambda v, kind=kind: self._keys(v, kind))
            else:
                at(pattern, lambda v, kind=kind: self._id(v, kind))
        return value

    def _id(self, v: Any, kind: str) -> Any:
        if not _is_int(v):
            return v
        token = self.env.tokens.token(kind, v)
        if token:
            return token
        return "#unknown" if v >= ID_MIN else v

    def _keys(self, v: Any, kind: str) -> Any:
        if not isinstance(v, dict):
            return v
        out = {}
        for k, item in v.items():
            if k.isdigit() and k == str(int(k)):
                k = self._id(int(k), kind)
                k = k if isinstance(k, str) else str(k)
            out[k] = item
        return out

    def _error_text(self, text: str) -> str:
        tokens = self.env.tokens
        if text.startswith(("Note", "note")):
            kind = "n"
        elif text.startswith(("Card", "card")):
            kind = "c"
        elif text.startswith("deck"):
            kind = "d"
        elif text.startswith("model"):
            kind = "m"
        else:
            kind = None

        def repl(m: re.Match) -> str:
            real = int(m.group(0))
            if kind is not None:
                return tokens.token(kind, real) or m.group(0)
            found = tokens.any_kind(real)
            return found[0] if len(found) == 1 else m.group(0)

        return _DIGITS.sub(repl, text)

    def _leftover_ids(self, value: Any) -> list[str]:
        ids = self.env.tokens.alive_ids()
        found: list[str] = []

        def fn(v: Any, pointer: str) -> Any:
            if _is_int(v) and v >= ID_MIN and v in ids:
                found.append(pointer)
            return v

        _walk(value, fn, "/body/json")
        return found
