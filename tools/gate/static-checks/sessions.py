#!/usr/bin/env python3
"""Session records over a branch (R1958, docs/process/provenance.md).

    sessions.py <checkout> <base> <rev>

Records live one per file, docs/process/provenance/sessions/<id>.json, and
are only ever added. docs/process/provenance/sessions.jsonl is the archive of
the records made before the switch to that layout. For <base>..<rev>:

- A commit may add a record file, never modify, delete or rename one. A
  commit that adds one changes nothing else and carries `Role: infra` and
  `Session: <id>`. The record is one JSON object whose session is its file
  name, with a known role, its role's instructions (role_doc, role_doc_blob at
  base_commit), a base_commit that the commit descends from and a created_at.
- The archive only grows: no line of it is removed or changed. Until the
  layout's transition ends, a branch made before the switch may still add
  lines, each a record created before ARCHIVE_CUTOFF; nothing else may.
- The same holds between the merge base and <rev>, so a merge cannot drop,
  change or slip in a record either.
- Each session id is recorded once, and every `Session:` trailer names a
  record committed at <rev> or <base> (archive or directory).

Prints one JSON line per finding ({check, commit, key, subject, message}); exits
1 when there is one, 2 when it cannot check. Stdlib only, for macOS python3.
"""

import collections
import hashlib
import json
import re
import subprocess
import sys

ARCHIVE = "docs/process/provenance/sessions.jsonl"
RECORDS = "docs/process/provenance/sessions/"
# Transition rule: a line appended to the archive must be a record created
# before this instant (tools/mg before the switch appended there). A branch
# made before the switch merges whenever it lands; records made after it go
# to RECORDS. Set when the per-session layout landed (2026-11-20) plus a
# week for branches still on the old tools.
ARCHIVE_CUTOFF = "2026-11-27T00:00:00Z"

ROLES = ("spec-writer", "implementer", "fixer", "oracle-maintainer", "infra", "critic", "integrator")
ROLE_DOCS = {
    "fixer": "docs/process/roles/implementer.md",
    "spec-writer": "docs/process/roles/spec-writer.md",
    "implementer": "docs/process/roles/implementer.md",
    "oracle-maintainer": "docs/process/roles/oracle-maintainer.md",
    "critic": "docs/process/roles/critic.md",
}
ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{2,}")
STAMPED_ID = re.compile(r"s-(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z-[0-9a-f]{8}")
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
        proc = self.run("rev-parse", "-q", "--verify", rev + "^{commit}", ok=(0, 1))
        return proc.stdout.decode().strip() if proc.returncode == 0 else None

    def blob(self, rev, path):
        """The bytes of <rev>:<path>, or None when there is no such file."""
        proc = self.run("cat-file", "-e", "%s:%s" % (rev, path), ok=(0, 1, 128))
        if proc.returncode != 0:
            return None
        return self.out("cat-file", "blob", "%s:%s" % (rev, path))

    def blob_id(self, rev, path):
        proc = self.run("rev-parse", "-q", "--verify", "%s:%s" % (rev, path), ok=(0, 1, 128))
        return proc.stdout.decode().strip() if proc.returncode == 0 else None

    def is_ancestor(self, a, b):
        return self.run("merge-base", "--is-ancestor", a, b, ok=(0, 1, 128)).returncode == 0

    def record_files(self, rev):
        """{path: blob id} of the record directory at <rev>."""
        files = {}
        for entry in self.out("ls-tree", "-r", "-z", rev, "--", RECORDS).split(b"\0"):
            if not entry:
                continue
            meta, path = entry.split(b"\t", 1)
            files[path.decode("utf-8", "surrogateescape")] = meta.split(b" ")[2].decode()
        return files

    def blobs(self, ids):
        """{blob id: bytes}, read through one git cat-file --batch."""
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


def parse(raw):
    try:
        value = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, ValueError):
        return None
    return value if isinstance(value, dict) else None


def id_instant(session):
    m = STAMPED_ID.fullmatch(session)
    return "%s-%s-%sT%s:%s:%sZ" % m.groups() if m else None


class Check:
    def __init__(self, repo, base, rev):
        self.repo = repo
        self.base, self.rev = base, rev
        self.findings = collections.OrderedDict()
        self.added_by_commit = set()

    def find(self, commit, subject, message):
        key = hashlib.sha256(message.encode("utf-8", "surrogateescape")).hexdigest()[:10]
        self.findings.setdefault((commit, message), {"check": "sessions", "commit": commit, "key": key,
                                                    "subject": subject, "message": message})

    # -- records ---------------------------------------------------------

    def record_problem(self, path, data, commit):
        """Why the record file <path> (bytes <data>) added by <commit> is not
        a valid record, or None."""
        name = path[len(RECORDS):]
        if "/" in name or not name.endswith(".json") or not ID.fullmatch(name[:-5]):
            return "%s is not a record file (%s<session id>.json)" % (path, RECORDS)
        record = parse(data)
        if record is None:
            return "%s is not one JSON object" % path
        if record.get("session") != name[:-5]:
            return "%s holds session %s, not its file name" % (path, json.dumps(record.get("session")))
        role = record.get("role")
        if role not in ROLES:
            return "%s has role %s, not one of: %s" % (path, json.dumps(role), " ".join(ROLES))
        if record.get("kind") not in ("worktree", "session"):
            return "%s has kind %s, not worktree or session" % (path, json.dumps(record.get("kind")))
        if not isinstance(record.get("created_at"), str) or not INSTANT.fullmatch(record["created_at"]):
            return "%s has no created_at (YYYY-MM-DDTHH:MM:SSZ)" % path
        base = record.get("base_commit")
        base_sha = self.repo.commit(base) if isinstance(base, str) and base else None
        if base_sha is None or not self.repo.is_ancestor(base_sha, commit):
            return "%s has base_commit %s, which is not a commit this one descends from" % (path, json.dumps(base))
        doc = ROLE_DOCS.get(role)
        if record.get("role_doc") != doc:
            return "%s has role_doc %s; role %s works under %s" % (path, json.dumps(record.get("role_doc")), role, json.dumps(doc))
        blob = self.repo.blob_id(base_sha, doc) if doc else None
        if record.get("role_doc_blob") != blob:
            return "%s has role_doc_blob %s, not %s's blob at its base_commit (%s)" % (
                path, json.dumps(record.get("role_doc_blob")), doc, json.dumps(blob))
        return None

    def archive_problem(self, raw):
        """Why an archive line added on this branch breaks the transition
        rule, or None."""
        record = parse(raw)
        shown = raw[:120].decode("utf-8", "replace")
        if record is None:
            return "the archive %s gains a line that is not one JSON object: %s" % (ARCHIVE, shown)
        session, created = record.get("session"), record.get("created_at")
        stamp = id_instant(session) if isinstance(session, str) else None
        if stamp is None or not isinstance(created, str) or not INSTANT.fullmatch(created):
            return "the archive %s gains a line without a session id s-<UTC time>-<8 hex> and a created_at: %s" % (ARCHIVE, shown)
        if created >= ARCHIVE_CUTOFF or stamp >= ARCHIVE_CUTOFF:
            return ("the archive %s gains session %s, created %s; only records created before %s may be added "
                    "there (branches made before the switch). Merge the base branch and record the session "
                    "again: tools/mg now writes %s%s.json" % (ARCHIVE, session, created, ARCHIVE_CUTOFF, RECORDS, session))
        return None

    # -- per commit --------------------------------------------------------

    def commits(self):
        out = self.repo.out("log", "-z", "--reverse", "--format=%H%x1f%P%x1f%s%x1f%(trailers:only,unfold)",
                            "%s..%s" % (self.base, self.rev)).decode("utf-8", "replace")
        for entry in out.split("\0"):
            if not entry.strip():
                continue
            sha, parents, subject, trailers = entry.strip("\n").split("\x1f", 3)
            yield sha, parents.split(), subject, trailers

    def check_commit(self, sha, parents, subject, trailers):
        parent = parents[0] if parents else None
        raw = self.repo.out("diff-tree", "-r", "-z", "--root", "--no-renames", "--no-commit-id",
                            "--name-status", sha).split(b"\0")
        changes = [(raw[i].decode(), raw[i + 1].decode("utf-8", "surrogateescape"))
                   for i in range(0, len(raw) - 1, 2)]
        values = trailer_values(trailers)
        for status, path in changes:
            if path.startswith(RECORDS):
                if status != "A":
                    verb = {"D": "deletes", "M": "modifies", "T": "changes the type of"}.get(status, "changes")
                    self.find(sha, subject, "%s %s; session records are only ever added" % (verb, path))
                    continue
                self.added_by_commit.add(path)
                session = path[len(RECORDS):-5] if path.endswith(".json") else path
                problem = self.record_problem(path, self.repo.blob(sha, path), sha)
                if problem:
                    self.find(sha, subject, problem)
                if len(changes) != 1:
                    self.find(sha, subject, "adds %s together with other changes; a session record is committed alone" % path)
                if session not in values.get("session", []) or values.get("role") != ["infra"]:
                    self.find(sha, subject, "adds %s without the trailers Role: infra and Session: %s" % (path, session))
                if parent and session in archive_ids(self.repo, parent):
                    self.find(sha, subject, "adds %s, but the archive already records session %s" % (path, session))
            elif path == ARCHIVE:
                if status == "D":
                    self.find(sha, subject, "deletes the archive %s; session records are never removed" % ARCHIVE)
                    continue
                old = collections.Counter(lines(self.repo.blob(parent, ARCHIVE) if parent else None))
                new = collections.Counter(lines(self.repo.blob(sha, ARCHIVE)))
                if old - new:
                    self.find(sha, subject, "removes or rewrites %d line(s) of the archive %s; session records are never edited or removed"
                              % (sum((old - new).values()), ARCHIVE))
                for line in (new - old):
                    problem = self.archive_problem(line)
                    if problem:
                        self.find(sha, subject, problem)

    # -- merge base to rev -------------------------------------------------

    def check_range(self, base, subject):
        rev = self.rev
        before, after = self.repo.record_files(base), self.repo.record_files(rev)
        for path, oid in sorted(before.items()):
            if after.get(path) != oid:
                self.find(rev, subject, "%s %s, which %s holds; session records are never edited or removed"
                          % ("drops" if path not in after else "changes", path, base[:12]))
        for path in sorted(set(after) - set(before)):
            if path not in self.added_by_commit:
                self.find(rev, subject, "%s arrives without a commit of %s..%s that adds it alone (a merge made it)"
                          % (path, self.base[:12], rev[:12]))
        old = collections.Counter(lines(self.repo.blob(base, ARCHIVE)))
        new = collections.Counter(lines(self.repo.blob(rev, ARCHIVE)))
        if old - new:
            self.find(rev, subject, "the branch removes or rewrites %d line(s) of the archive %s held at %s"
                      % (sum((old - new).values()), ARCHIVE, base[:12]))
        for line in (new - old):
            problem = self.archive_problem(line)
            if problem:
                self.find(rev, subject, problem)
        # Each session recorded once, and every new one counted.
        counts = archive_ids(self.repo, rev, counter=True)
        data = self.repo.blobs(sorted(set(after.values())))
        for path, oid in after.items():
            record = parse(data[oid])
            if record is not None and isinstance(record.get("session"), str):
                counts[record["session"]] += 1
        new_ids = set()
        for line in (new - old):
            record = parse(line)
            if record is not None and isinstance(record.get("session"), str):
                new_ids.add(record["session"])
        for path in set(after) - set(before):
            record = parse(data[after[path]])
            if record is not None and isinstance(record.get("session"), str):
                new_ids.add(record["session"])
        for session in sorted(new_ids):
            if counts[session] > 1:
                self.find(rev, subject, "session %s is recorded %d times at %s; each session has one record"
                          % (session, counts[session], rev[:12]))
        return counts

    def run(self):
        merge_base = self.repo.out("merge-base", self.base, self.rev).decode().strip()
        if not merge_base:
            raise CannotCheck("%s and %s have no merge base" % (self.base, self.rev))
        commits = list(self.commits())
        for sha, parents, subject, trailers in commits:
            if len(parents) <= 1:
                self.check_commit(sha, parents, subject, trailers)
        tip_subject = self.repo.out("log", "-1", "--format=%s", self.rev).decode("utf-8", "replace").strip()
        counts = self.check_range(merge_base, tip_subject)
        # A record the base branch gained since the branch left it counts too.
        known = set(s for s, n in counts.items() if n) | recorded_ids(self.repo, self.base)
        for sha, parents, subject, trailers in commits:
            if len(parents) > 1:
                continue
            for session in trailer_values(trailers).get("session", []):
                if session not in known:
                    self.find(sha, subject, "Session: %s names no session record committed at %s or %s (%s or %s<id>.json)"
                              % (session, self.rev[:12], self.base[:12], ARCHIVE, RECORDS))
        return list(self.findings.values())


def archive_ids(repo, rev, counter=False):
    ids = collections.Counter()
    for raw in lines(repo.blob(rev, ARCHIVE)):
        record = parse(raw)
        if record is not None and isinstance(record.get("session"), str):
            ids[record["session"]] += 1
    return ids if counter else set(ids)


def recorded_ids(repo, rev):
    """The session ids committed at <rev>, archive and directory."""
    ids = archive_ids(repo, rev)
    files = repo.record_files(rev)
    for data in repo.blobs(sorted(set(files.values()))).values():
        record = parse(data)
        if record is not None and isinstance(record.get("session"), str):
            ids.add(record["session"])
    return ids


def trailer_values(trailers):
    """{lower-case key: [values]} of `Key: value` trailer lines."""
    values = collections.defaultdict(list)
    for line in trailers.splitlines():
        key, sep, value = line.partition(":")
        if sep and key.strip():
            for v in re.split(r"[\s,]+", value) if key.strip().lower() == "session" else [value]:
                if v.strip():
                    values[key.strip().lower()].append(v.strip())
    return values


def main(argv):
    if len(argv) != 3:
        print("usage: sessions.py <checkout> <base> <rev>", file=sys.stderr)
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
