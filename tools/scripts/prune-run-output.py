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
KEEP_DAYS = 7  # every run started or finished less than 7 days ago
# Always kept as well, whatever their age:
# - the run a job's `latest` link names;
# - a run whose id appears in a tracked or uncommitted file of any checkout,
#   in any local or remote branch tip, in a build/mg state file (the catch-up
#   queue, lease holders) or in a .json/.jsonl file at the top of a kept run
#   (gate and merge receipts name their child runs), in any checkout;
# - a run without result.json or with .mg/ (running, or killed while it ran),
#   and a run whose tools/mg process (the pid in its id) still runs;
# - a run folder holding a tracked file.
# Cleanroom runs (--cleanroom) follow the same rule for their folders, but
# lose source/ and codex-home/ once their records are verified in an archive.

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
STATE_DEPTH = 4
# build/ outside build/mg holds handoffs that name runs (orchestration
# checkpoints, prompts, saved logs): read for references, BUILD_DEPTH deep.
# These folders hold only build products, caches and private data copies.
BUILD_UNREAD = ("mg", "private", "docker-cargo", "app", "xcode-check", "swift-test",
                "selftest-xcframework", "oracle-macos-venv")
BUILD_DEPTH = 3
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


def git(args, cwd, stdin=None):
    p = subprocess.run(["git", "-C", cwd] + args, stdin=stdin, stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE)
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

    def file(self, path, out=None):
        out = self.ids if out is None else out
        out.update(ids_in(os.fsencode(path)))  # an id in a file or folder name counts too
        try:
            st = os.lstat(path)
            if stat.S_ISLNK(st.st_mode):
                out.update(ids_in(os.fsencode(os.readlink(path))))
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
            out.update(ids_in(os.fsencode(path)))
            self.tree(path, depth, out)
        else:
            self.file(path, out)

    def tree(self, path, depth, out):
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
                if depth > 1:
                    self.tree(e.path, depth - 1, out)
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
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
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
                             stdout=subprocess.PIPE, stderr=err)
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
        meta = entry.split(b"\t", 1)[0].split()
        if len(meta) == 3 and not meta[0].startswith(b"160"):  # not a submodule
            with refs.lock:
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
                    self.result = json.load(f)
            except (OSError, ValueError):
                self.result = None


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
                if c.name == "latest" and RUN_ID.match(os.path.basename(target)):
                    self.latest[job] = os.path.basename(target)
                elif m:
                    self.skipped.append({"path": c.path, "why": "run folder is a symbolic link; never followed"})
                    if os.path.isdir(c.path):
                        refs.tree(c.path, 2, self.state)
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
            if len(parts) >= 4 and RUN_ID.match(parts[3]):
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
    if cur:
        wts.append(cur)
    result = []
    for w in wts:
        if w.get("bare") or not os.path.isdir(w["path"]):
            continue
        w["path"] = os.path.realpath(w["path"])
        result.append(w)
    return result


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


def protect(ctx, ck, cands, others, tracked):
    """Give each candidate run its keep reasons (empty: it may go)."""
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


def source_kept_because(ctx, run):
    """Why a cleanroom run's source copy must stay, or None when it may go."""
    res = run.result or {}
    if res.get("status") != "pass" or res.get("dry_run") is True:
        return "did not pass"
    if "--check" in (res.get("args") or []):
        return None
    if not stat.S_ISREG(run.entries.get("changes.patch", 0)):
        return "exported no changes.patch"
    if (res.get("details") or {}).get("source_commit") not in ctx.reachable:
        return "no branch, tag or HEAD reaches its base commit"
    return None


def plan_cleanroom(ctx, ck, tracked):
    cands = [r for r in ck.runs if r.job == CLEANROOM]
    others = [r for r in ck.runs if r.job != CLEANROOM]
    protect(ctx, ck, cands, others, tracked)
    for r in cands:
        unknown = sorted(n for n in r.entries if n not in CLEANROOM_RECORDS + CLEANROOM_RUNTIME)
        bad = sorted(n for n in CLEANROOM_RECORDS if n in r.entries and not stat.S_ISREG(r.entries[n]))
        bad += sorted(n for n in CLEANROOM_RUNTIME if n in r.entries and not stat.S_ISDIR(r.entries[n]))
        hold = [x for x in r.reasons if x in ("unfinished", "live", "tracked")]
        if hold or unknown or bad:
            r.action = "untouched"
            if unknown and ".mg" not in unknown:
                r.reasons.append("unexpected entries: " + ", ".join(unknown))
            if bad:
                r.reasons.append("not a plain file or folder: " + ", ".join(bad))
            continue
        why = source_kept_because(ctx, r) if "source" in r.entries else None
        r.source_ok = why is None
        if why:
            r.action = "untouched"
            r.reasons.append("source kept: " + why)
            continue
        r.action = "strip" if r.reasons else "remove"
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
            if getattr(r, "source_ok", True) is False:
                s["source_kept"] += 1
                s["bytes_source_kept"] += r.bytes
                why = r.reasons[-1]
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

def under(path, root):
    return os.path.realpath(path).startswith(root + os.sep)


O_DIR = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
PENDING = ".prune-deleting-"  # a checked run folder renamed for deletion; --apply finishes it


def remove_at(dfd, name, dev):
    """Delete entry name of the open folder dfd with all it holds. Every step goes
    through an open descriptor (O_NOFOLLOW), so a folder swapped for a link while
    this runs is never followed, and another file system is never entered."""
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
        if st.st_dev != dev:
            raise Refusal("%s is on another file system" % name)
        try:
            entries = [(e.name, e.is_dir(follow_symlinks=False)) for e in os.scandir(fd)]
        except PermissionError:
            os.fchmod(fd, stat.S_IRWXU)
            entries = [(e.name, e.is_dir(follow_symlinks=False)) for e in os.scandir(fd)]
        for n, is_dir in entries:
            if is_dir:
                remove_at(fd, n, dev)
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
    os.rmdir(name, dir_fd=dfd)


def newest_write(path):
    """Newest mtime of the plain files at the top of a folder (Run.written)."""
    newest = 0
    for e in os.scandir(path):
        try:
            if e.is_file(follow_symlinks=False):
                newest = max(newest, e.stat(follow_symlinks=False).st_mtime)
        except FileNotFoundError:
            pass
    return newest


class Deleter:
    """Deletes only run folders (or a cleanroom run's runtime parts) below one
    checkout's build/mg, through descriptors opened from a checked build/mg."""

    def __init__(self, ck, ctx):
        self.ck = ck
        self.ctx = ctx
        root = os.path.realpath(ck.path)
        mg_real = os.path.realpath(ck.mg)
        if mg_real != os.path.join(root, "build", "mg"):
            raise Refusal("%s resolves to %s, outside this checkout's build/mg; nothing deleted" % (ck.mg, mg_real))
        for p in (os.path.join(root, "build"), ck.mg):
            st = os.lstat(p)
            if stat.S_ISLNK(st.st_mode) or not stat.S_ISDIR(st.st_mode):
                raise Refusal("%s is not a plain folder; nothing deleted" % p)
        self.mg = mg_real
        self.mg_fd = os.open(self.mg, O_DIR)
        st = os.fstat(self.mg_fd)
        lst = os.lstat(self.mg)
        if (st.st_dev, st.st_ino) != (lst.st_dev, lst.st_ino):
            raise Refusal("%s changed while it was opened; nothing deleted" % self.mg)
        self.dev = st.st_dev
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
        """(job, id) of run folders holding a tracked file, read again whenever the index changed."""
        try:
            st = os.stat(self.index)
            seen = (st.st_ino, st.st_size, st.st_mtime_ns)
        except FileNotFoundError:
            seen = None
        if seen != self.index_seen or seen is None:
            self.tracked = self.ck.tracked_runs()
            self.index_seen = seen
        return self.tracked

    def check(self, run, part=None):
        """Why this path must not be deleted now, or None. Checked again just
        before each deletion: a job may have started or finished since the plan."""
        job_dir = os.path.join(self.mg, run.job)
        expect = os.path.join(job_dir, run.id)
        st = lstat(job_dir)
        if st is None or stat.S_ISLNK(st.st_mode) or not stat.S_ISDIR(st.st_mode):
            return "its job folder is not a plain folder"
        st = lstat(run.path)
        if st is None:
            return "gone"
        if stat.S_ISLNK(st.st_mode) or not stat.S_ISDIR(st.st_mode):
            return "not a plain folder"
        if run.path != expect or os.path.realpath(run.path) != expect or not under(run.path, self.mg):
            return "resolves outside build/mg"
        if st.st_dev != self.dev:
            return "on another file system"
        if not os.path.isfile(os.path.join(run.path, "result.json")) or os.path.lexists(os.path.join(run.path, ".mg")):
            return "unfinished"
        if self.ctx.live(run):
            return "its tools/mg process runs"
        if run.key in self.tracked_now():
            return "holds a tracked file"
        if any(c.path == run.path or c.path.startswith(run.path + os.sep) for c in self.ctx.checkouts):
            return "holds a checkout"
        # A detached writer (a VM's console log, a container's output) can
        # outlive its job: a file written in that time keeps the folder.
        quiet = 3600 if part is not None else KEEP_DAYS * 86400
        if newest_write(run.path) > time.time() - quiet:
            return "written to in the last %d s" % quiet
        if part is not None:
            # Only a cleanroom run's source/ or codex-home/; its folder stays.
            if part not in CLEANROOM_RUNTIME or run.job != CLEANROOM:
                return "%s is not runtime state" % part
            pst = lstat(os.path.join(run.path, part))
            if pst is None:
                return "gone"
            if stat.S_ISLNK(pst.st_mode) or not stat.S_ISDIR(pst.st_mode):
                return "%s is not a plain folder" % part
            return None
        link = os.path.join(job_dir, "latest")
        if os.path.islink(link) and os.path.basename(os.readlink(link)) == run.id:
            return "latest"
        return None

    def remove(self, run, part=None):
        why = self.check(run, part)
        if why is not None:
            return why
        jfd = os.open(run.job, O_DIR, dir_fd=self.mg_fd)
        try:
            if part is None:
                # Renamed first, so a deletion cut short is finished by the next
                # --apply rather than left as an "unfinished" run.
                os.rename(run.id, PENDING + run.id, src_dir_fd=jfd, dst_dir_fd=jfd)
                remove_at(jfd, PENDING + run.id, self.dev)
            else:
                rfd = os.open(run.id, O_DIR, dir_fd=jfd)
                try:
                    remove_at(rfd, part, self.dev)
                finally:
                    os.close(rfd)
        finally:
            os.close(jfd)
        return None

    def finish_pending(self, failures):
        """Delete what an earlier --apply renamed for deletion and could not finish."""
        done = 0
        for job, name in self.ck.pending:
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
    through a link), read it back and compare, then move it over name."""
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


def archive_and_delete(ctx, ck, deleter, adfd, adir, cands, failures):
    work = [r for r in cands if r.action in ("strip", "remove")]
    months = {}
    for r in work:
        months.setdefault("%s-%s" % (r.id[0:4], r.id[4:6]), []).append(r)
    report = {"archived_runs": 0, "removed_runs": 0, "stripped_runs": 0, "bytes_freed": 0,
              "archives": {}, "skipped": []}
    for month in sorted(months):
        runs = months[month]
        name = month + ".tar.xz"
        path = os.path.join(adir, name)
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
                clash = [n for n in recs if n in have and have[n] != recs[n]]
                if clash:
                    failures.append({"id": "archive/%s" % r.id, "message": "%s holds other content for %s; the run was left untouched" % (path, ", ".join(clash))})
                    bad.add(r.id)
                    continue
                new = {n: v for n, v in recs.items() if n not in have}
                if new:
                    add[r.id] = new
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
            try:
                freed = 0
                if r.action == "remove":
                    try:
                        listed = set(os.listdir(r.path))
                    except FileNotFoundError:
                        listed = set(r.entries)
                    # A file added since the plan would go unarchived.
                    why = deleter.remove(r) if listed == set(r.entries) else "its folder changed since the plan"
                    if why is None:
                        report["removed_runs"] += 1
                        freed = r.bytes
                    elif why != "gone":
                        report["skipped"].append({"run": r.id, "why": why})
                    report["bytes_freed"] += freed
                    continue
                for part in CLEANROOM_RUNTIME:
                    if part in r.entries:
                        why = deleter.remove(r, part)
                        if why is None:
                            freed += r.parts.get(part, 0)
                        elif why != "gone":
                            report["skipped"].append({"run": r.id, "why": "%s: %s" % (part, why)})
                if freed:
                    report["stripped_runs"] += 1
                report["bytes_freed"] += freed
            except (OSError, Refusal) as e:
                failures.append({"id": "delete/cleanroom/%s" % r.id, "message": "could not delete in %s: %s" % (r.path, e)})
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
        log("  %d runs kept whole (%s), their source copy possibly the only copy of their work: %s"
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
    root = self_root
    if args.root:
        root = os.path.realpath(args.root)
        if args.apply and root != self_root:
            return fail("--root only reports; run tools/mg prune --apply from the checkout you prune")
    wts = worktree_list(self_root)
    known = [w["path"] for w in wts]
    if root not in known:
        return fail("%s is not a checkout of this repository (git worktree list)" % root)
    if args.apply and os.environ.get("CI"):
        return fail("--apply does not run in CI", "ci")

    try:
        ctx = Context(time.time(), processes())
    except OSError as e:
        return fail("cannot tell which runs are live: %s" % e, "processes")
    mode = "all-worktrees" if args.all_worktrees else ("cleanroom" if args.cleanroom else "runs")
    log("prune: %s, %s" % (mode, "applying" if args.apply else "report only (--apply deletes)"))
    log("rule: keep the newest %d runs of each job, every run started, finished or written to in the last %d days, "
        "and every latest, referenced, unfinished, live or tracked run (tools/AGENTS.md)" % (KEEP_NEWEST, KEEP_DAYS))

    try:
        return run(args, ctx, wts, root, targets, out_path, mode)
    except Refusal as e:
        return fail(str(e), "refused")


def run(args, ctx, wts, root, targets, out_path, mode):
    t0 = time.time()
    refs = ctx.refs
    ctx.checkouts = [Checkout(w["path"]) for w in wts]
    deleter = None
    if args.apply:
        # Checked and locked before anything is read: the plan is made under the lock.
        deleter = Deleter(next(ck for ck in ctx.checkouts if ck.path == root), ctx)
        if not deleter.lock():
            raise Refusal("another tools/mg prune --apply runs in this checkout; nothing deleted")

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
        # A source copy goes only when a ref (a branch, tag or worktree HEAD)
        # still reaches its base, so gc never takes the commit its patch needs.
        heads = "".join(w["head"] + "\n" for w in wts if w.get("head"))
        p = subprocess.run(["git", "-C", args.self_root, "rev-list", "--all", "--stdin"],
                           input=heads.encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if p.returncode != 0:
            refs.errors.append("git rev-list --all failed: %s" % p.stderr.decode(errors="replace").strip())
        else:
            ctx.reachable = commits & set(p.stdout.decode().split())
    ctx.ref_stats = {"branch_commits": ncommits, "objects": nblobs, "run_ids": len(refs.ids),
                     "run_ids_in_branches": in_git, "run_ids_uncommitted": len(uncommitted),
                     "run_ids_in_state_files": len(set().union(*[ck.state for ck in ctx.checkouts])),
                     "errors": refs.errors[:50]}
    log("references: %d run ids from %d branch tips and worktree HEADs, %d checkouts' uncommitted files, build/mg state and receipts (%.0f s)"
        % (len(refs.ids), ncommits, len(wts), time.time() - t0))
    for e in refs.errors[:20]:
        log("  could not read: %s" % e)
    if args.apply and refs.errors:
        raise Refusal("some reference sources could not be read, so a referenced run could be missed; nothing deleted")

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
    for r in cands:
        plan_rows.append({"job": r.job, "run": r.id, "action": r.action, "reasons": r.reasons, "bytes": r.bytes})
    with open(os.path.join(args.run_dir, "plan.json"), "w") as f:
        json.dump(plan_rows, f)
    for f in failures:
        log("  failed: %s: %s" % (f["id"], f["message"]))
    status = "fail" if failures else ("warn" if refs.errors else "pass")
    if refs.errors and not failures:
        message += "; some reference sources could not be read (details.references.errors), so --apply refuses"
    return finish(out_path, status, message, counts, failures, details)


def finish(out_path, status, message, counts=None, failures=None, details=None):
    data = {"status": status, "message": message, "counts": counts or {}, "failures": failures or [],
            "details": details or {}}
    with open(out_path, "w") as f:
        json.dump(data, f)
    log(message)
    return 0


if __name__ == "__main__":
    sys.exit(main())
