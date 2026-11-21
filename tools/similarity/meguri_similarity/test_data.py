"""Explicit, paired conformance test data (R1881); never changes ordinary scan scope.

Every conformance area pairs `conformance/scenarios/<area>` with
`conformance/goldens/<area>`: a case is a scenario and the golden of the same
name in the same area.
"""

from __future__ import annotations

import bisect
import decimal
import hashlib
import json
import os
import re
import stat

import yaml

from .lexer import Lexed, LineIndex, TEXT_WORD, _COMMENT_WORD

ROOTS = ("conformance/scenarios", "conformance/goldens")
# The characters PyYAML's reader refuses (yaml.reader.Reader.NON_PRINTABLE).
_YAML_REFUSED = re.compile("[^\x09\x0A\x0D\x20-\x7E\x85\xA0-\uD7FF\uE000-\uFFFD\U00010000-\U0010FFFF]")
CHANNELS = ("lex", "com", "text")
_AREA = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]*")


class TestDataError(ValueError):
    """A test-data input cannot be checked safely or completely."""


def _scope(path):
    """(side, area) of a path in the two roots: side 0 for scenarios, 1 for
    goldens; area None for a root itself. None outside them."""
    for side, root in enumerate(ROOTS):
        if path == root:
            return side, None
        if path.startswith(root + "/"):
            area = path[len(root) + 1:].split("/", 1)[0]
            return (side, area) if _AREA.fullmatch(area) else None
    return None


def inputs(root, paths, max_bytes):
    """Expand only the scenario and golden roots and read regular, singly
    linked files.

    Every path component is opened relative to a held directory descriptor,
    without following links. Read bytes are also the bytes hashed and scanned.
    Returns the inputs and the number of (area, name) pairs.
    """
    selected = {}
    root_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)

    def open_relative(path):
        current = os.dup(root_fd)
        try:
            parts = path.split("/")
            for part in parts[:-1]:
                next_fd = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=current)
                os.close(current)
                current = next_fd
            return os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=current)
        finally:
            os.close(current)

    def collect(path):
        scope = _scope(path)
        if scope is None:
            raise TestDataError("test-data input is outside the conformance scenario and golden areas")
        side, area = scope
        fd = open_relative(path)
        try:
            before = os.fstat(fd)
            if stat.S_ISDIR(before.st_mode):
                with os.scandir(fd) as entries:
                    names = sorted(entry.name for entry in entries)
                for name in names:
                    collect(path + "/" + name)
                return
            if area is None or path == ROOTS[side] + "/" + area:
                raise TestDataError("test-data files must be inside an area directory")
            extension = os.path.splitext(path)[1]
            if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
                raise TestDataError("test-data requires regular files without links")
            # Top-level area manifests activate suites; they are not cases and
            # have no corresponding golden. Link checks still apply to them.
            if side == 0 and path in (f"{ROOTS[0]}/{area}/area.yaml", f"{ROOTS[0]}/{area}/area.yml"):
                return
            if extension not in ((".yaml", ".yml", ".json") if side == 0 else (".json",)):
                raise TestDataError("unsupported test-data file type")
            if before.st_size > max_bytes:
                raise TestDataError("test-data file exceeds the scanner size bound")
            with os.fdopen(os.dup(fd), "rb") as stream:
                raw = stream.read(max_bytes + 1)
            after = os.fstat(fd)
            if len(raw) > max_bytes or (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
                after.st_size, after.st_mtime_ns, after.st_ctime_ns
            ):
                raise TestDataError("test-data changed while being read")
            try:
                text = raw.decode("utf-8")
            except UnicodeError as error:
                raise TestDataError("test-data is not UTF-8") from error
            selected[path] = {"path": path, "sha256": hashlib.sha256(raw).hexdigest(), "text": text}
        finally:
            os.close(fd)

    try:
        for path in paths:
            collect(path)
    except (OSError, RecursionError) as error:
        # Do not include OS diagnostics that can quote targets outside scope.
        raise TestDataError("test-data input is missing, linked or unreadable") from error
    finally:
        os.close(root_fd)
    if not selected:
        raise TestDataError("test-data selection is empty")
    pairs = ({}, {})
    for path in sorted(selected):
        side, area = _scope(path)
        key = (area, os.path.splitext(path[len(ROOTS[side]) + len(area) + 2:])[0])
        if key in pairs[side]:
            raise TestDataError("test-data has duplicate scenario or golden names")
        pairs[side][key] = path
    if set(pairs[0]) != set(pairs[1]):
        raise TestDataError("test-data requires a scenario and golden of the same area for every selected name")
    return [selected[path] for path in sorted(selected)], len(pairs[0])


def _strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate key")
        result[key] = value
    return result


def _nonfinite(_value):
    raise ValueError("non-finite number")


def lex(text, path, lexer):
    """Scalar values/keys use existing cross-language hashes, never structure.

    The source remains in memory only. Decoded scalar characters map back to
    their source lines, including folded blocks and escaped strings. YAML
    comments are scanned separately, outside scalar spans.
    """
    try:
        if path.endswith(".json"):
            json.loads(text, object_pairs_hook=_strict_object, parse_constant=_nonfinite)
            # JSON strings may hold characters the YAML reader refuses (DEL,
            # C1 controls). No word contains one, so a space at the same
            # offset keeps every word, position and line.
            text = _YAML_REFUSED.sub(" ", text)
        if any(isinstance(token, (yaml.tokens.AliasToken, yaml.tokens.AnchorToken)) for token in yaml.scan(text)):
            raise ValueError("anchors and aliases are not supported")
        documents = list(yaml.compose_all(text, Loader=yaml.SafeLoader))
        if len(documents) != 1 or documents[0] is None:
            raise ValueError("one document required")
        out = Lexed("test-data")
        lines = LineIndex(text)
        spans = []

        def walk(node):
            if isinstance(node, yaml.ScalarNode):
                if node.tag not in {"tag:yaml.org,2002:" + t for t in ("str", "int", "float", "bool", "null")}:
                    raise ValueError("unsupported scalar tag")
                start, end = node.start_mark.index, node.end_mark.index
                spans.append((start, end))
                raw = text[start:end]
                # Monotone alignment keeps folded words on their source lines.
                # Unlike a general sequence diff, repetitive large values do
                # not cause quadratic matching. Escaped characters inherit the
                # preceding source position, on the same scalar source line.
                positions = {}
                for offset, char in enumerate(raw):
                    positions.setdefault(char, []).append(offset)
                mapping, previous = [], -1
                for char in node.value:
                    possible = positions.get(char, [])
                    index = bisect.bisect_right(possible, previous)
                    if index < len(possible):
                        previous = possible[index]
                    mapping.append(start + max(0, previous))
                # Lexing keys too avoids exempting strings used as map keys.
                lower, lower_lines = [], []
                for index, char in enumerate(node.value):
                    folded = char.lower()
                    lower.append(folded)
                    lower_lines.extend([lines.line(mapping[index])] * len(folded))
                words, word_lines = [], []
                for match in TEXT_WORD.finditer("".join(lower)):
                    words.append(match.group(0))
                    word_lines.append(lower_lines[match.start()])
                if words:
                    out.texts.append((words, word_lines, "scalar"))
                for match in re.finditer(r"[A-Za-z_][A-Za-z0-9_]*|\d+", node.value):
                    line = lines.line(mapping[match.start()])
                    lexer._lex_string(match.group(0), line, out.lex, out.lex_lines)
            elif isinstance(node, yaml.SequenceNode):
                if node.tag != "tag:yaml.org,2002:seq":
                    raise ValueError("unsupported sequence tag")
                for child in node.value:
                    walk(child)
            elif isinstance(node, yaml.MappingNode):
                if node.tag != "tag:yaml.org,2002:map":
                    raise ValueError("unsupported mapping tag")
                for key, value in node.value:
                    walk(key)
                    walk(value)
            else:
                raise ValueError("unsupported data node")

        walk(documents[0])
        if not path.endswith(".json"):
            remaining = list(text)
            for start, end in spans:
                remaining[start:end] = ["\n" if char == "\n" else " " for char in text[start:end]]
            previous_end = None
            for match in re.finditer(r"(?m)(?:^|(?<=\s))#[^\n]*", "".join(remaining)):
                words = _COMMENT_WORD.findall(match.group(0).lower())
                word_lines = [lines.line(match.start())] * len(words)
                # Use the original gap: masking scalar text must not join two
                # comment blocks separated by a data entry.
                gap = text[previous_end:match.start()] if previous_end is not None else None
                if out.comments and gap is not None and not gap.strip():
                    out.comments[-1][0].extend(words)
                    out.comments[-1][1].extend(word_lines)
                else:
                    out.comments.append((words, word_lines))
                previous_end = match.end()
        return out
    except (ValueError, TypeError, yaml.YAMLError, RecursionError) as error:
        # Parser exceptions may embed fixture content; expose no raw payload.
        raise TestDataError("invalid or unsupported test-data document") from error


# Oracle output (provenance.md, "Similarity scan"): a golden's matches are
# Anki's recorded program output, reported as `oracle-output` and not failing,
# only while the golden has exactly the shape `tools/mg oracle record` writes,
# names the current oracle pin, and its scenario_sha256 is the D3 hash of the
# paired scenario as it is now. Scenarios are never exempt. The header can be
# copied, so this trusts the rule that only `oracle record` writes goldens;
# the T1 oracle compare is what checks the observations themselves.
ORACLE_OUTPUT = "oracle-output"
_HEX64 = re.compile(r"[0-9a-f]{64}")
_GOLDEN_KEYS = {"clock", "observations", "oracle", "scenario_id", "scenario_sha256", "tz"}
_ORACLE_KEYS = {"anki_version", "driver_sha256", "lock_sha256"}
_MAX_SAFE = 1 << 53


def oracle_pin(root, rel):
    """(anki_version, lock_sha256) of the oracle pin file `rel` under root, or
    None when it is missing, linked, not a regular file or malformed (then no
    golden is exempt)."""
    if not isinstance(rel, str) or not rel or rel.startswith("/") or ".." in rel.split("/"):
        return None
    fds = []
    try:
        fds.append(os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW))
        parts = rel.split("/")
        for part in parts[:-1]:
            fds.append(os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fds[-1]))
        fd = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fds[-1])
        fds.append(fd)
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > 65536:
            return None
        with os.fdopen(os.dup(fd), "rb") as stream:
            raw = stream.read(65537)
        if len(raw) > 65536:
            return None
        pin = json.loads(raw.decode("utf-8"), object_pairs_hook=_strict_object, parse_constant=_nonfinite)
    except (OSError, ValueError, UnicodeError, RecursionError):
        return None
    finally:
        for fd in fds:
            os.close(fd)
    if not isinstance(pin, dict):
        return None
    version, lock = pin.get("anki_version"), pin.get("lock_sha256")
    if not isinstance(version, str) or not version or not isinstance(lock, str) or not _HEX64.fullmatch(lock):
        return None
    return version, lock


class _NotJson(ValueError):
    """The scenario has no JSON value under D2, so it has no D3 hash."""


def _core_plain(text):
    """The YAML 1.2 core schema's value of a plain scalar (dsl.md D2)."""
    if text in ("true", "True", "TRUE"):
        return True
    if text in ("false", "False", "FALSE"):
        return False
    if text in ("", "~", "null", "Null", "NULL"):
        return None
    if re.fullmatch(r"[-+]?[0-9]+", text):
        return _safe_int(int(text, 10))
    if re.fullmatch(r"0o[0-7]+", text):
        return _safe_int(int(text[2:], 8))
    if re.fullmatch(r"0x[0-9a-fA-F]+", text):
        return _safe_int(int(text[2:], 16))
    if re.fullmatch(r"[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?", text):
        value = float(text)
        if value != value or value in (float("inf"), float("-inf")):
            raise _NotJson()
        return value
    if re.fullmatch(r"[-+]?\.(inf|Inf|INF)|\.(nan|NaN|NAN)", text):
        raise _NotJson()
    return text


def _safe_int(value):
    if abs(value) > _MAX_SAFE:
        raise _NotJson()
    return value


def _node_value(node):
    if isinstance(node, yaml.ScalarNode):
        if node.tag not in ("tag:yaml.org,2002:str", "tag:yaml.org,2002:int", "tag:yaml.org,2002:float",
                            "tag:yaml.org,2002:bool", "tag:yaml.org,2002:null", "tag:yaml.org,2002:timestamp",
                            "tag:yaml.org,2002:value", "tag:yaml.org,2002:merge"):
            raise _NotJson()
        return _core_plain(node.value) if node.style is None else node.value
    if isinstance(node, yaml.SequenceNode):
        return [_node_value(child) for child in node.value]
    if isinstance(node, yaml.MappingNode):
        out = {}
        for key, value in node.value:
            name = _node_value(key)
            if not isinstance(name, str) or name in out or (key.style is None and key.value == "<<"):
                raise _NotJson()
            out[name] = _node_value(value)
        return out
    raise _NotJson()


def _jcs_number(value):
    """ECMAScript Number::toString (RFC 8785 3.2.2.3) of an int or a finite float."""
    if isinstance(value, int):
        return str(value)
    if value == 0:
        return "0"
    sign = "-" if value < 0 else ""
    _sign, digits, exponent = decimal.Decimal(repr(abs(value))).as_tuple()
    digits = list(digits)
    while len(digits) > 1 and digits[-1] == 0:
        digits.pop()
        exponent += 1
    while len(digits) > 1 and digits[0] == 0:
        digits.pop(0)
    s = "".join(map(str, digits))
    k = len(s)
    n = k + exponent
    if k <= n <= 21:
        return sign + s + "0" * (n - k)
    if 0 < n <= 21:
        return sign + s[:n] + "." + s[n:]
    if -6 < n <= 0:
        return sign + "0." + "0" * (-n) + s
    e = n - 1
    return sign + (s if k == 1 else s[0] + "." + s[1:]) + "e" + ("+" if e > 0 else "-") + str(abs(e))


def _jcs(value):
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, (int, float)):
        return _jcs_number(value)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, list):
        return "[" + ",".join(_jcs(item) for item in value) + "]"
    if isinstance(value, dict):
        keys = sorted(value, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(json.dumps(k, ensure_ascii=False) + ":" + _jcs(value[k]) for k in keys) + "}"
    raise _NotJson()


def scenario_sha256(path, text):
    """The D3 hash of a scenario's text (dsl.md: SHA-256 of the JCS form of the
    YAML 1.2 core value, or of the JSON value for .json), or None."""
    value = scenario_value(path, text)
    if value is _NONE:
        return None
    try:
        return hashlib.sha256(_jcs(value).encode("utf-8")).hexdigest()
    except (ValueError, TypeError, RecursionError, UnicodeError):
        return None


_NONE = object()


def scenario_value(path, text):
    """The scenario's D2 value, or _NONE when it has none."""
    try:
        if path.endswith(".json"):
            value = json.loads(text, object_pairs_hook=_strict_object, parse_constant=_nonfinite)
            _check_json_numbers(value)
        else:
            if any(isinstance(token, (yaml.tokens.AliasToken, yaml.tokens.AnchorToken, yaml.tokens.TagToken))
                   for token in yaml.scan(text)):
                return _NONE
            documents = list(yaml.compose_all(text, Loader=yaml.SafeLoader))
            if len(documents) > 1:
                return _NONE
            value = _node_value(documents[0]) if documents and documents[0] is not None else None
        return value
    except (ValueError, TypeError, yaml.YAMLError, RecursionError):
        return _NONE


def _check_json_numbers(value):
    if isinstance(value, bool) or value is None or isinstance(value, str):
        return
    if isinstance(value, int):
        _safe_int(value)
    elif isinstance(value, float):
        if value != value or value in (float("inf"), float("-inf")):
            raise _NotJson()
    elif isinstance(value, list):
        for item in value:
            _check_json_numbers(item)
    elif isinstance(value, dict):
        for item in value.values():
            _check_json_numbers(item)


def oracle_recorded(path, text, pin, scenario_path, scenario_text):
    """True when the golden at `path` has exactly the shape `tools/mg oracle
    record` writes, for the pin `pin` and for the paired scenario as it is now:
    its D3 hash, its zone and its clock (dsl.md D5)."""
    scope = _scope(path)
    if pin is None or scope is None or scope[0] != 1 or not path.endswith(".json"):
        return False
    scenario = scenario_value(scenario_path, scenario_text)
    scenario_sha = scenario_sha256(scenario_path, scenario_text)
    if scenario_sha is None or not isinstance(scenario, dict) or not isinstance(scenario.get("clock"), dict):
        return False
    clock_in = scenario["clock"]
    expected_clock = {"start": clock_in.get("start"), "mode": clock_in.get("mode", "frozen")}
    try:
        doc = json.loads(text, object_pairs_hook=_strict_object, parse_constant=_nonfinite)
    except (ValueError, RecursionError):
        return False
    if not isinstance(doc, dict) or set(doc) != _GOLDEN_KEYS or not isinstance(doc["observations"], dict):
        return False
    oracle, clock = doc["oracle"], doc["clock"]
    if not isinstance(oracle, dict) or set(oracle) != _ORACLE_KEYS:
        return False
    if (oracle["anki_version"], oracle["lock_sha256"]) != pin:
        return False
    if not isinstance(oracle["driver_sha256"], str) or not _HEX64.fullmatch(oracle["driver_sha256"]):
        return False
    if not isinstance(doc["tz"], str) or not doc["tz"] or not isinstance(clock, dict) \
            or not isinstance(clock.get("start"), str) or clock.get("mode") not in ("frozen", "step"):
        return False
    if clock != expected_clock or doc["tz"] != clock_in.get("tz"):
        return False
    return (doc["scenario_sha256"] == scenario_sha
            and doc["scenario_id"] == path[len(ROOTS[1]) + 1:-len(".json")])
