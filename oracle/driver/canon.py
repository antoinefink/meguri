# SPDX-License-Identifier: AGPL-3.0-or-later
"""Scenario loading, scenario hashing and golden file text.

- Scenario files are YAML restricted to a JSON-compatible subset, read with the
  YAML 1.2 core schema (so `yes`, `on`, `017` and `2030-01-15` are a string, a
  string, 17 and a string, as in serde-saphyr; PyYAML's YAML 1.1 defaults differ).
  Aliases, merge keys, explicit tags, duplicate keys and non-string keys are
  rejected. `.json` files are read as JSON.
- scenario_sha256 is the sha256 of the RFC 8785 (JCS) text of the parsed
  scenario value: comments, key order and formatting never change it.
- Golden files are UTF-8 JSON with sorted keys, two-space indents, arrays of
  scalars on one line, LF line ends and a final newline.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
from pathlib import Path
from typing import Any

import yaml
from yaml.constructor import ConstructorError
from yaml.events import AliasEvent
from yaml.nodes import MappingNode, ScalarNode, SequenceNode

SCENARIO_SUFFIXES = (".yaml", ".yml", ".json")


class ScenarioFormatError(ValueError):
    """The file is not a scenario in the JSON-compatible YAML subset."""


# --- YAML 1.2 core schema -----------------------------------------------------

_STR = "tag:yaml.org,2002:str"
_INT = "tag:yaml.org,2002:int"
_FLOAT = "tag:yaml.org,2002:float"
_BOOL = "tag:yaml.org,2002:bool"
_NULL = "tag:yaml.org,2002:null"
_SEQ = "tag:yaml.org,2002:seq"
_MAP = "tag:yaml.org,2002:map"

_CORE_RESOLVERS = [
    (_BOOL, re.compile(r"^(?:true|True|TRUE|false|False|FALSE)$"), list("tTfF")),
    (_INT, re.compile(r"^(?:[-+]?[0-9]+|0o[0-7]+|0x[0-9a-fA-F]+)$"), list("-+0123456789")),
    (
        _FLOAT,
        re.compile(
            r"^(?:[-+]?(?:\.[0-9]+|[0-9]+(?:\.[0-9]*)?)(?:[eE][-+]?[0-9]+)?"
            r"|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$"
        ),
        list("-+.0123456789"),
    ),
    (_NULL, re.compile(r"^(?:~|null|Null|NULL|)$"), ["~", "n", "N", ""]),
]


class _CoreLoader(yaml.SafeLoader):
    yaml_implicit_resolvers: dict = {}
    yaml_constructors: dict = {}

    def compose_node(self, parent, index):  # type: ignore[override]
        event = self.peek_event()
        if isinstance(event, AliasEvent):
            raise ConstructorError(None, None, "aliases are not allowed in scenarios", event.start_mark)
        # Every explicit tag, core ones (!!str true) and the non-specific `!`
        # included: node.tag, checked below, no longer tells them apart.
        if getattr(event, "tag", None) is not None:
            raise ConstructorError(None, None, f"tag {event.tag} is not allowed in scenarios", event.start_mark)
        return super().compose_node(parent, index)

    def construct_object(self, node, deep=False):  # type: ignore[override]
        if node.tag not in (_STR, _INT, _FLOAT, _BOOL, _NULL, _SEQ, _MAP):
            raise ConstructorError(None, None, f"tag {node.tag} is not allowed in scenarios", node.start_mark)
        return super().construct_object(node, deep)


for _tag, _regexp, _first in _CORE_RESOLVERS:
    _CoreLoader.add_implicit_resolver(_tag, _regexp, _first)


def _construct_str(loader, node):
    return loader.construct_scalar(node)


def _construct_int(loader, node):
    text = loader.construct_scalar(node)
    if text.startswith("0o"):
        return int(text[2:], 8)
    if text.startswith("0x"):
        return int(text[2:], 16)
    return int(text, 10)


def _construct_float(loader, node):
    text = loader.construct_scalar(node).lower()
    if text.endswith(".inf") or text.endswith(".nan"):
        raise ConstructorError(None, None, "infinities and NaN have no JSON form", node.start_mark)
    return float(text)


def _construct_bool(loader, node):
    return loader.construct_scalar(node).lower() == "true"


def _construct_null(loader, node):
    loader.construct_scalar(node)
    return None


def _construct_seq(loader, node):
    if not isinstance(node, SequenceNode):
        raise ConstructorError(None, None, "expected a sequence", node.start_mark)
    return [loader.construct_object(child, deep=True) for child in node.value]


def _construct_map(loader, node):
    if not isinstance(node, MappingNode):
        raise ConstructorError(None, None, "expected a mapping", node.start_mark)
    out: dict[str, Any] = {}
    for key_node, value_node in node.value:
        if key_node.tag == "tag:yaml.org,2002:merge":
            raise ConstructorError(None, None, "merge keys are not allowed in scenarios", key_node.start_mark)
        if not isinstance(key_node, ScalarNode) or key_node.tag != _STR:
            raise ConstructorError(None, None, "mapping keys must be strings", key_node.start_mark)
        key = key_node.value
        if key in out:
            raise ConstructorError(None, None, f"duplicate key {key!r}", key_node.start_mark)
        out[key] = loader.construct_object(value_node, deep=True)
    return out


for _tag, _fn in (
    (_STR, _construct_str),
    (_INT, _construct_int),
    (_FLOAT, _construct_float),
    (_BOOL, _construct_bool),
    (_NULL, _construct_null),
    (_SEQ, _construct_seq),
    (_MAP, _construct_map),
):
    _CoreLoader.add_constructor(_tag, _fn)
# `<<` resolves to the merge tag in SafeLoader; keep it visible so it is rejected.
_CoreLoader.add_implicit_resolver("tag:yaml.org,2002:merge", re.compile(r"^(?:<<)$"), ["<"])


def load_yaml_text(text: str) -> Any:
    try:
        return yaml.load(text, Loader=_CoreLoader)  # noqa: S506 - restricted loader
    except yaml.YAMLError as exc:
        raise ScenarioFormatError(str(exc)) from exc


def _reject_duplicate_json_keys(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise ScenarioFormatError(f"duplicate key {k!r}")
        out[k] = v
    return out


def load_scenario_file(path: Path) -> Any:
    text = path.read_text(encoding="utf-8")
    if path.suffix == ".json":
        try:
            return json.loads(text, object_pairs_hook=_reject_duplicate_json_keys)
        except json.JSONDecodeError as exc:
            raise ScenarioFormatError(str(exc)) from exc
    return load_yaml_text(text)


# --- RFC 8785 canonical JSON --------------------------------------------------


def _jcs_number(x: int | float) -> str:
    if isinstance(x, bool):
        raise TypeError("bool is not a number")
    if isinstance(x, int):
        if abs(x) > 2**53:
            raise ScenarioFormatError(f"integer {x} is outside the exact JSON number range")
        return str(x)
    if not math.isfinite(x):
        raise ScenarioFormatError("infinities and NaN have no JSON form")
    if x == 0:
        return "0"
    # ECMAScript Number::toString over the shortest round-trip digits, which
    # Python's repr also produces: value = 0.<digits> * 10**n.
    sign = "-" if x < 0 else ""
    mantissa, _, exp = repr(abs(x)).partition("e")
    int_part, _, frac_part = mantissa.partition(".")
    significant = (int_part + frac_part).lstrip("0")
    digits = significant.rstrip("0")
    n = len(significant) + (int(exp) if exp else 0) - len(frac_part)
    k = len(digits)
    if k <= n <= 21:
        return sign + digits + "0" * (n - k)
    if 0 < n <= 21:
        return sign + digits[:n] + "." + digits[n:]
    if -6 < n <= 0:
        return sign + "0." + "0" * (-n) + digits
    e = n - 1
    exp_text = f"e{'+' if e >= 0 else '-'}{abs(e)}"
    if k == 1:
        return sign + digits + exp_text
    return sign + digits[0] + "." + digits[1:] + exp_text


def _jcs_string(s: str) -> str:
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\b":
            out.append("\\b")
        elif ch == "\f":
            out.append("\\f")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif o < 0x20:
            out.append(f"\\u{o:04x}")
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def jcs(value: Any) -> str:
    """RFC 8785 serialization: sorted keys (UTF-16 order), no whitespace."""
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, (int, float)):
        return _jcs_number(value)
    if isinstance(value, str):
        return _jcs_string(value)
    if isinstance(value, list):
        return "[" + ",".join(jcs(v) for v in value) + "]"
    if isinstance(value, dict):
        keys = sorted(value, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(_jcs_string(k) + ":" + jcs(value[k]) for k in keys) + "}"
    raise TypeError(f"{type(value).__name__} has no JSON form")


def scenario_sha256(scenario: Any) -> str:
    return hashlib.sha256(jcs(scenario).encode("utf-8")).hexdigest()


# --- golden text --------------------------------------------------------------


def _is_scalar(v: Any) -> bool:
    return v is None or isinstance(v, (bool, int, float, str))


def _dump(value: Any, indent: int, out: list[str]) -> None:
    pad = "  " * indent
    if _is_scalar(value):
        if isinstance(value, float) and not math.isfinite(value):
            raise ValueError("infinities and NaN have no JSON form")
        out.append(json.dumps(value, ensure_ascii=False))
    elif isinstance(value, list):
        if all(_is_scalar(v) for v in value):
            out.append("[" + ", ".join(json.dumps(v, ensure_ascii=False) for v in value) + "]")
        else:
            out.append("[\n")
            for i, v in enumerate(value):
                out.append(pad + "  ")
                _dump(v, indent + 1, out)
                out.append(",\n" if i < len(value) - 1 else "\n")
            out.append(pad + "]")
    elif isinstance(value, dict):
        if not value:
            out.append("{}")
            return
        out.append("{\n")
        keys = sorted(value)
        for i, k in enumerate(keys):
            out.append(pad + "  " + json.dumps(k, ensure_ascii=False) + ": ")
            _dump(value[k], indent + 1, out)
            out.append(",\n" if i < len(keys) - 1 else "\n")
        out.append(pad + "}")
    else:
        raise TypeError(f"{type(value).__name__} has no JSON form")


def golden_text(value: Any) -> str:
    out: list[str] = []
    _dump(value, 0, out)
    out.append("\n")
    return "".join(out)
