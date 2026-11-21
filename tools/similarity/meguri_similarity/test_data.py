"""Explicit, paired conformance test data (R1881); never changes ordinary scan scope.

Every conformance area pairs `conformance/scenarios/<area>` with
`conformance/goldens/<area>`: a case is a scenario and the golden of the same
name in the same area.
"""

from __future__ import annotations

import bisect
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
