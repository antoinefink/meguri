"""Scanner configuration, allowlist and interop vocabulary."""

from __future__ import annotations

import glob
import hashlib
import os
import re
from dataclasses import dataclass

import yaml

from .lexer import normalize_vocab_word

CHANNELS = ("struct", "lex", "com", "text")


class ConfigError(Exception):
    pass


def load_yaml(path):
    try:
        with open(path, encoding="utf-8") as f:
            return yaml.safe_load(f) or {}
    except FileNotFoundError:
        raise ConfigError(f"{path}: not found")
    except yaml.YAMLError as e:
        raise ConfigError(f"{path}: invalid YAML: {e}")


def load_config(path):
    cfg = load_yaml(path)
    for key in ("corpus", "channels", "scan"):
        if key not in cfg:
            raise ConfigError(f"{path}: missing '{key}'")
    tags = cfg["corpus"].get("tags") or {}
    count = tags.get("count")
    if not all(isinstance(tags.get(k), str) for k in ("min", "max")) or not isinstance(count, int) or count < 1:
        raise ConfigError(f"{path}: corpus.tags needs min and max (release tags) and count (the pinned number of release tags)")
    return cfg


def glob_match(path: str, pattern: str) -> bool:
    """fnmatch where `**/` may match no directory and `*` does not cross `/`."""
    regex = _glob_regex(pattern)
    return regex.fullmatch(path) is not None


_GLOB_CACHE: dict[str, re.Pattern] = {}


def _glob_regex(pattern: str) -> re.Pattern:
    r = _GLOB_CACHE.get(pattern)
    if r is None:
        out, i = [], 0
        while i < len(pattern):
            if pattern.startswith("**/", i):
                out.append("(?:.*/)?")
                i += 3
            elif pattern.startswith("**", i):
                out.append(".*")
                i += 2
            elif pattern[i] == "*":
                out.append("[^/]*")
                i += 1
            elif pattern[i] == "?":
                out.append("[^/]")
                i += 1
            else:
                out.append(re.escape(pattern[i]))
                i += 1
        r = re.compile("".join(out))
        _GLOB_CACHE[pattern] = r
    return r


@dataclass
class AllowEntry:
    index: int
    path: str
    channels: tuple
    lines: tuple | None
    reason: str

    def covers(self, path: str, channel: str, line_start: int, line_end: int) -> bool:
        """The entry exempts a match spanning these lines."""
        if channel not in self.channels or not glob_match(path, self.path):
            return False
        if self.lines is None:
            return True
        return self.lines[0] <= line_start and line_end <= self.lines[1]

    def overlaps(self, path: str, channel: str, line_start: int, line_end: int) -> bool:
        """The entry exempts part of a region spanning these lines (for reports)."""
        if channel not in self.channels or not glob_match(path, self.path):
            return False
        return self.lines is None or (self.lines[0] <= line_end and line_start <= self.lines[1])


_ALLOW_KEYS = {"path", "channels", "lines", "reason", "approved_by", "approved_on", "note"}


def load_allowlist(path) -> list[AllowEntry]:
    """Entries need a reason and `approved_by: antoine` (written only from
    Antoine's chat approval; U19 escalation)."""
    if path is None or not os.path.exists(path):
        return []
    data = load_yaml(path)
    entries = data.get("entries") or []
    if not isinstance(entries, list):
        raise ConfigError(f"{path}: 'entries' must be a list")
    out = []
    for i, e in enumerate(entries):
        where = f"{path}: entry {i + 1}"
        if not isinstance(e, dict):
            raise ConfigError(f"{where}: must be a mapping")
        unknown = set(e) - _ALLOW_KEYS
        if unknown:
            raise ConfigError(f"{where}: unknown keys {sorted(unknown)}")
        if not isinstance(e.get("path"), str) or not e["path"]:
            raise ConfigError(f"{where}: 'path' (a glob) is required")
        if not isinstance(e.get("reason"), str) or not e["reason"].strip():
            raise ConfigError(f"{where}: 'reason' is required")
        if e.get("approved_by") != "antoine":
            raise ConfigError(f"{where}: 'approved_by: antoine' is required (only from Antoine's chat approval)")
        channels = e.get("channels") or list(CHANNELS)
        if not isinstance(channels, list) or any(c not in CHANNELS for c in channels):
            raise ConfigError(f"{where}: 'channels' must be a list of {list(CHANNELS)}")
        lines = None
        if e.get("lines") is not None:
            m = re.fullmatch(r"\s*(\d+)\s*-\s*(\d+)\s*", str(e["lines"]))
            if not m or int(m.group(1)) > int(m.group(2)):
                raise ConfigError(f"{where}: 'lines' must look like '12-80'")
            lines = (int(m.group(1)), int(m.group(2)))
        out.append(AllowEntry(i + 1, e["path"], tuple(channels), lines, e["reason"].strip()))
    return out


def load_vocab(root: str | None):
    """Interop vocabulary from specs/<area>/interop-vocab.txt under `root`:
    one word per line, `#` comments. Returns (normalized set, sha256 of the set)."""
    words = set()
    paths = sorted(glob.glob(os.path.join(root, "specs", "*", "interop-vocab.txt"))) if root else []
    for path in paths:
        with open(path, encoding="utf-8") as f:
            for line in f:
                line = line.split("#", 1)[0].strip()
                if line:
                    norm = normalize_vocab_word(line)
                    if norm:
                        words.add(norm)
    digest = hashlib.sha256("\n".join(sorted(words)).encode()).hexdigest()
    return frozenset(words), digest
