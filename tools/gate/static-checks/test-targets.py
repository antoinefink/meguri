"""Every integration test file is compiled (one test binary per crate).

    test-targets.py <checkout> [<rev>]

A crate whose Cargo.toml sets `autotests = false` builds only the test targets
it declares: usually one, `it` (tests/main.rs), whose root lists each
`tests/*.rs` file as a module, and a few files that need their own `[[test]]`.
Cargo ignores any other file without a word, so its tests would never run.
Checks, in the working tree or in <rev>'s tree, for each such crate:
- every `tests/*.rs` (and `tests/*/main.rs`) is a declared `[[test]]` root or
  is listed by one, exactly once;
- a listed file does not need a binary of its own: it re-runs the test binary
  by test name (`current_exe()`; names gain the module prefix in a shared
  binary), sets a `#[global_allocator]`, or names insta snapshots (named after
  the binary's module path). Its `tests/<stem>/` modules count as the file.
Prints one JSON line per finding ({check, path, line, message}) and exits 1
when there is one, 2 when it cannot check. Standard library only: it runs
under macOS's python3.
"""

import json
import posixpath
import re
import subprocess
import sys

TABLE = re.compile(r"^\s*(\[\[?)\s*([A-Za-z0-9_.\-\"']+)\s*\]\]?\s*(#.*)?$")
KEY = re.compile(r"^\s*([A-Za-z0-9_\-]+)\s*=\s*(.*?)\s*(#.*)?$")
PATH_ATTR = re.compile(r'^#\[path\s*=\s*"([^"]+)"\]\s*(.*)$')
MOD = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+(r#)?([A-Za-z_][A-Za-z0-9_]*)\s*;")
NEEDS_OWN = [
    (re.compile(r"current_exe\(\)"), "re-runs the test binary by test name (current_exe())"),
    (re.compile(r"#\[global_allocator\]"), "sets a #[global_allocator]"),
    (re.compile(r"\binsta::|\bassert_(?:debug_|yaml_|json_|ron_|toml_|csv_)?snapshot!"),
     "names insta snapshots after the binary's module path"),
]


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
        return sorted({p for p in out.decode("utf-8", "surrogateescape").split("\0") if p})

    def read(self, path):
        if self.rev:
            data = self.git("show", self.rev + ":" + path)
        else:
            with open(posixpath.join(self.checkout, path), "rb") as f:
                data = f.read()
        return data.decode("utf-8", "replace")


def manifest(text):
    """(autotests, [[test]] entries as dicts) from a Cargo.toml's text."""
    autotests, tests, table = True, [], None
    for line in text.splitlines():
        m = TABLE.match(line)
        if m:
            table = ("array:" if m.group(1) == "[[" else "") + m.group(2)
            if table == "array:test":
                tests.append({})
            continue
        m = KEY.match(line)
        if not m:
            continue
        key, value = m.group(1), m.group(2)
        if table == "package" and key == "autotests":
            autotests = value != "false"
        elif table == "array:test" and key in ("name", "path"):
            try:
                tests[-1][key] = json.loads(value)
            except ValueError:
                raise ValueError("unreadable [[test]] %s: %s" % (key, value))
    return autotests, tests


def listed(root, text, present):
    """Files a target root declares with `mod` lines: (path, line) pairs."""
    base, pending, out = posixpath.dirname(root), None, []
    for n, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        m = PATH_ATTR.match(line)
        if m:
            pending, line = m.group(1), m.group(2).strip()
            if not line:
                continue
        m = MOD.match(line)
        if m:
            name = m.group(2)
            if pending is not None:
                out.append((posixpath.normpath(posixpath.join(base, pending)), n))
            else:
                own = posixpath.join(base, name + ".rs")
                out.append((own if own in present else posixpath.join(base, name, "mod.rs"), n))
            pending = None
        elif line and not line.startswith(("#[", "//")):
            pending = None
    return out


def check(tree):
    paths = tree.paths()
    present = set(paths)
    findings = []

    def finding(path, line, message):
        findings.append({"check": "test-targets", "path": path, "line": line, "message": message})

    for cargo in paths:
        if posixpath.basename(cargo) != "Cargo.toml" or cargo.startswith("core/vendor/"):
            continue
        autotests, tests = manifest(tree.read(cargo))
        if autotests:
            continue
        crate = posixpath.dirname(cargo)
        prefix = posixpath.join(crate, "tests") + "/"
        files = [p for p in paths if p.startswith(prefix) and (
            re.fullmatch(r"[^/]+\.rs", p[len(prefix):]) or re.fullmatch(r"[^/]+/main\.rs", p[len(prefix):]))]
        roots = {}
        for t in tests:
            if "path" in t:
                root = posixpath.normpath(posixpath.join(crate, t["path"]))
            elif "name" in t:
                root = prefix + t["name"] + ".rs"
                if root not in present:
                    root = prefix + t["name"] + "/main.rs"
            else:
                continue
            roots[root] = t.get("name", root)
        seen = {}
        for root in sorted(roots):
            if root not in present:
                finding(cargo, 0, "[[test]] %s names %s, which does not exist" % (roots[root], root))
                continue
            for path, line in listed(root, tree.read(root), present):
                if path not in files:
                    continue  # a helper module (tests/common/ and the like)
                if path in roots:
                    finding(root, line, "%s is also its own [[test]], so it is compiled twice" % path)
                elif path in seen:
                    finding(root, line, "%s is already listed by %s" % (path, seen[path]))
                else:
                    seen[path] = root
        for path in files:
            if path in roots:
                continue
            if path not in seen:
                finding(path, 0, "no test target compiles this file (%s sets autotests = false): add "
                        "`#[path = \"%s\"] pub mod %s;` to tests/main.rs, or give it its own [[test]]"
                        % (cargo, posixpath.basename(path), posixpath.basename(path)[:-3]))
                continue
            stem = path[:-3] if path.endswith(".rs") and not path.endswith("/main.rs") else posixpath.dirname(path)
            for part in [path] + sorted(p for p in paths if p.startswith(stem + "/") and p.endswith(".rs")):
                text = tree.read(part)
                for rx, why in NEEDS_OWN:
                    m = rx.search(text)
                    if m:
                        line = text.count("\n", 0, m.start()) + 1
                        finding(part, line, "%s %s, so it needs its own [[test]] in %s, not a line in %s"
                                % (path, why, cargo, seen[path]))
    return findings


def main(argv):
    if len(argv) not in (2, 3):
        print("usage: test-targets.py <checkout> [<rev>]", file=sys.stderr)
        return 2
    rev = None
    try:
        if len(argv) == 3:
            rev = subprocess.run(["git", "-C", argv[1], "rev-parse", "--verify", "-q", argv[2] + "^{commit}"],
                                 check=True, stdout=subprocess.PIPE).stdout.decode().strip()
        findings = check(Tree(argv[1], rev))
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print("test-targets: cannot check: %s" % error, file=sys.stderr)
        return 2
    for f in findings:
        print(json.dumps(f, sort_keys=True))
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
