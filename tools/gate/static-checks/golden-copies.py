"""Test fixtures that copy text out of a conformance golden still match it.

    golden-copies.py <checkout> [<rev>]
    golden-copies.py --write <checkout>   (rewrite the copies from their goldens)

Some app test fixtures are exact copies of strings that a conformance golden
holds (the goldens are oracle output, so tests take their expectations from
there and never from Meguri's renderer). A golden re-recorded after a scenario
change would leave such a copy behind without a word. Each directory with
copies declares them in a `golden-copies.json` manifest:

    {"golden": "<golden path from the checkout root>",
     "copies": [{"file": "<path relative to the manifest>",
                 "pointer": "<RFC 6901 JSON pointer to a string in the golden>"}]}

A copy's content is that string followed by one newline. Checks, in the
working tree or in <rev>'s tree, that every manifest parses, names an existing
golden, points at strings, and that every copy exists and is exactly its
string. Prints one JSON line per finding ({check, path, line, message}) and
exits 1 when there is one, 2 when it cannot check. Standard library only: it
runs under macOS's python3.
"""

import json
import os
import posixpath
import subprocess
import sys

MANIFEST = "golden-copies.json"


class Tree:
    def __init__(self, checkout, rev):
        self.checkout, self.rev = checkout, rev

    def git(self, *args):
        return subprocess.run(["git", "-C", self.checkout] + list(args), check=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout

    def paths(self):
        if self.rev:
            out = self.git("ls-tree", "-r", "-z", "--name-only", self.rev)
        else:
            out = self.git("ls-files", "-z", "--cached", "--others", "--exclude-standard")
        paths = {p for p in out.decode("utf-8", "surrogateescape").split("\0") if p}
        if not self.rev:  # a tracked file deleted from the working tree is gone
            paths = {p for p in paths if os.path.isfile(posixpath.join(self.checkout, p))}
        return sorted(paths)

    def read(self, path):
        if self.rev:
            return self.git("show", self.rev + ":" + path)
        with open(posixpath.join(self.checkout, path), "rb") as f:
            return f.read()


def resolve(document, pointer):
    """The value at an RFC 6901 pointer, or raise KeyError naming the step."""
    if pointer == "":
        return document
    if not pointer.startswith("/"):
        raise KeyError("a pointer starts with /")
    value = document
    for raw in pointer[1:].split("/"):
        token = raw.replace("~1", "/").replace("~0", "~")
        if isinstance(value, list):
            if not token.isdigit() or int(token) >= len(value) or (len(token) > 1 and token[0] == "0"):
                raise KeyError("no item %s" % token)
            value = value[int(token)]
        elif isinstance(value, dict):
            if token not in value:
                raise KeyError("no key %s" % token)
            value = value[token]
        else:
            raise KeyError("%s is below a scalar" % token)
    return value


def expected(tree, manifest_path, present, finding):
    """[(copy path, expected bytes)] of one manifest; findings for what is wrong."""
    try:
        manifest = json.loads(tree.read(manifest_path).decode("utf-8"))
        golden_path, copies = manifest["golden"], manifest["copies"]
        if not isinstance(golden_path, str) or not isinstance(copies, list) or not copies:
            raise ValueError("golden must be a path and copies a non-empty list")
        entries = [(c["file"], c["pointer"]) for c in copies]
        if not all(isinstance(f, str) and isinstance(p, str) for f, p in entries):
            raise ValueError("each copy has a file and a pointer, both strings")
    except (ValueError, KeyError, TypeError) as error:
        finding(manifest_path, 0, "unreadable manifest: %s" % error)
        return []
    if golden_path not in present:
        finding(manifest_path, 0, "names golden %s, which does not exist" % golden_path)
        return []
    try:
        golden = json.loads(tree.read(golden_path).decode("utf-8"))
    except ValueError as error:
        finding(golden_path, 0, "the golden that %s copies from is not JSON: %s" % (manifest_path, error))
        return []
    base, out = posixpath.dirname(manifest_path), []
    for name, pointer in entries:
        path = posixpath.normpath(posixpath.join(base, name))
        try:
            value = resolve(golden, pointer)
        except KeyError as error:
            finding(path, 0, "%s copies it from pointer %s, which is not in %s (%s)"
                    % (manifest_path, pointer, golden_path, error))
            continue
        if not isinstance(value, str):
            finding(path, 0, "%s copies it from pointer %s, which is not a string in %s"
                    % (manifest_path, pointer, golden_path))
            continue
        out.append((path, (value + "\n").encode("utf-8"), golden_path, pointer))
    return out


def check(tree, write=False):
    paths = tree.paths()
    present = set(paths)
    findings = []

    def finding(path, line, message):
        findings.append({"check": "golden-copies", "path": path, "line": line, "message": message})

    for manifest_path in paths:
        if posixpath.basename(manifest_path) != MANIFEST:
            continue
        for path, want, golden_path, pointer in expected(tree, manifest_path, present, finding):
            if write:
                with open(posixpath.join(tree.checkout, path), "wb") as f:
                    f.write(want)
                continue
            if path not in present:
                finding(path, 0, "missing; %s lists it as a copy of %s at %s" % (manifest_path, golden_path, pointer))
            elif tree.read(path) != want:
                finding(path, 0, "differs from %s at %s, which %s says it copies; rewrite the copies with "
                        "`python3 -I -B tools/gate/static-checks/golden-copies.py --write .`"
                        % (golden_path, pointer, manifest_path))
    return findings


def main(argv):
    write = len(argv) == 3 and argv[1] == "--write"
    if not write and len(argv) not in (2, 3):
        print("usage: golden-copies.py <checkout> [<rev>] | golden-copies.py --write <checkout>", file=sys.stderr)
        return 2
    rev = None
    try:
        if write:
            findings = check(Tree(argv[2], None), write=True)
        else:
            if len(argv) == 3:
                rev = subprocess.run(["git", "-C", argv[1], "rev-parse", "--verify", "-q", argv[2] + "^{commit}"],
                                     check=True, stdout=subprocess.PIPE).stdout.decode().strip()
            findings = check(Tree(argv[1], rev))
    except (OSError, subprocess.CalledProcessError, UnicodeDecodeError) as error:
        print("golden-copies: cannot check: %s" % error, file=sys.stderr)
        return 2
    for f in findings:
        print(json.dumps(f, sort_keys=True))
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
