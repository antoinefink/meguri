#!/usr/bin/env python3
"""Conservatively require explicit zero repetition in browser runner configs."""

import json
import os
from pathlib import Path
import re
import subprocess
import sys

# Never evaluate a repository's config. Skip comments and tokenize quoted keys,
# identifiers, numbers and punctuation, retaining offsets across physical lines.
TOKEN = re.compile(r'''//[^\n]*|/\*[\s\S]*?\*/|`(?:\\[\s\S]|[^`\\])*`|"(?:\\[\s\S]|[^"\\])*"|'(?:\\[\s\S]|[^'\\])*'|[A-Za-z_$][\w$]*|\d+|[^\s]''')


# Decode only these unambiguous JS key escapes. Other escaped key syntax is
# refused for review, including legacy octal, line continuations and escaped
# identifiers. Python's unicode_escape codec is not a JavaScript parser.
KEY_ESCAPE = re.compile(r"\\(?:x([0-9a-fA-F]{2})|u([0-9a-fA-F]{4})|u\{([0-9a-fA-F]{1,6})\})")
CONFIG_PATH = re.compile(r"(?:^|/)(?:playwright|vitest|vite|cypress)\.config\.[cm]?[jt]s$")


def key_value(literal):
    def decode(match):
        return chr(int(next(part for part in match.groups() if part is not None), 16))

    value = KEY_ESCAPE.sub(decode, literal)
    if "\\" in value:
        raise ValueError("unsupported key escape")
    return value


def findings(source, path):
    tokens = [m for m in TOKEN.finditer(source) if not m.group().startswith(("//", "/*"))]
    for index, match in enumerate(tokens):
        value = match.group()
        following = [m.group() for m in tokens[index + 1:index + 4]]
        key_syntax = bool(following) and (following[0] in (":", "(") or following[:2] == ["]", ":"])
        # Parentheses and other expressions can hide escaped keys. Refuse all
        # computed properties/methods rather than interpret JavaScript.
        if value == "]" and following and following[0] in (":", "("):
            yield {"check": "no-retries", "path": path,
                   "line": source.count("\n", 0, match.start()) + 1,
                   "message": "browser config computed property syntax needs review"}
            continue
        if value == "\\":
            yield {"check": "no-retries", "path": path,
                   "line": source.count("\n", 0, match.start()) + 1,
                   "message": "browser config escaped identifier syntax needs review"}
            continue
        # Plain templates are opaque strings. Interpolation can contain config
        # expressions and nested templates; refuse it instead of hiding code.
        if value.startswith("`"):
            if len(value) == 1 or "${" in value:
                yield {"check": "no-retries", "path": path,
                       "line": source.count("\n", 0, match.start()) + 1,
                       "message": "browser config template interpolation or unterminated template needs review"}
                continue
        if value.startswith(("'", '"', "`")):
            value = value[1:-1]
            if key_syntax:
                try:
                    value = key_value(value)
                except ValueError:
                    yield {"check": "no-retries", "path": path,
                           "line": source.count("\n", 0, match.start()) + 1,
                           "message": "browser config escaped property syntax needs review"}
                    continue
        if value not in ("retries", "retry"):
            continue
        if len(following) != 3 or following[:2] != [":", "0"] or following[2] not in (",", "}"):
            yield {"check": "no-retries", "path": path,
                   "line": source.count("\n", 0, match.start()) + 1,
                   "message": "browser test repetition must be an explicit zero"}


def scan_tree(root, rev):
    # Preserve spaces/newlines and check both enumeration and content reads.
    # The shell must not split filenames or hide a failed reader in a pipeline.
    command = ["git", "-C", root]
    listing = (["ls-tree", "-r", "--name-only", "-z", rev] if rev else
               ["ls-files", "--cached", "--others", "--exclude-standard", "-z"])
    for raw_path in subprocess.check_output(command + listing).split(b"\0"):
        path = os.fsdecode(raw_path)
        if not CONFIG_PATH.search(path):
            continue
        content = (subprocess.check_output(command + ["show", f"{rev}:{path}"]) if rev else
                   (Path(root) / path).read_bytes())
        yield from findings(content.decode("utf-8"), path)


if __name__ == "__main__":
    try:
        for finding in scan_tree(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else ""):
            print(json.dumps(finding))
    except (OSError, UnicodeError, subprocess.CalledProcessError) as error:
        print(f"no-retries: could not read browser configs: {error}", file=sys.stderr)
        sys.exit(2)
