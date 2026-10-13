"""Shared helpers for the scanner tests (run in the similarity container)."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
PKG_ROOT = os.path.dirname(HERE)  # tools/similarity
CONFIG = os.path.join(PKG_ROOT, "config.yaml")
FIXTURES = os.path.join(HERE, "fixtures")
REFS = os.environ.get("MG_TEST_REFS", "/refs")
OUT = os.environ.get("MG_TEST_OUT")


def git(repo, *args, env=None):
    e = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t", GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t")
    if env:
        e.update(env)
    return subprocess.run(["git", "-C", repo, *args], check=True, capture_output=True, text=True, env=e).stdout


def make_corpus(root, tags):
    """A fake corpus: refs dir with an `anki` git repo holding one commit per
    tag. `tags` is a list of (tag, {path: content}) applied in order."""
    repo = os.path.join(root, "anki")
    os.makedirs(repo)
    git(repo, "init", "-q")
    for tag, files in tags:
        for path, content in files.items():
            full = os.path.join(repo, path)
            os.makedirs(os.path.dirname(full), exist_ok=True)
            if content is None:
                os.remove(full)
            else:
                with open(full, "w") as f:
                    f.write(content)
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "--allow-empty", "-m", tag)
        git(repo, "tag", tag)
    return root


def synthetic_config(base, lo, hi, count):
    """The scanner config with its pinned corpus (corpus.tags) set to a
    synthetic corpus's release tags; returns its path."""
    import yaml

    with open(CONFIG) as f:
        cfg = yaml.safe_load(f)
    cfg["corpus"]["tags"] = {"min": lo, "max": hi, "count": count}
    path = os.path.join(base, f"config-{lo}-{hi}-{count}.yaml")
    with open(path, "w") as f:
        yaml.safe_dump(cfg, f)
    return path


def write_tree(root, files):
    for path, content in files.items():
        full = os.path.join(root, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w") as f:
            f.write(content)
    return root


def read_json(path):
    with open(path) as f:
        return json.load(f)


def run_cli(*args):
    """Run the scanner CLI; returns (exit code, parsed stdout JSON, stderr)."""
    env = dict(os.environ, PYTHONPATH=PKG_ROOT)
    r = subprocess.run([sys.executable, "-m", "meguri_similarity", *args], capture_output=True, text=True, env=env)
    try:
        out = json.loads(r.stdout.strip().splitlines()[-1]) if r.stdout.strip() else {}
    except json.JSONDecodeError:
        out = {"raw": r.stdout}
    return r.returncode, out, r.stderr


def scan(refs, cache, root, paths, out, allowlist=None, config=CONFIG, vocab_root=None, extra=()):
    shutil.rmtree(out, ignore_errors=True)  # never read a previous scan's reports
    args = ["scan", "--refs", refs, "--cache", cache, "--root", root, "--out", out, "--config", config]
    args += ["--allowlist", allowlist or os.path.join(FIXTURES, "empty-allowlist.yaml")]
    if vocab_root:
        args += ["--vocab-root", vocab_root]
    args += list(extra) + list(paths)
    code, line, err = run_cli(*args)
    views = {}
    for name in ("implementer", "reviewer"):
        p = os.path.join(out, f"{name}.json")
        if os.path.exists(p):
            with open(p) as f:
                views[name] = json.load(f)
    summary = {}
    if os.path.exists(os.path.join(out, "summary.json")):
        with open(os.path.join(out, "summary.json")) as f:
            summary = json.load(f)
    return code, summary, views, err


def file_entry(view, path):
    for f in view["files"]:
        if f["path"] == path:
            return f
    raise AssertionError(f"{path} not in the {view['view']} view")


def tmpdir():
    return tempfile.mkdtemp(prefix="simtest-")


def record_calibration(name, value):
    """Calibration numbers (scores and timings only) for the tools/mg result."""
    if not OUT:
        return
    path = os.path.join(OUT, "calibration.json")
    data = {}
    if os.path.exists(path):
        with open(path) as f:
            data = json.load(f)
    data[name] = value
    with open(path, "w") as f:
        json.dump(data, f, indent=1, sort_keys=True)
