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
  the binary's module path). Its `tests/<stem>/` modules count as the file;
- a listed file that defines a test some code re-runs by a bare name
  (`"--exact", "<name>"` anywhere in the tree, e.g. a testkit child process)
  has a test of that name at the binary's root;
- every feature a root's `#[cfg(feature = ...)]` names is in [features].
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
# `"--exact", "<name>"` or `.arg("--exact").arg(<name>)`, over line breaks;
# <name> is a literal or a constant of the same file.
EXACT = re.compile(r'"--exact"\s*(?:,|\)\s*\.arg\()\s*(?:"([A-Za-z_][A-Za-z0-9_]*)"|([A-Z][A-Z0-9_]*)\b)')
CFG = re.compile(r"#\[cfg\((.*?)\)\]", re.S)
FEATURE = re.compile(r'feature\s*=\s*"([^"]+)"')
TOML_STRING = re.compile(r'^(?:"((?:[^"\\]|\\.)*)"|\'([^\']*)\')$')
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

    def grep(self, pattern):
        """Tracked (and, in the working tree, untracked) .rs files that match."""
        args = ["grep", "-z", "-I", "-l", "-E", "-e", pattern]
        if self.rev:
            args += [self.rev]
        else:
            args.insert(1, "--untracked")
        result = subprocess.run(["git", "-C", self.checkout] + args + ["--", "*.rs", ":!core/vendor/"],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if result.returncode not in (0, 1):
            raise OSError(result.stderr.decode("utf-8", "replace").strip())
        prefix = self.rev + ":" if self.rev else ""
        return [p[len(prefix):] if p.startswith(prefix) else p
                for p in result.stdout.decode("utf-8", "surrogateescape").split("\0") if p]

    def read(self, path):
        if self.rev:
            data = self.git("show", self.rev + ":" + path)
        else:
            with open(posixpath.join(self.checkout, path), "rb") as f:
                data = f.read()
        return data.decode("utf-8", "replace")


def toml_string(value):
    """A TOML basic ("...") or literal ('...') string's value."""
    m = TOML_STRING.match(value)
    if not m:
        raise ValueError("not a TOML string: %s" % value)
    if m.group(2) is not None:
        return m.group(2)
    return re.sub(r"\\(.)", lambda e: {"n": "\n", "t": "\t"}.get(e.group(1), e.group(1)), m.group(1))


def manifest(text):
    """(autotests, [[test]] entries as dicts, features) from a Cargo.toml's text.

    Features are the [features] keys and the implicit feature of each
    optional dependency that no `dep:` reference hides.
    """
    autotests, tests, table, features = True, [], None, set()
    optional, dep_refs, sub = set(), set(re.findall(r"[\"']dep:([A-Za-z0-9_\-]+)", text)), None
    for line in text.splitlines():
        m = TABLE.match(line)
        if m:
            table = ("array:" if m.group(1) == "[[" else "") + m.group(2)
            sub = re.match(r"(?:target\..+\.)?(?:dev-|build-)?dependencies\.([A-Za-z0-9_\-]+)$", table)
            if table == "array:test":
                tests.append({})
            continue
        m = KEY.match(line)
        if not m:
            continue
        key, value = m.group(1), m.group(2)
        if table == "features":
            features.add(key)
        if sub and key == "optional" and value == "true":
            optional.add(sub.group(1))
        elif (table and re.match(r"(?:target\..+\.)?(?:dev-|build-)?dependencies$", table)
              and re.search(r"\boptional\s*=\s*true\b", value)):
            optional.add(key)
        if table == "package" and key == "autotests":
            autotests = value != "false"
        elif table == "array:test" and key in ("name", "path"):
            try:
                tests[-1][key] = toml_string(value)
            except ValueError:
                raise ValueError("unreadable [[test]] %s: %s" % (key, value))
    return autotests, tests, features | (optional - dep_refs)


def uncomment(text):
    """Rust comments (line, and nested block) blanked out; strings and line numbers kept."""
    out, i, n, depth = [], 0, len(text), 0
    while i < n:
        c, two = text[i], text[i:i + 2]
        if depth:
            if two == "/*":
                depth += 1
                out.append("  ")
                i += 2
            elif two == "*/":
                depth -= 1
                out.append("  ")
                i += 2
            else:
                out.append("\n" if c == "\n" else " ")
                i += 1
        elif two == "/*":
            depth = 1
            out.append("  ")
            i += 2
        elif two == "//":
            j = text.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i:j + 1])
            i = j + 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


def listed(root, text, present):
    """Files a target root declares with `mod` lines: (path, line) pairs."""
    base, pending, out = posixpath.dirname(root), None, []
    for n, raw in enumerate(uncomment(text).splitlines(), 1):
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
        elif line and not line.startswith("#["):
            pending = None
    return out


def check(tree):
    paths = tree.paths()
    present = set(paths)
    findings = []
    exact = set()
    for path in tree.grep('"--exact"'):
        text = tree.read(path)
        for m in EXACT.finditer(text):
            if m.group(1):
                exact.add(m.group(1))
            else:
                const = re.search(r'\bconst\s+%s\s*:\s*&(?:\'static\s+)?str\s*=\s*"([A-Za-z_][A-Za-z0-9_]*)"'
                                  % m.group(2), text)
                if const:
                    exact.add(const.group(1))

    def finding(path, line, message):
        findings.append({"check": "test-targets", "path": path, "line": line, "message": message})

    for cargo in paths:
        if posixpath.basename(cargo) != "Cargo.toml" or cargo.startswith("core/vendor/"):
            continue
        autotests, tests, features = manifest(tree.read(cargo))
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
            if root in roots:
                finding(cargo, 0, "[[test]] %s and %s both build %s, so it is compiled twice"
                        % (roots[root], t.get("name", root), root))
                continue
            roots[root] = t.get("name", root)
        seen = {}
        for root in sorted(roots):
            if root not in present:
                finding(cargo, 0, "[[test]] %s names %s, which does not exist" % (roots[root], root))
                continue
            root_text = tree.read(root)
            clean = uncomment(root_text)
            for m in CFG.finditer(clean):
                for f in FEATURE.findall(m.group(1)):
                    if f not in features:
                        finding(root, clean.count("\n", 0, m.start()) + 1,
                                "feature %r is not in %s's [features], so the module never builds" % (f, cargo))
            for path, line in listed(root, root_text, present):
                if path not in files:
                    continue  # a helper module (tests/common/ and the like)
                if path in roots:
                    finding(root, line, "%s is also its own [[test]], so it is compiled twice" % path)
                elif path in seen:
                    finding(root, line, "%s is already listed by %s" % (path, seen[path][0]))
                else:
                    seen[path] = (root, root_text)
        for path in files:
            if path in roots:
                continue
            if path not in seen:
                finding(path, 0, "no test target compiles this file (%s sets autotests = false): add "
                        "`#[path = \"%s\"] pub mod %s;` to tests/main.rs, or give it its own [[test]]"
                        % (cargo, posixpath.basename(path), posixpath.basename(path)[:-3]))
                continue
            stem = path[:-3] if path.endswith(".rs") and not path.endswith("/main.rs") else posixpath.dirname(path)
            root, root_text = seen[path]
            for part in [path] + sorted(p for p in paths if p.startswith(stem + "/") and p.endswith(".rs")):
                text = tree.read(part)
                for rx, why in NEEDS_OWN:
                    m = rx.search(text)
                    if m:
                        line = text.count("\n", 0, m.start()) + 1
                        finding(part, line, "%s %s, so it needs its own [[test]] in %s, not a line in %s"
                                % (path, why, cargo, root))
            for part in [path] + sorted(p for p in paths if p.startswith(stem + "/") and p.endswith(".rs")):
                text = tree.read(part)
                for name in sorted(exact):
                    m = re.search(r"\bfn\s+%s\s*\(" % re.escape(name), text)
                    if not m or re.search(r"^#\[test\]\s*(?:#\[[^\n]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?fn\s+%s\s*\("
                                          % re.escape(name), uncomment(root_text), re.M):
                        continue
                    finding(part, text.count("\n", 0, m.start()) + 1,
                            "defines %s, which code re-runs with `--exact %s`; in %s its name gains the module "
                            "prefix, so %s needs a root-level `#[test] fn %s` too (or the file its own [[test]])"
                            % (name, name, root, root, name))
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
