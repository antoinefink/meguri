#!/usr/bin/env python3
"""Session records over a branch (R1958, docs/process/provenance.md).

    sessions.py <checkout> <base> <rev>
    sessions.py --records <checkout> <rev>   (the records at <rev>, for readers)

Records live one per file, docs/process/provenance/sessions/<id>.json, and
are only ever added. docs/process/provenance/sessions.jsonl is the archive of
the records made before the switch to that layout. For <base>..<rev>:

- A commit may add a record file (a regular file), never modify, delete or
  rename one. It changes nothing else and carries `Role: infra` and
  `Session: <id>`. The record is one JSON object whose session is its file
  name, with a known role, its role's instructions (role_doc, role_doc_blob at
  base_commit), a full base_commit id that the commit descends from and a
  created_at.
- No archive line is ever removed or changed. Transition rule: a line may
  still be added by a commit made with the tools from before the switch (its
  parent has no MARKER), that changes only the archive, carries `Role: infra`
  and `Session: <the line's session>`, and adds a record that passes the same
  field checks. A line added any other way counts only while it remains: a
  later commit of the branch may take it out again.
- A merge keeps each parent's records and archive lines as they are and
  adds none. Between the merge bases and <rev> no record file or archive line is
  dropped or changed, and every new one came from such a commit (or the base
  branch already holds it), so a merge cannot drop, change or slip in a record.
- Each new session id is recorded once, and every `Session:` trailer (merges'
  included) names a
  record committed at <rev> or <base> (archive or directory), except a record
  commit's own (an archive line taken out again leaves it behind).

Prints one JSON line per finding ({check, commit, key, subject, message}); exits
1 when there is one, 2 when it cannot check. Stdlib only, for macOS python3.
"""

import collections
import datetime
import hashlib
import json
import re
import subprocess
import sys

ARCHIVE = "docs/process/provenance/sessions.jsonl"
RECORDS = "docs/process/provenance/sessions/"
# This file: a commit whose parent holds it was made with the per-session
# tools, which never append to the archive.
MARKER = "tools/gate/static-checks/sessions.py"

ROLES = ("spec-writer", "implementer", "fixer", "oracle-maintainer", "infra", "critic", "integrator")
ROLE_DOCS = {
    "fixer": "docs/process/roles/implementer.md",
    "spec-writer": "docs/process/roles/spec-writer.md",
    "implementer": "docs/process/roles/implementer.md",
    "oracle-maintainer": "docs/process/roles/oracle-maintainer.md",
    "critic": "docs/process/roles/critic.md",
}
ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{2,}")
OID = re.compile(r"[0-9a-f]{40}|[0-9a-f]{64}")
INSTANT = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z")


class CannotCheck(Exception):
    pass


class Repo:
    def __init__(self, path):
        self.path = path

    def run(self, *args, ok=(0,), data=None):
        proc = subprocess.run(["git", "--no-replace-objects", "-C", self.path] + list(args),
                              input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if proc.returncode not in ok:
            raise CannotCheck("git %s failed: %s" % (args[0], proc.stderr.decode("utf-8", "replace").strip()))
        return proc

    def out(self, *args, data=None):
        return self.run(*args, data=data).stdout

    def commit(self, rev):
        proc = self.run("rev-parse", "-q", "--verify", rev + "^{commit}", ok=(0, 1, 128))
        return proc.stdout.decode().strip() if proc.returncode == 0 else None

    def entry(self, rev, path):
        """(mode, type, oid) of <rev>:<path>, or None when it has none."""
        out = self.out("ls-tree", "-z", rev, "--", path).split(b"\0")[0]
        if not out:
            return None
        meta, name = out.split(b"\t", 1)
        if name.decode("utf-8", "surrogateescape") != path:
            return None
        return tuple(meta.decode().split(" "))

    def blob(self, rev, path):
        """The bytes of the file <rev>:<path>, or None when there is none."""
        e = self.entry(rev, path)
        if e is None or e[1] != "blob":
            return None
        return self.out("cat-file", "blob", e[2])

    def blob_id(self, rev, path):
        e = self.entry(rev, path)
        return e[2] if e is not None and e[1] == "blob" else None

    def is_ancestor(self, a, b):
        return self.run("merge-base", "--is-ancestor", a, b, ok=(0, 1)).returncode == 0

    def record_files(self, rev):
        """{path: (mode, oid)} of everything under the record directory."""
        files = {}
        for entry in self.out("ls-tree", "-r", "-z", rev, "--", RECORDS).split(b"\0"):
            if not entry:
                continue
            meta, path = entry.split(b"\t", 1)
            mode, _kind, oid = meta.decode().split(" ")
            files[path.decode("utf-8", "surrogateescape")] = (mode, oid)
        return files

    def blobs(self, ids):
        """{blob id: bytes}, read through one git cat-file --batch."""
        ids = sorted(set(ids))
        if not ids:
            return {}
        data = self.out("cat-file", "--batch", data=("\n".join(ids) + "\n").encode())
        found, at = {}, 0
        for oid in ids:
            end = data.index(b"\n", at)
            header = data[at:end].split(b" ")
            if len(header) != 3 or header[1] != b"blob":
                raise CannotCheck("cannot read blob %s" % oid)
            size = int(header[2])
            found[oid] = data[end + 1 : end + 1 + size]
            at = end + 1 + size + 1
        return found


def lines(data):
    return [l for l in (data or b"").split(b"\n") if l.strip()]


def no_constant(name):
    raise ValueError("not JSON: " + name)


def finite(text):
    value = float(text)
    if value in (float("inf"), float("-inf")):
        raise ValueError("number out of range: " + text)
    return value


def parse(raw):
    try:
        value = json.loads(raw.decode("utf-8"), parse_constant=no_constant, parse_float=finite)
    except (UnicodeDecodeError, ValueError):
        return None
    return value if isinstance(value, dict) else None


def instant(value):
    if not isinstance(value, str) or not INSTANT.fullmatch(value):
        return False
    try:
        datetime.datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        return False
    return True


def regular(files):
    """The (path, oid) of the regular files of a record_files() map."""
    return [(path, oid) for path, (mode, oid) in files.items() if mode == "100644"]


def session_of(raw):
    record = parse(raw)
    return record.get("session") if record is not None and isinstance(record.get("session"), str) else None


def trailer_values(trailers):
    """{lower-case key: [values]} of `Key: value` trailer lines."""
    values = collections.defaultdict(list)
    for line in trailers.splitlines():
        key, sep, value = line.partition(":")
        if sep and key.strip():
            parts = re.split(r"[\s,]+", value) if key.strip().lower() == "session" else [value]
            for v in parts:
                if v.strip():
                    values[key.strip().lower()].append(v.strip())
    return values


class Check:
    def __init__(self, repo, base, rev):
        self.repo = repo
        self.base, self.rev = base, rev
        self.findings = collections.OrderedDict()
        self.added_files = {}  # path -> oid a non-merge commit of the range added
        self.added_lines = collections.Counter()  # archive lines old-tools commits added
        self.line_problems = collections.OrderedDict()  # line -> (commit, subject, message)
        self.own = set()  # (commit, session) of each record a commit adds

    def find(self, commit, subject, message):
        key = hashlib.sha256(message.encode("utf-8", "surrogateescape")).hexdigest()[:10]
        self.findings.setdefault((commit, message), {"check": "sessions", "commit": commit, "key": key,
                                                    "subject": subject, "message": message})

    # -- records ---------------------------------------------------------

    def fields_problem(self, what, record, commit):
        """Why <record> (described as <what>), added by <commit>, has wrong
        fields, or None."""
        role = record.get("role")
        if role not in ROLES:
            return "%s has role %s, not one of: %s" % (what, json.dumps(role), " ".join(ROLES))
        if record.get("kind") not in ("worktree", "session"):
            return "%s has kind %s, not worktree or session" % (what, json.dumps(record.get("kind")))
        if not instant(record.get("created_at")):
            return "%s has no created_at (YYYY-MM-DDTHH:MM:SSZ)" % what
        base = record.get("base_commit")
        base_sha = self.repo.commit(base) if isinstance(base, str) and OID.fullmatch(base) else None
        if base_sha != base:  # a full id of the commit itself, not of a tag
            base_sha = None
        if base_sha is None or not self.repo.is_ancestor(base_sha, commit):
            return "%s has base_commit %s, not the full id of a commit this one descends from" % (what, json.dumps(base))
        doc = ROLE_DOCS.get(role)
        if record.get("role_doc") != doc:
            return "%s has role_doc %s; role %s works under %s" % (what, json.dumps(record.get("role_doc")), role, json.dumps(doc))
        blob = self.repo.blob_id(base_sha, doc) if doc else None
        if doc and blob is None:
            return "%s names role_doc %s, which its base_commit does not hold" % (what, doc)
        if record.get("role_doc_blob") != blob:
            return "%s has role_doc_blob %s, not %s's blob at its base_commit (%s)" % (
                what, json.dumps(record.get("role_doc_blob")), doc, json.dumps(blob))
        return None

    def record_problem(self, path, mode, data, commit):
        name = path[len(RECORDS):]
        if "/" in name or not name.endswith(".json") or not ID.fullmatch(name[:-5]):
            return "%s is not a record file (%s<session id>.json)" % (path, RECORDS)
        if mode != "100644":
            return "%s is not a regular file (mode %s)" % (path, mode)
        record = parse(data)
        if record is None:
            return "%s is not one JSON object" % path
        if record.get("session") != name[:-5]:
            return "%s holds session %s, not its file name" % (path, json.dumps(record.get("session")))
        return self.fields_problem(path, record, commit)

    def archive_line_problem(self, raw, commit, parent, changes, values):
        """Why the archive line <raw> that <commit> adds breaks the transition
        rule, or None."""
        shown = raw[:120].decode("utf-8", "replace")
        record = parse(raw)
        if record is None:
            return "adds an archive line that is not one JSON object: %s" % shown
        session = record.get("session")
        if not isinstance(session, str) or not ID.fullmatch(session):
            return "adds an archive line without a session id: %s" % shown
        if parent is None or self.repo.entry(parent, MARKER) is not None:
            return ("adds session %s to the archive %s, which takes no new records since the switch to %s<id>.json. "
                    "Move it: remove the line in one commit, then commit the same record alone as %s%s.json "
                    "(Role: infra, Session: %s), so commits that cite the session still resolve"
                    % (session, ARCHIVE, RECORDS, RECORDS, session, session))
        if len(changes) != 1:
            return "adds session %s to the archive together with other changes; a session record is committed alone" % session
        if session not in values.get("session", []) or values.get("role") != ["infra"]:
            return "adds session %s to the archive without the trailers Role: infra and Session: %s" % (session, session)
        return self.fields_problem("archived session %s" % session, record, commit)

    # -- per commit --------------------------------------------------------

    def commits(self):
        out = self.repo.out("log", "-z", "--reverse", "--format=%H%x1f%P%x1f%s%x1f%(trailers:only,unfold)",
                            "%s..%s" % (self.base, self.rev)).decode("utf-8", "replace")
        for entry in out.split("\0"):
            if not entry.strip():
                continue
            sha, parents, subject, trailers = entry.strip("\n").split("\x1f", 3)
            yield sha, parents.split(), subject, trailers

    def changes(self, sha):
        """[(status, new mode, new oid, path)] of a commit against its first parent."""
        raw = self.repo.out("diff-tree", "-r", "-z", "--root", "--no-renames", "--no-commit-id", sha).split(b"\0")
        out, i = [], 0
        while i + 1 < len(raw):
            meta = raw[i].decode().lstrip(":").split(" ")
            out.append((meta[4][0], meta[1], meta[3], raw[i + 1].decode("utf-8", "surrogateescape")))
            i += 2
        return out

    def check_commit(self, sha, parents, subject, trailers):
        parent = parents[0] if parents else None
        changes = self.changes(sha)
        values = trailer_values(trailers)
        for status, mode, oid, path in changes:
            if path == RECORDS[:-1] and status != "D":
                self.find(sha, subject, "puts a file at %s, where the session records directory goes" % path)
            if path.startswith(RECORDS):
                if status != "A":
                    verb = {"D": "deletes", "M": "modifies", "T": "changes the type of"}.get(status, "changes")
                    self.find(sha, subject, "%s %s; session records are only ever added" % (verb, path))
                    continue
                self.added_files[path] = (mode, oid)
                session = path[len(RECORDS):-5] if path.endswith(".json") else path
                problem = self.record_problem(path, mode, self.repo.blobs([oid])[oid] if mode == "100644" else b"", sha)
                if problem:
                    self.find(sha, subject, problem)
                if len(changes) != 1:
                    self.find(sha, subject, "adds %s together with other changes; a session record is committed alone" % path)
                if session not in values.get("session", []) or values.get("role") != ["infra"]:
                    self.find(sha, subject, "adds %s without the trailers Role: infra and Session: %s" % (path, session))
            elif path == ARCHIVE:
                if status == "D":
                    self.find(sha, subject, "deletes the archive %s; session records are never removed" % ARCHIVE)
                    continue
                old = collections.Counter(lines(self.repo.blob(parent, ARCHIVE) if parent else None))
                new = collections.Counter(lines(self.repo.blob(sha, ARCHIVE)))
                # Taking out a refused line this range added is allowed; any
                # other is not.
                removed = old - new
                range_added = collections.Counter(self.line_problems.keys())
                if removed - range_added:
                    self.find(sha, subject, "removes or rewrites %d line(s) of the archive %s; session records are never edited or removed"
                              % (sum((removed - range_added).values()), ARCHIVE))
                for line, n in removed.items():
                    self.added_lines[line] = max(0, self.added_lines[line] - n)
                    if new[line] == 0:
                        self.line_problems.pop(line, None)
                for line, n in (new - old).items():
                    self.own.add((sha, session_of(line)))
                    problem = self.archive_line_problem(line, sha, parent, changes, values)
                    if problem:
                        self.line_problems[line] = (sha, subject, problem)
                    else:
                        self.added_lines[line] += n

    # -- merge bases to rev ------------------------------------------------

    def check_range(self, bases, subject):
        rev = self.rev
        self.check_directory(rev, subject)
        after = self.repo.record_files(rev)
        base_tip = self.repo.record_files(self.base)
        before = {}
        for mb in bases:
            before.update(self.repo.record_files(mb))
        for path, (mode, oid) in sorted(before.items()):
            if after.get(path) != (mode, oid):
                self.find(rev, subject, "%s %s, which the merge base holds; session records are never edited or removed"
                          % ("drops" if path not in after else "changes", path))
        for path, oid in sorted(self.added_files.items()):
            if path in before:
                continue
            if path not in after:
                self.find(rev, subject, "drops %s, which %s added; session records are never removed" % (path, self.base[:12] + ".." + rev[:12]))
            elif after[path] != oid:
                self.find(rev, subject, "changes %s after the commit that added it (a merge did)" % path)
        for path in sorted(set(after) - set(before) - set(self.added_files)):
            if base_tip.get(path) != after[path]:
                self.find(rev, subject, "%s arrives without a commit of %s..%s that adds it alone (a merge made it)"
                          % (path, self.base[:12], rev[:12]))
        new = collections.Counter(lines(self.repo.blob(rev, ARCHIVE)))
        held = collections.Counter(lines(self.repo.blob(self.base, ARCHIVE)))
        for mb in bases:
            old = collections.Counter(lines(self.repo.blob(mb, ARCHIVE)))
            if old - new:
                self.find(rev, subject, "the branch removes or rewrites %d line(s) of the archive %s held at the merge base"
                          % (sum((old - new).values()), ARCHIVE))
            held |= old
        arrivals = new - held
        for line in arrivals:
            if line in self.line_problems:
                sha, commit_subject, problem = self.line_problems[line]
                self.find(sha, commit_subject, problem)
            elif arrivals[line] > self.added_lines[line]:
                self.find(rev, subject, "the archive gains a line that no commit of %s..%s added the old way (a merge made it): %s"
                          % (self.base[:12], rev[:12], line[:120].decode("utf-8", "replace")))
        # Each new session recorded once.
        counts = collections.Counter(session_of(l) for l in new.elements())
        data = self.repo.blobs(oid for _path, oid in regular(after))
        for path, oid in regular(after):
            counts[session_of(data[oid])] += 1
        new_ids = set(session_of(l) for l in arrivals)
        new_ids |= set(session_of(data[oid]) for p, oid in regular(after) if p not in before)
        new_ids.discard(None)
        for session in sorted(new_ids):
            if counts[session] > 1:
                self.find(rev, subject, "session %s is recorded %d times at %s; each session has one record"
                          % (session, counts[session], rev[:12]))
        counts.pop(None, None)
        return counts

    def check_directory(self, sha, subject):
        entry = self.repo.entry(sha, RECORDS[:-1])
        if entry is not None and entry[1] != "tree":
            self.find(sha, subject, "%s is a %s (mode %s), not the session records directory" % (RECORDS[:-1], entry[1], entry[0]))

    def check_merge(self, sha, parents, subject):
        """A merge keeps every record and archive line of each parent, as it
        is, and brings none of its own."""
        self.check_directory(sha, subject)
        files = self.repo.record_files(sha)
        mine = collections.Counter(lines(self.repo.blob(sha, ARCHIVE)))
        known, held = set(), collections.Counter()
        for parent in parents:
            for path, entry in sorted(self.repo.record_files(parent).items()):
                known.add(path)
                if files.get(path) != entry:
                    self.find(sha, subject, "%s %s, which its parent %s holds; session records are never edited or removed"
                              % ("drops" if path not in files else "changes", path, parent[:12]))
            theirs = collections.Counter(lines(self.repo.blob(parent, ARCHIVE)))
            if theirs - mine:
                self.find(sha, subject, "removes or rewrites %d line(s) of the archive %s that its parent %s holds"
                          % (sum((theirs - mine).values()), ARCHIVE, parent[:12]))
            held |= theirs
        for path in sorted(set(files) - known):
            self.find(sha, subject, "adds %s in a merge; a session record is committed alone" % path)
        if mine - held:
            self.find(sha, subject, "adds %d archive line(s) in a merge; a session record is committed alone"
                      % sum((mine - held).values()))

    def run(self):
        bases = self.repo.out("merge-base", "--all", self.base, self.rev).decode().split()
        if not bases:
            raise CannotCheck("%s and %s have no merge base" % (self.base, self.rev))
        commits = list(self.commits())
        for sha, parents, subject, trailers in commits:
            if len(parents) <= 1:
                self.check_commit(sha, parents, subject, trailers)
            else:
                self.check_merge(sha, parents, subject)
        tip_subject = self.repo.out("log", "-1", "--format=%s", self.rev).decode("utf-8", "replace").strip()
        counts = self.check_range(bases, tip_subject)
        # A record the base branch gained since the branch left it counts too.
        known = set(s for s, n in counts.items() if n) | recorded_ids(self.repo, self.base)
        for sha, parents, subject, trailers in commits:
            for session in trailer_values(trailers).get("session", []):
                # A record commit's own trailer is checked with its record.
                if session not in known and (sha, session) not in self.own:
                    self.find(sha, subject, "Session: %s names no session record committed at %s or %s (%s or %s<id>.json)"
                              % (session, self.rev[:12], self.base[:12], ARCHIVE, RECORDS))
        return list(self.findings.values())


def recorded_ids(repo, rev):
    """The session ids committed at <rev>, archive and directory."""
    ids = set(session_of(l) for l in lines(repo.blob(rev, ARCHIVE)))
    files = repo.record_files(rev)
    ids |= set(session_of(d) for d in repo.blobs(oid for _path, oid in regular(files)).values())
    ids.discard(None)
    return ids


def records(repo, rev):
    """Every record committed at <rev>, one compact JSON line each: the
    archive's lines, then each regular <id>.json directly under RECORDS whose
    session is its <id> (anything else there is no record). A line or file
    that is not one JSON object is an error."""
    out = []
    for raw in lines(repo.blob(rev, ARCHIVE)):
        record = parse(raw)
        if record is None:
            raise CannotCheck("%s holds a line that is not one JSON object" % ARCHIVE)
        out.append(record)
    files = repo.record_files(rev)
    data = repo.blobs(oid for _path, oid in regular(files))
    for path, oid in sorted(regular(files)):
        name = path[len(RECORDS):]
        if "/" in name or not name.endswith(".json"):
            continue
        record = parse(data[oid])
        if record is None:
            raise CannotCheck("%s is not one JSON object" % path)
        if record.get("session") == name[:-5]:
            out.append(record)
    return out


def main(argv):
    if len(argv) == 3 and argv[0] == "--records":
        repo = Repo(argv[1])
        try:
            rev = repo.commit(argv[2])
            if rev is None:
                raise CannotCheck("%s is not a commit" % argv[2])
            found = records(repo, rev)
        except Exception as e:
            print("sessions: %s" % e, file=sys.stderr)
            return 2
        for record in found:
            print(json.dumps(record, separators=(",", ":")))
        return 0
    if len(argv) != 3:
        print("usage: sessions.py <checkout> <base> <rev> | --records <checkout> <rev>", file=sys.stderr)
        return 2
    repo = Repo(argv[0])
    try:
        base, rev = repo.commit(argv[1]), repo.commit(argv[2])
        if base is None or rev is None:
            raise CannotCheck("%s is not a commit" % (argv[1] if base is None else argv[2]))
        findings = Check(repo, base, rev).run()
    except CannotCheck as e:
        print("sessions: %s" % e, file=sys.stderr)
        return 2
    except Exception as e:  # a bug here is "cannot check", never a finding
        print("sessions: internal error: %r" % e, file=sys.stderr)
        return 2
    for f in findings:
        print(json.dumps(f, sort_keys=True))
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
