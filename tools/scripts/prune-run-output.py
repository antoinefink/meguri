#!/usr/bin/env python3
"""tools/mg prune: report, and with --apply delete, old tools/mg run output.

Called by tools/mg.d/prune.sh on the host. It reports by default and deletes
only with --apply, only below build/mg/ of the checkout the job runs in. The
retention rule is the block of constants below; tools/AGENTS.md describes it.
Standard library only, for macOS /usr/bin/python3 (3.9).
"""

import argparse
import concurrent.futures
import datetime
import fcntl
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
import tarfile
import threading
import time

# ---------------------------------------------------------------------------
# The retention rule. This block is the only place it is stated.

KEEP_NEWEST = 20  # the newest run folders of every job folder
KEEP_DAYS = 7  # every run started, finished or written to (a file at its top) less than 7 days ago
# Always kept as well, whatever their age:
# - the run a job's `latest` link names;
# - a run whose id appears in a file name or content of: any branch, tag or
#   stash entry; a tracked, staged or uncommitted file of any checkout;
#   build/ outside build/mg (handoffs; not BUILD_UNREAD); a build/mg state
#   file (catch-up queue, leases); a top-level .json/.jsonl of a kept run
#   (gate and merge receipts) or of any run in another checkout. --apply reads
#   them all again after planning and keeps whatever either reading keeps;
# - a run without result.json or with .mg/ (running, or killed while it ran),
#   and a run whose tools/mg process (the pid in its id) still runs;
# - a run folder holding a tracked file or a checkout.
# Cleanroom runs (--cleanroom) follow the same rule for their folders, but
# lose source/ and codex-home/ once their records are verified in an archive,
# unless they must stay whole (cleanroom_whole: not passed, no patch, a base
# no branch, tag or worktree HEAD reaches, a bad receipt, unexpected files, or
# a write in the last CLEANROOM_QUIET seconds).

CLEANROOM = "cleanroom"
PRUNE = "prune"
ARCHIVE_DIR = "cleanroom-archive"
# A cleanroom run's records: archived, one .tar.xz per month, then deleted.
CLEANROOM_RECORDS = (
    "prompt.md", "changes.patch", "seed.patch", "result.json", "source-view.json",
    "log.txt", "codex.log", "codex.out", "guard-marker",
)
# Runtime state: deleted, never archived (codex-home holds Codex's
# credentials while a run lasts, its session transcripts and shell snapshots).
CLEANROOM_RUNTIME = ("source", "codex-home")

RUN_ID = re.compile(r"^(\d{8}T\d{6}Z)-(\d+)$")
TOKEN = re.compile(rb"\d{8}T\d{6}Z-\d+")
JOB_DIR = re.compile(r"^[a-z][a-z0-9-]*$")
RECEIPT_SUFFIXES = (".json", ".jsonl")
MEMBER = re.compile(r"^(\d{8}T\d{6}Z-\d+)/([^/]+)$")
STATE_DEPTH = 64  # folder levels read under a state folder; deeper is a read error
# build/ outside build/mg holds handoffs that name runs (orchestration
# checkpoints, prompts, saved logs): read for references, all of it.
# These folders hold only build products, caches and private data copies.
BUILD_UNREAD = ("mg", "private", "docker-cargo", "app", "xcode-check", "swift-test",
                "selftest-xcframework", "oracle-macos-venv", "native-durability")
# Build product folders, at any depth under build/: never read as handoffs.
BUILD_UNREAD_NAMES = ("DerivedData", "SourcePackages", "node_modules", ".build", "target")
BUILD_DEPTH = 64
DU_BATCH = 400
SMALL = 100 << 20  # build/ entries below this are summed in the printed report
USAGE = "usage: tools/mg prune [--cleanroom] [--apply] [--root <checkout>] [--all-worktrees] [--no-docker]"

NOTES = {
    "docker-cargo": "Rust target directories set with MEGURI_DOCKER_CARGO_TARGET; prune never touches them (tools/AGENTS.md: Docker Rust builds, old test binaries)",
    "app": "Xcode derived data and app products of build-app and run; tools/AGENTS.md (Apple compiler caches)",
    "xcode-check": "Xcode derived data of xcode-check; tools/AGENTS.md (Apple compiler caches)",
    "swift-test": "Swift package build output of swift-test; tools/AGENTS.md (Apple compiler caches)",
    "private": "private vault copies; never pruned",
    "selftest-xcframework": "sandbox and Rust target of selftest-xcframework; reused between runs",
}


class Refusal(Exception):
    pass


def log(msg=""):
    print(msg, file=sys.stderr, flush=True)


def human(n):
    n = float(n)
    for unit in ("B", "KiB", "MiB", "GiB", "TiB"):
        if abs(n) < 1024 or unit == "TiB":
            return ("%d %s" % (n, unit)) if unit == "B" else ("%.1f %s" % (n, unit))
        n /= 1024
    return "%.1f TiB" % n


# An exported GIT_DIR or GIT_INDEX_FILE would make every checkout read one
# repository's index; each git child gets the environment without them.
GIT_LOCATION_VARS = ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY",
                     "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_COMMON_DIR", "GIT_NAMESPACE")
GIT_ENV = {k: v for k, v in os.environ.items() if k not in GIT_LOCATION_VARS}


def git(args, cwd, stdin=None, input=None):
    p = subprocess.run(["git", "-C", cwd] + args, stdin=stdin, input=input, stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE, env=GIT_ENV)
    if p.returncode != 0:
        raise OSError("git %s failed in %s: %s" % (" ".join(args[:3]), cwd,
                                                   p.stderr.decode(errors="replace").strip()))
    return p.stdout


def lstat(path):
    try:
        return os.lstat(path)
    except FileNotFoundError:
        return None


# ---------------------------------------------------------------------------
# Run ids in files

ANCHOR = re.compile(rb"Z-\d+")


def ids_in(buf):
    """Run ids in a byte string. Searching for "Z-" first is several times
    faster than the full pattern, which matters for gigabytes of receipts."""
    found = []
    for m in ANCHOR.finditer(buf):
        start = m.start() - 15
        if start >= 0:
            t = TOKEN.match(buf, start)
            if t and t.end() == m.end():
                found.append(t.group().decode())
    return found


def scan_stream(f, out):
    tail = b""
    while True:
        chunk = f.read(1 << 20)
        if not chunk:
            return
        buf = tail + chunk
        out.update(ids_in(buf))
        tail = buf[-48:]


class Refs:
    """Run ids found in reference sources, and what could not be read."""

    def __init__(self):
        self.ids = set()
        self.errors = []
        self.lock = threading.Lock()
        self.seen = set()  # (dev, ino) of folders read through a link: links may loop

    def file(self, path, out=None):
        out = self.ids if out is None else out
        out.update(ids_in(os.fsencode(path)))  # an id in a file or folder name counts too
        try:
            st = os.lstat(path)
            if stat.S_ISLNK(st.st_mode):
                out.update(ids_in(os.fsencode(os.readlink(path))))
                # Reading follows a link to a folder (deletion never does).
                try:
                    tst = os.stat(path)
                except OSError:
                    return
                if stat.S_ISDIR(tst.st_mode):
                    with self.lock:
                        if (tst.st_dev, tst.st_ino) in self.seen:
                            return
                        self.seen.add((tst.st_dev, tst.st_ino))
                    self.tree(path, STATE_DEPTH, out)
                return
            if not stat.S_ISREG(st.st_mode):
                return
            fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
            with os.fdopen(fd, "rb") as f:
                scan_stream(f, out)
        except FileNotFoundError:
            pass
        except OSError as e:
            self.errors.append("cannot read %s: %s" % (path, e.strerror or e))

    def tree_or_file(self, path, depth, out):
        st = lstat(path)
        if st is not None and stat.S_ISDIR(st.st_mode):
            self.tree(path, depth, out, handoffs=True)
        else:
            self.file(path, out)

    def tree(self, path, depth, out, handoffs=False):
        """Run ids in the names and contents below path. handoffs: a build/
        folder outside build/mg, whose build product folders are skipped."""
        out.update(ids_in(os.fsencode(path)))  # the folder's own name counts too
        try:
            entries = list(os.scandir(path))
        except FileNotFoundError:
            return
        except OSError as e:
            self.errors.append("cannot list %s: %s" % (path, e.strerror or e))
            return
        for e in entries:
            try:
                st = e.stat(follow_symlinks=False)
            except FileNotFoundError:
                continue
            except OSError as err:
                self.errors.append("cannot read %s: %s" % (e.path, err.strerror or err))
                continue
            if stat.S_ISDIR(st.st_mode):
                out.update(ids_in(os.fsencode(e.path)))  # an id in a folder name counts too
                if handoffs and e.name in BUILD_UNREAD_NAMES:
                    continue
                if depth > 1:
                    self.tree(e.path, depth - 1, out, handoffs)
                else:
                    self.errors.append("%s is nested deeper than prune reads" % e.path)
            else:
                self.file(e.path, out)


def scan_exact(f, size, out):
    """Run ids in the next size bytes of f, read in pieces."""
    tail = b""
    left = size
    while left > 0:
        chunk = f.read(min(left, 1 << 20))
        if not chunk:
            raise OSError("short read")
        left -= len(chunk)
        buf = tail + chunk
        out.update(ids_in(buf))
        tail = buf[-48:]


def git_refs(repo, heads, staged, refs, scratch):
    """Run ids in every object of every local and remote branch tip and worktree HEAD
    (their commits, trees and files), and in every worktree's staged files."""
    out = git(["for-each-ref", "--format=%(objectname) %(objecttype) %(*objectname)",
               "refs/heads", "refs/remotes", "refs/tags"], repo)
    commits = set(h for h in heads if h)
    for line in out.decode().splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1] == "commit":
            commits.add(parts[0])
        elif len(parts) == 3 and parts[1] == "tag":
            commits.add(parts[2])
    # Every stash entry (shared by all worktrees): its work tree, index and untracked commits.
    if subprocess.run(["git", "-C", repo, "rev-parse", "--verify", "-q", "refs/stash"],
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=GIT_ENV).returncode == 0:
        commits.update(git(["log", "-g", "--format=%H %P", "refs/stash"], repo).decode().split())
    listing = os.path.join(scratch, "objects.txt")
    with open(listing, "wb") as f:
        f.write("".join(c + "\n" for c in sorted(commits)).encode())
    with open(listing, "rb") as f:
        objects = git(["rev-list", "--objects", "--no-walk", "--stdin"], repo, stdin=f)
    wanted = set(line.split(b" ", 1)[0].decode() for line in objects.splitlines() if line)
    wanted |= staged
    with open(listing, "wb") as f:
        f.write("".join(o + "\n" for o in sorted(wanted)).encode())
    with open(listing, "rb") as f, open(listing + ".err", "wb") as err:
        p = subprocess.Popen(["git", "-C", repo, "cat-file", "--batch"], stdin=f,
                             stdout=subprocess.PIPE, stderr=err, env=GIT_ENV)
        while True:
            header = p.stdout.readline()
            if not header:
                break
            parts = header.split()
            if len(parts) != 3:
                p.kill()
                raise OSError("git cat-file --batch: unexpected %r" % header[:80])
            scan_exact(p.stdout, int(parts[2]) + 1, refs.ids)
        if p.wait() != 0:
            raise OSError("git cat-file --batch failed")
    os.unlink(listing)
    os.unlink(listing + ".err")
    return len(commits), len(wanted)


def worktree_changes(checkout, refs, out_ids, staged):
    """Run ids in a checkout's modified and untracked (not ignored) files; the
    blobs of its index (staged files) go to staged, for git_refs."""
    out = git(["ls-files", "-z", "-m", "-o", "--exclude-standard"], checkout)
    for rel in out.split(b"\0"):
        if rel:
            refs.file(os.path.join(checkout, os.fsdecode(rel)), out_ids)
    for entry in git(["ls-files", "-z", "-s"], checkout).split(b"\0"):
        meta, _, name = entry.partition(b"\t")
        meta = meta.split()
        with refs.lock:
            out_ids.update(ids_in(name))  # an id in a staged file's name
            if len(meta) == 3 and not meta[0].startswith(b"160"):  # not a submodule
                staged.add(meta[1].decode())


# ---------------------------------------------------------------------------
# Checkouts and runs

class Run:
    def __init__(self, checkout, job, run_id, path):
        self.checkout = checkout
        self.job = job
        self.id = run_id
        self.path = path
        m = RUN_ID.match(run_id)
        self.started = datetime.datetime.strptime(m.group(1), "%Y%m%dT%H%M%SZ").replace(
            tzinfo=datetime.timezone.utc).timestamp()
        self.pid = int(m.group(2))
        self.entries = {}
        self.result_mtime = None
        self.written = 0  # newest mtime of a file at the top of the folder (a log still growing)
        self.result = None
        self.receipts = set()
        self.parts = {}
        self.bytes = 0
        self.reasons = []
        self.action = None

    @property
    def key(self):
        return (self.job, self.id)

    @property
    def finished(self):
        return self.result_mtime is not None and ".mg" not in self.entries

    def load(self, refs):
        for e in os.scandir(self.path):
            # d_type answers without a stat call; only result.json needs one.
            if e.is_symlink():
                mode = stat.S_IFLNK
            elif e.is_dir(follow_symlinks=False):
                mode = stat.S_IFDIR
            elif e.is_file(follow_symlinks=False):
                mode = stat.S_IFREG
            else:
                mode = 0
            self.entries[e.name] = mode
            if mode != stat.S_IFREG:
                continue
            mtime = e.stat(follow_symlinks=False).st_mtime
            self.written = max(self.written, mtime)
            if e.name == "result.json":
                self.result_mtime = mtime
            # prune's own receipts list every run it considered: never references.
            if e.name.endswith(RECEIPT_SUFFIXES) and self.job != PRUNE:
                refs.file(e.path, self.receipts)
        self.receipts.discard(self.id)
        if self.job == CLEANROOM and self.result_mtime is not None:
            try:
                with open(os.path.join(self.path, "result.json"), "rb") as f:
                    self.result = valid_receipt(json.load(f))
            except (OSError, ValueError):
                self.result = None


def valid_receipt(res):
    """A cleanroom result.json with the fields prune reads, or None."""
    if not isinstance(res, dict) or not isinstance(res.get("status"), str):
        return None
    if not isinstance(res.get("args", []), list) or not isinstance(res.get("details", {}), dict):
        return None
    commit = (res.get("details") or {}).get("source_commit")
    if commit is not None and not (isinstance(commit, str) and re.match(r"^[0-9a-f]{40}$", commit)):
        return None
    return res


class Checkout:
    def __init__(self, path):
        self.path = path
        self.mg = os.path.join(path, "build", "mg")
        self.runs = []
        self.latest = {}  # job -> run id
        self.state = set()  # run ids in build/mg state files
        self.skipped = []  # entries prune never follows
        self.job_dirs = []  # (job, folder): scan() lists them, scan_job() reads each
        self.pending = []  # (job, name) of run folders an --apply renamed and did not finish

    def scan(self, refs):
        st = lstat(self.mg)
        if st is None:
            return
        if not stat.S_ISDIR(st.st_mode):
            self.skipped.append({"path": self.mg, "why": "build/mg is not a directory (a symbolic link is never followed)"})
            refs.file(self.mg, self.state)
            if os.path.isdir(self.mg):  # references behind the link still count
                refs.tree(self.mg, STATE_DEPTH + 2, self.state)
            return
        for e in sorted(os.scandir(self.mg), key=lambda e: e.name):
            est = e.stat(follow_symlinks=False)
            if stat.S_ISREG(est.st_mode) or stat.S_ISLNK(est.st_mode):
                refs.file(e.path, self.state)
                if stat.S_ISLNK(est.st_mode):
                    self.skipped.append({"path": e.path, "why": "symbolic link; never followed"})
                    if os.path.isdir(e.path):
                        refs.tree(e.path, STATE_DEPTH + 1, self.state)
                continue
            if not stat.S_ISDIR(est.st_mode):
                continue
            if e.name == ARCHIVE_DIR:
                continue
            if not JOB_DIR.match(e.name):
                refs.tree(e.path, STATE_DEPTH, self.state)
                continue
            self.job_dirs.append((e.name, e.path))

    def build_entries(self):
        """Entries of build/ outside build/mg to read for references."""
        try:
            return [e.path for e in os.scandir(os.path.join(self.path, "build")) if e.name not in BUILD_UNREAD]
        except FileNotFoundError:
            return []

    def scan_job(self, job, path, refs):
        for c in sorted(os.scandir(path), key=lambda c: c.name):
            cst = c.stat(follow_symlinks=False)
            m = RUN_ID.match(c.name)
            if c.name.startswith(PENDING) and RUN_ID.match(c.name[len(PENDING):]) and stat.S_ISDIR(cst.st_mode):
                self.pending.append((job, c.name))
                continue
            if stat.S_ISLNK(cst.st_mode):
                target = os.readlink(c.path)
                self.state.update(ids_in(os.fsencode(target)))
                self.state.update(ids_in(os.fsencode(c.path)))  # the link's own name
                if c.name == "latest" and RUN_ID.match(os.path.basename(target)):
                    self.latest[job] = os.path.basename(target)
                elif m:
                    self.skipped.append({"path": c.path, "why": "run folder is a symbolic link; never followed"})
                    if os.path.isdir(c.path):
                        refs.tree(c.path, STATE_DEPTH, self.state)
                continue
            if m and stat.S_ISDIR(cst.st_mode):
                run = Run(self, job, c.name, c.path)
                try:
                    run.load(refs)
                except OSError as err:
                    refs.errors.append("cannot list %s: %s" % (c.path, err.strerror or err))
                    run.entries[".unreadable"] = 0
                    run.result_mtime = None
                self.runs.append(run)
            elif stat.S_ISDIR(cst.st_mode):
                refs.tree(c.path, STATE_DEPTH - 1, self.state)
            elif stat.S_ISREG(cst.st_mode):
                refs.file(c.path, self.state)

    def tracked_runs(self):
        """(job, id) of run folders holding a tracked file."""
        out = git(["ls-files", "-z", "--", "build/mg"], self.path)
        held = set()
        for rel in out.split(b"\0"):
            parts = os.fsdecode(rel).split("/")
            if len(parts) >= 4 and RUN_ID.match(parts[3][len(PENDING):] if parts[3].startswith(PENDING) else parts[3]):
                held.add((parts[2], parts[3]))
        return held


def worktree_list(repo):
    out = git(["worktree", "list", "--porcelain", "-z"], repo)
    wts, cur = [], {}
    for field in out.split(b"\0"):
        f = os.fsdecode(field)
        if not f:
            if cur:
                wts.append(cur)
            cur = {}
        elif f.startswith("worktree "):
            cur = {"path": f[len("worktree "):]}
        elif f.startswith("HEAD "):
            cur["head"] = f[len("HEAD "):]
        elif f == "bare":
            cur["bare"] = True
        elif f == "prunable" or f.startswith("prunable "):
            cur["prunable"] = True
    if cur:
        wts.append(cur)
    result, missing = [], []
    for w in wts:
        if w.get("bare"):
            continue
        if w.get("prunable") or not os.path.isdir(w["path"]):
            # Unmounted, moved, or removed without `git worktree prune`; a folder
            # left at the path is not that checkout.
            missing.append(w["path"])
            continue
        w["path"] = os.path.realpath(w["path"])
        result.append(w)
    return result, missing


def processes():
    p = subprocess.run(["ps", "-A", "-o", "pid=", "-o", "command="], stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL)
    procs = {}
    for line in p.stdout.decode(errors="replace").splitlines():
        pid, _, cmd = line.strip().partition(" ")
        if pid.isdigit():
            procs[int(pid)] = cmd
    if p.returncode != 0 or not procs:
        raise OSError("ps could not list processes")
    return procs


def runs_mg(cmd):
    return any(os.path.basename(w) == "mg" for w in cmd.split())


# ---------------------------------------------------------------------------
# Sizes

def du(paths):
    """Disk usage in bytes of each path (du -skx: no symbolic links, one file system)."""
    sizes = {}
    for i in range(0, len(paths), DU_BATCH):
        part = paths[i:i + DU_BATCH]
        p = subprocess.run(["du", "-skx", "--"] + part, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        for line in p.stdout.decode(errors="replace").splitlines():
            kb, _, path = line.partition("\t")
            if kb.isdigit():
                sizes[path] = int(kb) * 1024
    return sizes


def measure(runs, by_part):
    paths = []
    for r in runs:
        if by_part:
            paths.extend(os.path.join(r.path, n) for n in sorted(r.entries))
        else:
            paths.append(r.path)
    sizes = du(paths)
    for r in runs:
        if by_part:
            r.parts = {n: sizes.get(os.path.join(r.path, n), 0) for n in r.entries}
            r.bytes = sum(r.parts.values())
        else:
            r.bytes = sizes.get(r.path, 0)


# ---------------------------------------------------------------------------
# Plans

class Context:
    def __init__(self, now, procs):
        self.now = now
        self.procs = procs
        self.refs = Refs()
        self.checkouts = []
        self.reachable = set()
        self.ref_stats = {}

    def external(self, target):
        """Run ids that protect target's runs, from everything but target's runs."""
        ids = set(self.refs.ids)
        for ck in self.checkouts:
            ids |= ck.state
            if ck is not target:
                for r in ck.runs:
                    ids |= r.receipts
        return ids

    def live(self, run):
        cmd = self.procs.get(run.pid)
        return cmd is not None and runs_mg(cmd)


def protect(ctx, ck, cands, others, tracked, whole=None):
    """Give each candidate run its keep reasons (empty: it may go). whole(r)
    gives reasons a run stays whole; they count before the receipt closure."""
    refs = ctx.external(ck)
    for r in others:
        refs |= r.receipts
    newest = set()
    by_job = {}
    for r in cands:
        by_job.setdefault(r.job, []).append(r)
    for job_runs in by_job.values():
        job_runs.sort(key=lambda r: (r.started, r.pid), reverse=True)
        newest.update(r.key for r in job_runs[:KEEP_NEWEST])
    horizon = ctx.now - KEEP_DAYS * 86400
    for r in cands:
        r.reasons = []
        if not r.finished:
            r.reasons.append("unfinished")
        if ctx.live(r):
            r.reasons.append("live")
        if r.key in tracked:
            r.reasons.append("tracked")
        if ck.latest.get(r.job) == r.id:
            r.reasons.append("latest")
        if r.key in newest:
            r.reasons.append("newest")
        if max(r.started, r.written) > horizon:
            r.reasons.append("recent")
        if whole is not None:
            r.reasons.extend(whole(r))
    for r in cands:
        if r.reasons:
            refs |= r.receipts
    changed = True
    while changed:
        changed = False
        for r in cands:
            if not r.reasons and r.id in refs:
                r.reasons.append("referenced")
                refs |= r.receipts
                changed = True
    for r in cands:
        if r.id in refs and "referenced" not in r.reasons:
            r.reasons.append("referenced")


def plan_runs(ctx, ck, tracked):
    cands = [r for r in ck.runs if r.job != CLEANROOM]
    others = [r for r in ck.runs if r.job == CLEANROOM]
    protect(ctx, ck, cands, others, tracked)
    for r in cands:
        r.action = "keep" if r.reasons else "delete"
    return cands


CLEANROOM_QUIET = 3600  # s without a write before a cleanroom run loses source/ and codex-home/
KEPT_WHOLE = "kept whole: "


def cleanroom_whole(ctx, r):
    """Why a cleanroom run must stay whole (records, source/ and codex-home/), if it must."""
    why = []
    unknown = sorted(n for n in r.entries if n not in CLEANROOM_RECORDS + CLEANROOM_RUNTIME + (".mg",))
    if unknown:
        why.append("unexpected entries: " + ", ".join(unknown))
    bad = sorted(n for n in CLEANROOM_RECORDS if n in r.entries and not stat.S_ISREG(r.entries[n]))
    bad += sorted(n for n in CLEANROOM_RUNTIME if n in r.entries and not stat.S_ISDIR(r.entries[n]))
    if bad:
        why.append("not a plain file or folder: " + ", ".join(bad))
    if not r.finished:
        return why
    res = r.result
    if res is None:
        why.append(KEPT_WHOLE + "result.json is not a valid receipt")
    elif res.get("status") != "pass":
        why.append(KEPT_WHOLE + "did not pass")
    elif "source" in r.entries and res.get("dry_run") is not True and "--check" not in (res.get("args") or []):
        if not stat.S_ISREG(r.entries.get("changes.patch", 0)):
            why.append(KEPT_WHOLE + "exported no changes.patch")
        elif (res.get("details") or {}).get("source_commit") not in ctx.reachable:
            why.append(KEPT_WHOLE + "no branch, tag or HEAD reaches its base commit")
    if any(n in r.entries for n in CLEANROOM_RUNTIME) and r.written > ctx.now - CLEANROOM_QUIET:
        why.append("written to in the last hour")
    return why


def plan_cleanroom(ctx, ck, tracked):
    cands = [r for r in ck.runs if r.job == CLEANROOM]
    others = [r for r in ck.runs if r.job != CLEANROOM]
    whole = {r.key: cleanroom_whole(ctx, r) for r in cands}
    protect(ctx, ck, cands, others, tracked, lambda r: whole[r.key])
    for r in cands:
        hold = [x for x in r.reasons if x in ("unfinished", "live", "tracked")]
        r.source_ok = not any(x.startswith(KEPT_WHOLE) for x in whole[r.key])
        r.action = "untouched" if hold or whole[r.key] else ("strip" if r.reasons else "remove")
    return cands


def cleanroom_freed(r):
    if r.action == "remove":
        return r.bytes
    if r.action == "strip":
        return r.parts.get("codex-home", 0) + r.parts.get("source", 0)
    return 0


def records_of(r):
    return [n for n in CLEANROOM_RECORDS if n in r.entries]


def summarize_runs(cands):
    jobs = {}
    for r in cands:
        j = jobs.setdefault(r.job, {"job": r.job, "runs": 0, "delete": 0, "keep": 0,
                                    "bytes_freed": 0, "bytes_kept": 0, "kept_because": {}})
        j["runs"] += 1
        if r.action == "delete":
            j["delete"] += 1
            j["bytes_freed"] += r.bytes
        else:
            j["keep"] += 1
            j["bytes_kept"] += r.bytes
            for why in r.reasons:
                j["kept_because"][why] = j["kept_because"].get(why, 0) + 1
    rows = sorted(jobs.values(), key=lambda j: (-j["bytes_freed"], j["job"]))
    total = {k: sum(j[k] for j in rows) for k in ("runs", "delete", "keep", "bytes_freed", "bytes_kept")}
    return rows, total


def summarize_cleanroom(cands):
    s = {"runs": len(cands), "remove": 0, "strip": 0, "untouched": 0,
         "bytes_freed": 0, "bytes_source": 0, "bytes_codex_home": 0, "bytes_records": 0,
         "source_kept": 0, "bytes_source_kept": 0, "source_kept_because": {}, "kept_because": {}, "months": {}, "untouched_runs": []}
    for r in cands:
        s[r.action] += 1
        if r.action == "untouched":
            if not r.source_ok:
                s["source_kept"] += 1
                s["bytes_source_kept"] += r.bytes
                why = next(x for x in r.reasons if x.startswith(KEPT_WHOLE))[len(KEPT_WHOLE):]
                s["source_kept_because"][why] = s["source_kept_because"].get(why, 0) + 1
                continue
            s["untouched_runs"].append({"run": r.id, "why": [x for x in r.reasons if x not in ("newest", "recent", "latest", "referenced")] or r.reasons,
                                        "bytes": r.bytes})
            continue
        s["bytes_freed"] += cleanroom_freed(r)
        s["bytes_codex_home"] += r.parts.get("codex-home", 0)
        s["bytes_source"] += r.parts.get("source", 0)
        rec = sum(r.parts.get(n, 0) for n in records_of(r))
        s["bytes_records"] += rec
        month = "%s-%s" % (r.id[0:4], r.id[4:6])
        m = s["months"].setdefault(month, {"runs": 0, "bytes_records": 0})
        m["runs"] += 1
        m["bytes_records"] += rec
        if r.action == "strip":
            for why in r.reasons:
                s["kept_because"][why] = s["kept_because"].get(why, 0) + 1
    return s


# ---------------------------------------------------------------------------
# Not managed here

def unmanaged(ck, targets, volumes):
    items = []
    build = os.path.join(ck.path, "build")
    try:
        names = sorted(e.name for e in os.scandir(build) if e.name != "mg")
    except OSError:
        names = []
    sizes = du([os.path.join(build, n) for n in names])
    for n in names:
        p = os.path.join(build, n)
        items.append({"path": p, "bytes": sizes.get(p, 0), "note": NOTES.get(n, "not managed by prune")})
    archive = os.path.join(ck.mg, ARCHIVE_DIR)
    if os.path.isdir(archive):
        items.append({"path": archive, "bytes": du([archive]).get(archive, 0),
                      "note": "cleanroom records, one .tar.xz per month; keep"})
    items.sort(key=lambda i: -i["bytes"])
    target = targets.get(ck.path, {}).get("target", "")
    if target.startswith("volume:"):
        name = target[len("volume:"):]
        v = next((v for v in volumes.get("list", []) if v["name"] == name), None)
        why = "" if v else (" (no such volume)" if volumes.get("measured") else " (" + volumes.get("why", "not measured") + ")")
        items.insert(0, {"path": "docker volume " + name, "bytes": v["bytes"] if v else None,
                         "note": "this checkout's Docker Rust target; prune never removes volumes" + why})
    elif target.startswith("dir:"):
        items.insert(0, {"path": target[len("dir:"):], "bytes": None,
                         "note": "this checkout's Docker Rust target (MEGURI_DOCKER_CARGO_TARGET); prune never touches it"})
    return items


def docker_volumes(targets, enabled):
    if not enabled:
        return {"measured": False, "why": "--no-docker", "list": []}
    try:
        p = subprocess.run(["docker", "system", "df", "-v", "--format", "{{json .Volumes}}"],
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=900)
    except (OSError, subprocess.TimeoutExpired) as e:
        return {"measured": False, "why": "docker system df failed: %s" % e, "list": []}
    if p.returncode != 0:
        return {"measured": False, "why": "Docker is not reachable", "list": []}
    try:
        vols = json.loads(p.stdout.decode() or "[]") or []
    except ValueError:
        return {"measured": False, "why": "docker system df printed no JSON", "list": []}
    owners = {t["project"]: path for path, t in targets.items()}
    out = []
    for v in vols:
        name = v.get("Name", "")
        labels = dict(x.split("=", 1) for x in (v.get("Labels") or "").split(",") if "=" in x)
        project = labels.get("com.docker.compose.project", "")
        if not project.startswith("meguri-") and not name.startswith("meguri-"):
            continue
        owner = owners.get(project) or next((path for proj, path in owners.items()
                                             if name.startswith(proj + "_")), None)
        out.append({"name": name, "bytes": size_bytes(v.get("Size", "")), "size": v.get("Size", ""),
                    "in_use": v.get("Links", "0") not in ("0", "", "N/A"), "checkout": owner})
    out.sort(key=lambda v: -(v["bytes"] or 0))
    return {"measured": True, "list": out}


def size_bytes(s):
    m = re.match(r"^([0-9.]+)\s*([kKMGTP]?)B$", s or "")
    if not m:
        return None
    return int(float(m.group(1)) * {"": 1, "k": 1e3, "K": 1e3, "M": 1e6, "G": 1e9, "T": 1e12, "P": 1e15}[m.group(2)])


# ---------------------------------------------------------------------------
# Deleting

O_DIR = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
PENDING = ".prune-deleting-"  # a checked run folder renamed for deletion; --apply finishes it
REFUSED = ".prune-refused-"  # a folder swapped in while it was checked; prune never deletes it


def remove_at(dfd, name, dev, inside=False, ident=None):
    """Delete entry name of the open folder dfd with all it holds. Every step goes
    through an open descriptor (O_NOFOLLOW), so a folder swapped for a link while
    this runs is never followed, and another file system is never entered.
    inside: dfd is itself part of the tree being deleted, so its permissions
    may be widened to remove name; a job folder's never are."""
    try:
        fd = os.open(name, O_DIR, dir_fd=dfd)
    except PermissionError:
        try:
            os.chmod(name, stat.S_IRWXU, dir_fd=dfd, follow_symlinks=False)
        except NotImplementedError:  # Linux has no lchmod
            raise PermissionError("cannot open folder %s" % name)
        fd = os.open(name, O_DIR, dir_fd=dfd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISDIR(st.st_mode):
            raise Refusal("%s is not a folder" % name)
        if ident is not None and (st.st_dev, st.st_ino) != ident:
            raise Refusal("%s changed while it was checked" % name)
        if st.st_dev != dev:
            raise Refusal("%s is on another file system" % name)
        try:
            entries = [(e.name, e.is_dir(follow_symlinks=False)) for e in os.scandir(fd)]
        except PermissionError:
            os.fchmod(fd, stat.S_IRWXU)
            entries = [(e.name, e.is_dir(follow_symlinks=False)) for e in os.scandir(fd)]
        for n, is_dir in entries:
            if is_dir:
                remove_at(fd, n, dev, inside=True)
                continue
            try:
                os.unlink(n, dir_fd=fd)
            except FileNotFoundError:
                pass
            except PermissionError:
                os.fchmod(fd, stat.S_IRWXU)
                os.unlink(n, dir_fd=fd)
    finally:
        os.close(fd)
    try:
        os.rmdir(name, dir_fd=dfd)
    except PermissionError:
        if not inside:
            raise
        os.fchmod(dfd, stat.S_IRWXU)  # a read-only folder of the tree being deleted
        os.rmdir(name, dir_fd=dfd)


def open_dir_at(dfd, name, what):
    try:
        return os.open(name, O_DIR, dir_fd=dfd)
    except FileNotFoundError:
        raise Refusal("%s does not exist; nothing deleted" % what)
    except OSError:
        raise Refusal("%s is not a plain folder; nothing deleted" % what)


class Deleter:
    """Deletes only run folders (or a cleanroom run's runtime parts) below one
    checkout's build/mg. build and mg are opened one component at a time from
    the checkout's descriptor without following links, and every later check
    and deletion goes through descriptors opened from that build/mg, so
    swapping build or a folder for a link after the check changes nothing."""

    def __init__(self, ck, ctx):
        self.ck = ck
        self.ctx = ctx
        root = os.path.realpath(ck.path)
        if root != ck.path:
            raise Refusal("%s is not a resolved checkout path; nothing deleted" % ck.path)
        root_fd = os.open(root, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
        try:
            build_fd = open_dir_at(root_fd, "build", os.path.join(root, "build"))
            try:
                self.mg_fd = open_dir_at(build_fd, "mg", os.path.join(root, "build", "mg"))
            finally:
                os.close(build_fd)
        finally:
            os.close(root_fd)
        self.mg = os.path.join(root, "build", "mg")
        self.dev = os.fstat(self.mg_fd).st_dev
        self.lock_fd = None
        self.index = os.path.join(ck.path, os.fsdecode(git(["rev-parse", "--git-path", "index"], ck.path).strip()))
        self.index_seen = None
        self.tracked = set()
        self.tracked_now()

    def lock(self):
        """One --apply per checkout: an flock on build/mg/prune/apply.lock, held until exit."""
        try:
            os.mkdir(PRUNE, dir_fd=self.mg_fd)
        except FileExistsError:
            pass
        pfd = os.open(PRUNE, O_DIR, dir_fd=self.mg_fd)
        try:
            self.lock_fd = os.open("apply.lock", os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0), 0o644, dir_fd=pfd)
        finally:
            os.close(pfd)
        try:
            fcntl.flock(self.lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            return False
        return True

    def tracked_now(self):
        """(job, folder name) of run folders, pending ones included, holding a
        tracked file; read again whenever the index changed."""
        try:
            st = os.stat(self.index)
            seen = (st.st_ino, st.st_size, st.st_mtime_ns)
        except FileNotFoundError:
            seen = None
        if seen != self.index_seen or seen is None:
            self.tracked = self.ck.tracked_runs()
            self.index_seen = seen
        return self.tracked

    def same_tree(self):
        """Refuse when build/mg by path is no longer the folder opened at the
        start: the plan and the archive checks read paths, deletion uses the
        descriptor, and both must mean one tree."""
        try:
            st = os.stat(self.mg)
        except OSError:
            st = None
        held = os.fstat(self.mg_fd)
        if st is None or (st.st_dev, st.st_ino) != (held.st_dev, held.st_ino):
            raise Refusal("%s is no longer the folder prune opened (build or mg was replaced); stopped" % self.mg)

    def _check(self, jfd, run, part, expect=None):
        """(why not, folder identity) for run's folder in the open job folder jfd.
        expect: the folder's entries as planned; any other entry keeps it."""
        self.same_tree()
        try:
            rfd = os.open(run.id, O_DIR, dir_fd=jfd)
        except FileNotFoundError:
            return "gone", None
        except OSError:
            return "not a plain folder", None
        try:
            st = os.fstat(rfd)
            ident = (st.st_dev, st.st_ino)
            if st.st_dev != self.dev:
                return "on another file system", ident
            entries = {e.name: e for e in os.scandir(rfd)}
            if expect is not None and not set(entries) <= set(expect):
                return "its folder changed since the plan", ident
            res = entries.get("result.json")
            if res is None or not res.is_file(follow_symlinks=False) or ".mg" in entries:
                return "unfinished", ident
            if self.ctx.live(run):
                return "its tools/mg process runs", ident
            if run.key in self.tracked_now():
                return "holds a tracked file", ident
            if any(c.path == run.path or c.path.startswith(run.path + os.sep) for c in self.ctx.checkouts):
                return "holds a checkout", ident
            # A detached writer (a VM's console log, a container's output) can
            # outlive its job: a file written in that time keeps the folder.
            quiet = CLEANROOM_QUIET if part is not None else KEEP_DAYS * 86400
            newest = max([e.stat(follow_symlinks=False).st_mtime for e in entries.values()
                          if e.is_file(follow_symlinks=False)] or [0])
            if newest > time.time() - quiet:
                return "written to in the last %d s" % quiet, ident
            if part is not None:
                # Only a cleanroom run's source/ or codex-home/; its folder stays.
                if part not in CLEANROOM_RUNTIME or run.job != CLEANROOM:
                    return "%s is not runtime state" % part, ident
                if part not in entries:
                    return "gone", ident
                if not entries[part].is_dir(follow_symlinks=False):
                    return "%s is not a plain folder" % part, ident
                return None, ident
        finally:
            os.close(rfd)
        try:
            if os.path.basename(os.readlink("latest", dir_fd=jfd)) == run.id:
                return "latest", ident
        except OSError:
            pass
        return None, ident

    def _job(self, run):
        try:
            return os.open(run.job, O_DIR, dir_fd=self.mg_fd)
        except OSError:
            return None

    def check(self, run, part=None, expect=None):
        """Why this path must not be deleted now, or None. Checked again just
        before each deletion: a job may have started or finished since the plan."""
        jfd = self._job(run)
        if jfd is None:
            return "its job folder is not a plain folder"
        try:
            return self._check(jfd, run, part, expect)[0]
        finally:
            os.close(jfd)

    def remove(self, run, part=None, expect=None):
        jfd = self._job(run)
        if jfd is None:
            return "its job folder is not a plain folder"
        try:
            why, ident = self._check(jfd, run, part, expect)
            if why is not None:
                return why
            if part is None:
                # Renamed first, so a deletion cut short is finished by the next
                # --apply rather than left as an "unfinished" run.
                os.rename(run.id, PENDING + run.id, src_dir_fd=jfd, dst_dir_fd=jfd)
                st = os.stat(PENDING + run.id, dir_fd=jfd, follow_symlinks=False)
                if (st.st_dev, st.st_ino) != ident:
                    # Never leave an unchecked folder under the pending name,
                    # which the next --apply would finish without checks.
                    try:
                        os.rename(PENDING + run.id, run.id, src_dir_fd=jfd, dst_dir_fd=jfd)
                    except OSError:
                        os.rename(PENDING + run.id, "%s%s-%d-%d" % (REFUSED, run.id, os.getpid(), time.time_ns()),
                                  src_dir_fd=jfd, dst_dir_fd=jfd)
                    return "changed while it was checked"
                remove_at(jfd, PENDING + run.id, self.dev, ident=ident)
            else:
                rfd = os.open(run.id, O_DIR, dir_fd=jfd)
                try:
                    st = os.fstat(rfd)
                    if (st.st_dev, st.st_ino) != ident:
                        return "changed while it was checked"
                    remove_at(rfd, part, self.dev)
                finally:
                    os.close(rfd)
        finally:
            os.close(jfd)
        return None

    def finish_pending(self, failures):
        """Delete what an earlier --apply renamed for deletion and could not
        finish, unless the index now tracks a file in it."""
        done = 0
        self.same_tree()
        named = self.ctx.external(self.ck)
        for r in self.ck.runs:
            named |= r.receipts
        for job, name in self.ck.pending:
            if (job, name) in self.tracked_now():
                failures.append({"id": "delete/%s/%s" % (job, name),
                                 "message": "holds a tracked file; left for you to resolve"})
                continue
            if name[len(PENDING):] in named:
                failures.append({"id": "delete/%s/%s" % (job, name),
                                 "message": "a reference names this run again; left for you to resolve"})
                continue
            try:
                jfd = os.open(job, O_DIR, dir_fd=self.mg_fd)
                try:
                    remove_at(jfd, name, self.dev)
                finally:
                    os.close(jfd)
                done += 1
            except FileNotFoundError:
                pass
            except (OSError, Refusal) as e:
                failures.append({"id": "delete/%s/%s" % (job, name), "message": "could not finish deleting it: %s" % e})
        return done


def apply_runs(ctx, ck, deleter, cands, failures):
    done = freed = 0
    skipped = []
    for r in sorted((r for r in cands if r.action == "delete"), key=lambda r: (r.started, r.pid)):
        try:
            why = deleter.remove(r)
        except (OSError, Refusal) as e:
            failures.append({"id": "delete/%s/%s" % (r.job, r.id), "message": "could not delete %s: %s" % (r.path, e)})
            continue
        if why is None:
            done += 1
            freed += r.bytes
        elif why != "gone":
            skipped.append({"run": "%s/%s" % (r.job, r.id), "why": why})
    return {"deleted": done, "bytes_freed": freed, "skipped": skipped}


# ---------------------------------------------------------------------------
# Cleanroom archives

class ArchiveError(Exception):
    pass


def sha_file(path):
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(fd, "rb") as f:
        st = os.fstat(f.fileno())
        if not stat.S_ISREG(st.st_mode):
            raise ArchiveError("%s is not a plain file" % path)
        h = hashlib.sha256()
        size = 0
        while True:
            b = f.read(1 << 20)
            if not b:
                break
            h.update(b)
            size += len(b)
    return size, h.hexdigest()


def grew(r, name, archived):
    """True when the archived (size, sha256) is the start of the file on disk:
    a log that grew after an earlier --apply archived it."""
    size, digest = archived
    h = hashlib.sha256()
    left = size
    fd = os.open(os.path.join(r.path, name), os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(fd, "rb") as f:
        while left > 0:
            chunk = f.read(min(left, 1 << 20))
            if not chunk:
                return False
            h.update(chunk)
            left -= len(chunk)
    return h.hexdigest() == digest


def disk_records(r):
    return {n: sha_file(os.path.join(r.path, n)) for n in records_of(r)}


def open_in(dfd, name, flags=os.O_RDONLY, mode=0o644):
    return os.open(name, flags | getattr(os, "O_NOFOLLOW", 0), mode, dir_fd=dfd)


def read_archive(dfd, name):
    """{run id: {record: (size, sha256)}} of the month archive name in the open
    folder dfd; ArchiveError if it is not one."""
    runs = {}
    try:
        with os.fdopen(open_in(dfd, name), "rb") as raw:
            if not stat.S_ISREG(os.fstat(raw.fileno()).st_mode):
                raise ArchiveError("%s is not a plain file" % name)
            with tarfile.open(fileobj=raw, mode="r:xz") as tf:
                for m in tf:
                    mm = MEMBER.match(m.name)
                    if not m.isreg() or not mm or mm.group(2) not in CLEANROOM_RECORDS:
                        raise ArchiveError("%s holds an unexpected member %s" % (name, m.name))
                    f = tf.extractfile(m)
                    h = hashlib.sha256()
                    size = 0
                    while True:
                        chunk = f.read(1 << 20)
                        if not chunk:
                            break
                        h.update(chunk)
                        size += len(chunk)
                    if size != m.size:
                        raise ArchiveError("%s: %s is truncated" % (name, m.name))
                    recs = runs.setdefault(mm.group(1), {})
                    if mm.group(2) in recs:
                        raise ArchiveError("%s holds %s twice" % (name, m.name))
                    recs[mm.group(2)] = (size, h.hexdigest())
    except (tarfile.TarError, EOFError, OSError) as e:
        raise ArchiveError("cannot read %s: %s" % (name, e))
    return runs


def archive_dir(ck):
    """Path of the archive folder, refused if it is anything but a plain folder."""
    d = os.path.join(ck.mg, ARCHIVE_DIR)
    st = lstat(d)
    if st is not None and (stat.S_ISLNK(st.st_mode) or not stat.S_ISDIR(st.st_mode)):
        raise Refusal("%s is not a plain folder" % d)
    return d


def existing_archives(ck):
    d = archive_dir(ck)
    out = {}
    if not os.path.isdir(d):
        return out
    for e in os.scandir(d):
        m = re.match(r"^(\d{4}-\d{2})\.tar\.xz$", e.name)
        if m and e.is_file(follow_symlinks=False):
            out[m.group(1)] = e.path
    return out


def write_archive(dfd, name, old, add, runs_by_id):
    """Write old's members plus add's records to name.partial (created anew, never
    through a link), read it back and compare, then move it over name. A record
    in add that old holds replaces it (a run whose folder still holds it)."""
    tmp = name + ".partial"
    try:
        os.unlink(tmp, dir_fd=dfd)
    except FileNotFoundError:
        pass
    with os.fdopen(open_in(dfd, tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL), "wb") as raw:
        with tarfile.open(fileobj=raw, mode="w:xz", format=tarfile.PAX_FORMAT) as out:
            if old:
                with os.fdopen(open_in(dfd, name), "rb") as src_raw:
                    with tarfile.open(fileobj=src_raw, mode="r:xz") as src:
                        for m in src:
                            rid, _, rec = m.name.partition("/")
                            if rec not in add.get(rid, {}):
                                out.addfile(m, src.extractfile(m))
            for rid in sorted(add):
                r = runs_by_id[rid]
                for rec in sorted(add[rid]):
                    src_path = os.path.join(r.path, rec)
                    fd = os.open(src_path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
                    with os.fdopen(fd, "rb") as f:
                        st = os.fstat(f.fileno())
                        if not stat.S_ISREG(st.st_mode) or st.st_size != add[rid][rec][0]:
                            raise ArchiveError("%s changed while it was archived" % src_path)
                        info = tarfile.TarInfo("%s/%s" % (rid, rec))
                        info.size = st.st_size
                        info.mtime = int(st.st_mtime)
                        info.mode = 0o644
                        info.uid = info.gid = 0
                        info.uname = info.gname = ""
                        out.addfile(info, f)
        raw.flush()
        os.fsync(raw.fileno())
    want = {k: dict(v) for k, v in old.items()}
    for rid, recs in add.items():
        want.setdefault(rid, {}).update(recs)
    if read_archive(dfd, tmp) != want:
        os.unlink(tmp, dir_fd=dfd)
        raise ArchiveError("%s does not hold what was written; the archive was left as it was" % tmp)
    os.replace(tmp, name, src_dir_fd=dfd, dst_dir_fd=dfd)
    os.fsync(dfd)


def apply_cleanroom(ctx, ck, deleter, cands, failures):
    adir = archive_dir(ck)
    try:
        os.mkdir(ARCHIVE_DIR, dir_fd=deleter.mg_fd)
    except FileExistsError:
        pass
    adfd = os.open(ARCHIVE_DIR, O_DIR, dir_fd=deleter.mg_fd)
    try:
        return archive_and_delete(ctx, ck, deleter, adfd, adir, cands, failures)
    finally:
        os.close(adfd)


def eligible(deleter, r):
    """None when r may lose now what its action removes, else why not. Asked
    before its records are archived, and again (by remove) before each deletion."""
    if r.action == "remove":
        return deleter.check(r, expect=set(r.entries))
    whys = [deleter.check(r, part, expect=set(r.entries)) for part in CLEANROOM_RUNTIME if part in r.entries]
    if not whys:
        whys = [deleter.check(r, expect=set(r.entries))]
    whys = [w for w in whys if w not in (None, "gone", "latest")]
    return whys[0] if whys else None


def archive_and_delete(ctx, ck, deleter, adfd, adir, cands, failures):
    work = [r for r in cands if r.action in ("strip", "remove")]
    months = {}
    for r in work:
        months.setdefault("%s-%s" % (r.id[0:4], r.id[4:6]), []).append(r)
    report = {"archived_runs": 0, "removed_runs": 0, "stripped_runs": 0, "bytes_freed": 0,
              "archives": {}, "skipped": [], "records_replaced": []}
    for month in sorted(months):
        name = month + ".tar.xz"
        path = os.path.join(adir, name)
        runs = []
        try:
            deleter.same_tree()
        except Refusal as e:
            failures.append({"id": "refused", "message": str(e)})
            return report
        for r in months[month]:
            try:
                why = eligible(deleter, r)
            except Refusal as e:
                failures.append({"id": "refused", "message": str(e)})
                return report
            if why is None:
                runs.append(r)
            elif why != "gone":
                report["skipped"].append({"run": r.id, "why": why})
        if not runs:
            continue
        try:
            try:
                pst = os.stat(name, dir_fd=adfd, follow_symlinks=False)
            except FileNotFoundError:
                pst = None
            if pst is not None and not stat.S_ISREG(pst.st_mode):
                raise ArchiveError("%s is not a plain file" % path)
            old = read_archive(adfd, name) if pst is not None else {}
            add, bad = {}, set()
            for r in runs:
                try:
                    recs = disk_records(r)
                except (OSError, ArchiveError) as e:
                    failures.append({"id": "archive/%s" % r.id, "message": "cannot read its records: %s" % e})
                    bad.add(r.id)
                    continue
                have = old.get(r.id, {})
                new = {n: v for n, v in recs.items() if have.get(n) != v}
                shrunk = [n for n in new if n in have and not grew(r, n, have[n])]
                if shrunk:
                    failures.append({"id": "archive/%s" % r.id, "message": "%s holds other content for %s than the run's folder, not an earlier part of it; the run was left untouched" % (path, ", ".join(shrunk))})
                    bad.add(r.id)
                    continue
                if new:
                    add[r.id] = new
                # The folder still holds its records, so an older archived copy
                # (a run that grew after an earlier --apply archived it) is replaced.
                report["records_replaced"] += ["%s/%s" % (r.id, n) for n in new if n in have]
            if add:
                write_archive(adfd, name, old, add, {r.id: r for r in runs})
            # Read back from the file itself before any run of this month goes.
            final = read_archive(adfd, name) if (add or old) else {}
        except (OSError, ArchiveError, tarfile.TarError) as e:
            failures.append({"id": "archive/%s" % month, "message": "%s; no run of %s was deleted" % (e, month)})
            continue
        report["archives"][month] = {"path": path, "runs": len(final),
                                     "members": sum(len(v) for v in final.values()),
                                     "bytes": os.path.getsize(path) if os.path.exists(path) else 0}
        for r in sorted(runs, key=lambda r: (r.started, r.pid)):
            if r.id in bad:
                continue
            try:
                now = disk_records(r)
            except (OSError, ArchiveError) as e:
                failures.append({"id": "archive/%s" % r.id, "message": "cannot read its records: %s" % e})
                continue
            have = final.get(r.id, {})
            if any(have.get(n) != v for n, v in now.items()):
                failures.append({"id": "archive/%s" % r.id, "message": "its records are not verified in %s; the run was left untouched" % path})
                continue
            report["archived_runs"] += 1
            parts = [None] if r.action == "remove" else [p for p in CLEANROOM_RUNTIME if p in r.entries]
            stripped = False
            for part in parts:
                try:
                    # A file or folder that appeared since the plan was never archived.
                    why = deleter.remove(r, part, expect=set(r.entries))
                except (OSError, Refusal) as e:
                    failures.append({"id": "delete/cleanroom/%s" % r.id,
                                     "message": "could not delete %s: %s" % (part or "the folder", e)})
                    break
                if why is None:
                    # Counted as each part goes, so a later failure keeps what was freed.
                    report["bytes_freed"] += r.bytes if part is None else r.parts.get(part, 0)
                    if part is None:
                        report["removed_runs"] += 1
                    else:
                        stripped = True
                elif why != "gone":
                    report["skipped"].append({"run": r.id, "why": "%s: %s" % (part or "folder", why)})
            if stripped:
                report["stripped_runs"] += 1
    return report


# ---------------------------------------------------------------------------
# Output

def print_runs(ck, rows, total):
    log("")
    log("%s: run folders (cleanroom excluded: --cleanroom)" % ck.mg)
    log("  %-34s %6s %7s %6s %11s  %s" % ("job", "runs", "delete", "keep", "frees", "kept because"))
    for j in rows:
        why = ", ".join("%s %d" % (k, v) for k, v in sorted(j["kept_because"].items(), key=lambda kv: -kv[1]))
        log("  %-34s %6d %7d %6d %11s  %s" % (j["job"], j["runs"], j["delete"], j["keep"], human(j["bytes_freed"]), why))
    log("  %-34s %6d %7d %6d %11s  (%s kept)" % ("total", total["runs"], total["delete"], total["keep"],
                                                  human(total["bytes_freed"]), human(total["bytes_kept"])))


def print_cleanroom(ck, s):
    log("")
    log("%s: cleanroom runs" % os.path.join(ck.mg, CLEANROOM))
    log("  %d runs: %d archived then removed, %d archived and kept without source/codex-home, %d untouched"
        % (s["runs"], s["remove"], s["strip"], s["untouched"]))
    log("  frees %s: source copies %s, codex-home %s, rest of removed folders; records to archive %s (uncompressed)"
        % (human(s["bytes_freed"]), human(s["bytes_source"]), human(s["bytes_codex_home"]), human(s["bytes_records"])))
    for month, m in sorted(s["months"].items()):
        log("  archive %s.tar.xz: %d runs, %s of records" % (month, m["runs"], human(m["bytes_records"])))
    if s["kept_because"]:
        log("  folders kept because: " + ", ".join("%s %d" % kv for kv in sorted(s["kept_because"].items(), key=lambda kv: -kv[1])))
    if s["source_kept"]:
        log("  %d runs kept whole (%s), their source copy or Codex state possibly the only copy of their work: %s"
            % (s["source_kept"], human(s["bytes_source_kept"]),
               ", ".join("%s %d" % kv for kv in sorted(s["source_kept_because"].items(), key=lambda kv: -kv[1]))))
    for u in s["untouched_runs"]:
        log("  untouched %s (%s): %s" % (u["run"], human(u["bytes"]), "; ".join(u["why"])))


def print_unmanaged(items, volumes):
    log("")
    log("not managed by prune (details.unmanaged lists every entry):")
    small = [i for i in items if i["bytes"] is not None and i["bytes"] < SMALL]
    for i in items:
        if i not in small:
            log("  %-60s %11s  %s" % (i["path"], "?" if i["bytes"] is None else human(i["bytes"]), i["note"]))
    if small:
        log("  %-60s %11s  not managed by prune" % ("%d smaller entries of build/" % len(small),
                                                     human(sum(i["bytes"] for i in small))))
    if not volumes.get("measured"):
        log("  Docker volumes: not measured (%s)" % volumes.get("why"))
        return
    vols = volumes.get("list", [])
    cargo = [v for v in vols if v["name"].endswith("_cargo-target")]
    log("  Docker: %d Meguri volumes, %s; %d Rust target volumes of checkouts, %s"
        % (len(vols), human(sum(v["bytes"] or 0 for v in vols)),
           len([v for v in cargo if v["checkout"]]), human(sum(v["bytes"] or 0 for v in cargo if v["checkout"]))))
    orphans = [v for v in vols if v["checkout"] is None]
    if orphans:
        log("  Docker volumes of no current checkout, %s (removed worktrees; tools/AGENTS.md: `docker volume rm <name>` once no container uses it):"
            % human(sum(v["bytes"] or 0 for v in orphans)))
        for v in orphans:
            log("    %-76s %10s%s" % (v["name"], v["size"], "  in use" if v["in_use"] else ""))


# ---------------------------------------------------------------------------

def main():
    # prune.sh passes: --self <checkout> --run-dir <dir> --checkouts <tsv> --out <json> -- <job args>
    ap = argparse.ArgumentParser(prog="prune-run-output.py", add_help=False)
    for name in ("--self", "--run-dir", "--checkouts", "--out"):
        ap.add_argument(name, required=True)
    fixed, rest = ap.parse_known_args()
    out_path = fixed.out
    if rest[:1] == ["--"]:
        rest = rest[1:]
    args = argparse.Namespace(self_root=fixed.self, run_dir=fixed.run_dir, apply=False, cleanroom=False,
                              all_worktrees=False, root=None, no_docker=False)
    i = 0
    while i < len(rest):
        a = rest[i]
        flag = {"--apply": "apply", "--cleanroom": "cleanroom", "--all-worktrees": "all_worktrees",
                "--no-docker": "no_docker"}.get(a)
        if flag:
            setattr(args, flag, True)
        elif a == "--root" and i + 1 < len(rest):
            args.root = rest[i + 1]
            i += 1
        else:
            return finish(out_path, "error", "prune: " + USAGE, failures=[{"id": "usage", "message": "unknown argument %r; %s" % (a, USAGE)}])
        i += 1
    self_root = os.path.realpath(args.self_root)
    targets = {}  # checkout -> its compose project and Docker Rust target, from prune.sh
    with open(fixed.checkouts, "rb") as f:
        fields = [os.fsdecode(x) for x in f.read().split(b"\0")[:-1]]
    for i in range(0, len(fields) - 2, 3):
        targets[os.path.realpath(fields[i])] = {"project": fields[i + 1], "target": fields[i + 2]}

    def fail(msg, fid="usage"):
        return finish(out_path, "error", "prune: " + msg, failures=[{"id": fid, "message": msg}])

    if args.apply and args.all_worktrees:
        return fail("--all-worktrees only reports; --apply works on one checkout, the one the job runs in")
    if args.apply and args.root:
        return fail("--root only reports; run tools/mg prune --apply from the checkout you prune")
    root = os.path.realpath(args.root) if args.root else self_root
    wts, missing = worktree_list(self_root)
    args.missing = missing
    known = [w["path"] for w in wts]
    if root not in known:
        return fail("%s is not a checkout of this repository (git worktree list)" % root)
    if args.apply and os.environ.get("CI"):
        return fail("--apply does not run in CI", "ci")

    mode = "all-worktrees" if args.all_worktrees else ("cleanroom" if args.cleanroom else "runs")
    log("prune: %s, %s" % (mode, "applying" if args.apply else "report only (--apply deletes)"))
    log("rule: keep the newest %d runs of each job, every run started, finished or written to in the last %d days, "
        "and every latest, referenced, unfinished, live or tracked run (tools/AGENTS.md)" % (KEEP_NEWEST, KEEP_DAYS))

    try:
        return run(args, wts, root, targets, out_path, mode)
    except Refusal as e:
        return fail(str(e), "refused")


def collect(args, wts):
    """Every checkout's runs and every reference source, read once."""
    t0 = time.time()
    try:
        ctx = Context(time.time(), processes())
    except OSError as e:
        raise Refusal("cannot tell which runs are live: %s" % e)
    refs = ctx.refs
    ctx.checkouts = [Checkout(w["path"]) for w in wts]
    for path in getattr(args, "missing", []):
        # Its files and receipts cannot be read, so they cannot protect a run.
        refs.errors.append("checkout %s is listed by git worktree list but missing; mount it, or run `git worktree prune` if it is gone" % path)
    uncommitted, staged = set(), set()

    def changes(path):
        try:
            worktree_changes(path, refs, uncommitted, staged)
        except OSError as e:
            refs.errors.append(str(e))

    # Listing and git calls wait on the disk and on git, not on Python.
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        jobs = [pool.submit(changes, w["path"]) for w in wts]
        for ck in ctx.checkouts:
            ck.scan(refs)
            jobs += [pool.submit(ck.scan_job, job, path, refs) for job, path in ck.job_dirs]
            jobs += [pool.submit(refs.tree_or_file, path, BUILD_DEPTH, ck.state) for path in ck.build_entries()]
        for j in jobs:
            j.result()
    for ck in ctx.checkouts:
        ck.runs.sort(key=lambda r: (r.job, r.started, r.pid))
    try:
        ncommits, nblobs = git_refs(args.self_root, [w.get("head", "") for w in wts], staged, refs, args.run_dir)
    except OSError as e:
        refs.errors.append(str(e))
        ncommits = nblobs = 0
    in_git = len(refs.ids)
    refs.ids |= uncommitted
    commits = set()
    for ck in ctx.checkouts:
        for r in ck.runs:
            c = ((r.result or {}).get("details") or {}).get("source_commit")
            if isinstance(c, str) and re.match(r"^[0-9a-f]{40}$", c):
                commits.add(c)
    if commits:
        # A source copy goes only when a branch, tag or worktree HEAD still
        # reaches its base, so gc never takes the commit its patch needs. A
        # stash is not such a root: dropping it would orphan the base.
        try:
            roots = set(w["head"] for w in wts if w.get("head"))
            for line in git(["for-each-ref", "--format=%(objectname) %(objecttype) %(*objectname)",
                             "refs/heads", "refs/remotes", "refs/tags"], args.self_root).decode().splitlines():
                parts = line.split()
                if len(parts) >= 2 and parts[1] == "commit":
                    roots.add(parts[0])
                elif len(parts) == 3 and parts[1] == "tag":
                    roots.add(parts[2])
            reach = git(["rev-list", "--stdin"], args.self_root, input="".join(r + "\n" for r in sorted(roots)).encode())
            ctx.reachable = commits & set(reach.decode().split())
        except OSError as e:
            refs.errors.append(str(e))
    ctx.ref_stats = {"branch_commits": ncommits, "objects": nblobs, "run_ids": len(refs.ids),
                     "run_ids_in_branches": in_git, "run_ids_uncommitted": len(uncommitted),
                     "run_ids_in_state_files": len(set().union(*[ck.state for ck in ctx.checkouts])),
                     "errors": refs.errors[:50]}
    log("references: %d run ids from %d branch tips and worktree HEADs, %d checkouts' uncommitted files, build/mg state and receipts (%.0f s)"
        % (len(refs.ids), ncommits, len(wts), time.time() - t0))
    for e in refs.errors[:20]:
        log("  could not read: %s" % e)
    return ctx


RANK = {"delete": 0, "keep": 1, "remove": 0, "strip": 1, "untouched": 2}


def second_scan(args, wts, root, mode, deleter, cands):
    """Read every reference source again after planning and measuring, and keep
    whatever the new plan keeps: a job may have written a checkpoint or a
    receipt naming an old run since the first scan."""
    log("second scan of references before deleting:")
    deleter.same_tree()
    ctx = collect(args, wts)
    deleter.same_tree()
    if ctx.refs.errors:
        raise Refusal("some reference sources could not be read at the second scan; nothing deleted. First: %s" % ctx.refs.errors[0])
    ck = next(c for c in ctx.checkouts if c.path == root)
    plan = (plan_runs if mode == "runs" else plan_cleanroom)(ctx, ck, deleter.tracked_now())
    again = {r.key: r for r in plan}
    held = 0
    for r in cands:
        r2 = again.get(r.key)
        if r2 is None:
            r.action, held = ("keep" if mode == "runs" else "untouched"), held + 1
            r.reasons.append("gone at the second scan")
        elif RANK[r2.action] > RANK[r.action]:
            r.action, r.reasons, held = r2.action, r2.reasons + ["second scan"], held + 1
        if mode != "runs" and r2 is not None:
            r.source_ok = r.source_ok and r2.source_ok
    deleter.ctx, deleter.ck = ctx, ck
    log("  %d runs kept that the first scan would have let go" % held)
    return held


def run(args, wts, root, targets, out_path, mode):
    deleter = None
    if args.apply:
        # Checked and locked before anything is read: the plan is made under the lock.
        deleter = Deleter(Checkout(root), None)
        if not deleter.lock():
            raise Refusal("another tools/mg prune --apply runs in this checkout; nothing deleted")
    if deleter:
        deleter.same_tree()
    ctx = collect(args, wts)
    refs = ctx.refs
    if deleter:
        deleter.same_tree()  # the scan read paths: they must still name the opened tree
        deleter.ctx = ctx
        deleter.ck = next(c for c in ctx.checkouts if c.path == root)
    if args.apply and refs.errors:
        raise Refusal("some reference sources could not be read, so a referenced run could be missed; nothing deleted. First: %s" % refs.errors[0])

    volumes = docker_volumes(targets, not args.no_docker and mode != "cleanroom")
    by_path = {ck.path: ck for ck in ctx.checkouts}
    failures = []
    details = {"mode": mode, "apply": bool(args.apply), "root": root,
               "rule": {"keep_newest": KEEP_NEWEST, "keep_days": KEEP_DAYS,
                        "always": ["latest", "referenced", "unfinished", "live", "tracked"]},
               "references": ctx.ref_stats}
    counts = {}
    plan_rows = []

    if mode == "all-worktrees":
        rows = []
        tot = {"runs_bytes_freed": 0, "cleanroom_bytes_freed": 0, "runs_delete": 0}
        for ck in ctx.checkouts:
            tracked = ck.tracked_runs()
            cands = plan_runs(ctx, ck, tracked)
            measure([r for r in cands if r.action == "delete"], False)
            _, total = summarize_runs(cands)
            cr = plan_cleanroom(ctx, ck, tracked)
            measure([r for r in cr if r.action != "untouched"], True)
            cs = summarize_cleanroom(cr)
            plan_rows += cands + cr
            target = targets.get(ck.path, {}).get("target") or ""
            vol = next((v for v in volumes.get("list", []) if "volume:" + v["name"] == target), None)
            row = {"checkout": ck.path, "runs": total["runs"], "delete": total["delete"],
                   "bytes_freed": total["bytes_freed"], "cleanroom_runs": cs["runs"],
                   "cleanroom_bytes_freed": cs["bytes_freed"], "cargo_target": target or None,
                   "cargo_volume_bytes": vol["bytes"] if vol else None}
            rows.append(row)
            tot["runs_bytes_freed"] += total["bytes_freed"]
            tot["cleanroom_bytes_freed"] += cs["bytes_freed"]
            tot["runs_delete"] += total["delete"]
        rows.sort(key=lambda r: -(r["bytes_freed"] + r["cleanroom_bytes_freed"]))
        log("")
        log("  %-72s %7s %11s %11s %12s" % ("checkout", "delete", "frees", "cleanroom", "Rust volume"))
        for r in rows:
            log("  %-72s %7d %11s %11s %12s" % (r["checkout"], r["delete"], human(r["bytes_freed"]), human(r["cleanroom_bytes_freed"]),
                                               "-" if r["cargo_volume_bytes"] is None else human(r["cargo_volume_bytes"])))
        log("  %-72s %7d %11s %11s" % ("total", tot["runs_delete"], human(tot["runs_bytes_freed"]), human(tot["cleanroom_bytes_freed"])))
        orphans = [v for v in volumes.get("list", []) if v["checkout"] is None]
        print_unmanaged([], volumes)
        details.update({"checkouts": rows, "totals": tot, "volumes": volumes})
        counts = {"checkouts": len(rows), "delete": tot["runs_delete"], "bytes_freed": tot["runs_bytes_freed"],
                  "cleanroom_bytes_freed": tot["cleanroom_bytes_freed"], "orphan_volumes": len(orphans)}
        write_plan(args, plan_rows)
        message = "prune: %d checkouts; would free %s of run folders and %s of cleanroom runtime state" % (
            len(rows), human(tot["runs_bytes_freed"]), human(tot["cleanroom_bytes_freed"]))
        return finish(out_path, "warn" if refs.errors else "pass", message, counts, [], details)

    ck = by_path[root]
    tracked = deleter.tracked if deleter else ck.tracked_runs()
    details["skipped"] = ck.skipped
    details["pending"] = ["%s/%s" % p for p in ck.pending]
    if deleter and ck.pending:
        details["pending_finished"] = deleter.finish_pending(failures)
    if mode == "runs":
        cands = plan_runs(ctx, ck, tracked)
        measure(cands, False)
        rows, total = summarize_runs(cands)
        print_runs(ck, rows, total)
        details.update({"jobs": rows, "totals": total})
        counts = {"runs": total["runs"], "delete": total["delete"], "keep": total["keep"],
                  "bytes_freed": total["bytes_freed"]}
        message = "prune: %d of %d runs would go, freeing %s" % (total["delete"], total["runs"], human(total["bytes_freed"]))
        if args.apply:
            details["second_scan_kept"] = second_scan(args, wts, root, mode, deleter, cands)
            done = apply_runs(ctx, ck, deleter, cands, failures)
            details["applied"] = done
            counts.update({"deleted": done["deleted"], "bytes_freed": done["bytes_freed"]})
            message = "prune: deleted %d runs, freed %s" % (done["deleted"], human(done["bytes_freed"]))
            log("")
            log(message + ("; %d skipped (rechecked just before deleting)" % len(done["skipped"]) if done["skipped"] else ""))
    else:
        cands = plan_cleanroom(ctx, ck, tracked)
        measure(cands, True)
        s = summarize_cleanroom(cands)
        try:
            s["archives_now"] = {m: {"path": p, "bytes": os.path.getsize(p)} for m, p in existing_archives(ck).items()}
        except Refusal as e:
            s["archives_now"] = {"error": str(e)}
        print_cleanroom(ck, s)
        details["cleanroom"] = s
        counts = {"runs": s["runs"], "remove": s["remove"], "strip": s["strip"], "untouched": s["untouched"],
                  "bytes_freed": s["bytes_freed"]}
        message = "prune --cleanroom: %d runs; would archive %d and free %s" % (
            s["runs"], s["remove"] + s["strip"], human(s["bytes_freed"]))
        if args.apply:
            details["second_scan_kept"] = second_scan(args, wts, root, mode, deleter, cands)
            done = apply_cleanroom(ctx, ck, deleter, cands, failures)
            details["applied"] = done
            counts.update({"archived": done["archived_runs"], "removed": done["removed_runs"],
                           "bytes_freed": done["bytes_freed"]})
            message = "prune --cleanroom: archived %d runs, removed %d folders, freed %s" % (
                done["archived_runs"], done["removed_runs"], human(done["bytes_freed"]))
            log("")
            log(message)
            for m, a in sorted(done["archives"].items()):
                log("  %s: %d runs, %d files, %s" % (a["path"], a["runs"], a["members"], human(a["bytes"])))
    if mode == "runs":
        items = unmanaged(ck, targets, volumes)
        print_unmanaged(items, volumes)
        details["unmanaged"] = items
        details["volumes"] = volumes
    write_plan(args, cands)
    for f in failures:
        log("  failed: %s: %s" % (f["id"], f["message"]))
    status = "fail" if failures else ("warn" if refs.errors else "pass")
    if refs.errors and not failures:
        message += "; some reference sources could not be read (details.references.errors), so --apply refuses"
    return finish(out_path, status, message, counts, failures, details)


def write_plan(args, runs):
    """plan.json: every run considered, with its checkout, action and keep reasons."""
    with open(os.path.join(args.run_dir, "plan.json"), "w") as f:
        json.dump([{"checkout": r.checkout.path, "job": r.job, "run": r.id, "action": r.action,
                    "reasons": r.reasons, "bytes": r.bytes} for r in runs], f)


def finish(out_path, status, message, counts=None, failures=None, details=None):
    data = {"status": status, "message": message, "counts": counts or {}, "failures": failures or [],
            "details": details or {}}
    with open(out_path, "w") as f:
        json.dump(data, f)
    log(message)
    return 0


if __name__ == "__main__":
    sys.exit(main())
