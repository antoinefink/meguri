"""The Anki corpus: tag enumeration, parallel fingerprinting and the cached index.

The corpus is every unique blob (by git blob id) with a corpus extension, across
every release tag in the configured range of the Anki repository, plus optional
extra sources (AnkiConnect and Migaku add-on sources) when they are present.
Each channel's index holds every k-gram hash of the corpus (not only winnowed
ones), sorted and deduplicated, with the first blob and line it occurs at. The
scanned file's side is winnowed, so a copied region matches on nearly every
fingerprint.

Nothing here prints corpus text. Reports name an Anki tag, path and line only.
"""

from __future__ import annotations

import hashlib
import json
import multiprocessing
import os
import re
import shutil
import subprocess
import time
from dataclasses import dataclass

import numpy as np

from .fingerprint import IdSpace, kgram_hashes, periodicity
from .lexer import STRUCT_FAMILY, Lexer, language_of

INDEX_VERSION = 1  # bump when lexing or fingerprinting changes corpus hashes
CHANNELS = ("struct", "lex", "com", "text")
_RELEASE_TAG = re.compile(r"^\d+(?:\.\d+)+$")


class CorpusError(Exception):
    pass


def version_tuple(tag: str):
    return tuple(int(p) for p in tag.split("."))


def _git(repo, *args, binary=False):
    try:
        r = subprocess.run(["git", "-C", repo, *args], capture_output=True, check=True)
    except subprocess.CalledProcessError as e:
        raise CorpusError(f"git {' '.join(args[:2])} failed in {repo}: {e.stderr.decode(errors='replace').strip()}")
    return r.stdout if binary else r.stdout.decode()


def _is_git_repo(path):
    """A repository whose top level is `path` itself (not a directory inside
    some other checkout)."""
    if not os.path.isdir(path):
        return False
    r = subprocess.run(["git", "-C", path, "rev-parse", "--show-toplevel"], capture_output=True, text=True)
    return r.returncode == 0 and os.path.realpath(r.stdout.strip()) == os.path.realpath(path)


@dataclass
class Plan:
    """What the index is built from; its key names the cache directory."""

    refs_dir: str
    sources: list  # dicts: name, kind, path, [tags: [(tag, commit)]] / [head] / [files]
    missing_sources: list
    missing_boundary_tags: list
    params: dict
    vocab_digest: str
    key: str

    def summary(self):
        anki = next((s for s in self.sources if s["name"] == "anki"), None)
        tags = [t for t, _ in anki["tags"]] if anki else []
        return {
            "key": self.key,
            "tags": len(tags),
            "first_tag": tags[0] if tags else None,
            "last_tag": tags[-1] if tags else None,
            "missing_boundary_tags": self.missing_boundary_tags,
            "extra_sources": [s["name"] for s in self.sources if s["name"] != "anki"],
            "missing_sources": self.missing_sources,
        }


def plan_corpus(refs_dir: str, cfg: dict, vocab_digest: str, stopwords) -> Plan:
    ccfg = cfg["corpus"]
    anki_path = os.path.join(refs_dir, ccfg["anki_repo"])
    if not _is_git_repo(anki_path):
        raise CorpusError(f"no Anki repository at {ccfg['anki_repo']} in the corpus directory")
    lo, hi = version_tuple(ccfg["tags"]["min"]), version_tuple(ccfg["tags"]["max"])
    refs = _git(anki_path, "for-each-ref", "--format=%(refname:short)\t%(objectname)\t%(*objectname)", "refs/tags")
    tags = []
    for line in refs.splitlines():
        name, obj, peeled = (line.split("\t") + ["", ""])[:3]
        if _RELEASE_TAG.match(name) and lo <= version_tuple(name) <= hi:
            tags.append((name, peeled or obj))
    tags.sort(key=lambda t: version_tuple(t[0]))
    if not tags:
        raise CorpusError(f"no release tags between {ccfg['tags']['min']} and {ccfg['tags']['max']} in the Anki repository")
    present = {version_tuple(t) for t, _ in tags}
    missing_boundary = [t for t in (ccfg["tags"]["min"], ccfg["tags"]["max"]) if version_tuple(t) not in present]
    sources = [{"name": "anki", "kind": "git-tags", "path": anki_path, "tags": tags}]
    missing = []
    for extra in ccfg.get("extra_sources") or []:
        path = os.path.join(refs_dir, extra["path"])
        if _is_git_repo(path):
            head = _git(path, "rev-parse", "HEAD").strip()
            sources.append({"name": extra["name"], "kind": "git-head", "path": path, "head": head})
        elif os.path.isdir(path):
            files = []
            for dirpath, _dirs, names in os.walk(path):
                for n in sorted(names):
                    full = os.path.join(dirpath, n)
                    rel = os.path.relpath(full, path)
                    if os.path.splitext(rel)[1].lower() in ccfg["extensions"]:
                        st = os.stat(full)
                        files.append((rel, st.st_size, int(st.st_mtime)))
            sources.append({"name": extra["name"], "kind": "dir", "path": path, "files": sorted(files)})
        else:
            missing.append(extra["name"])
    params = {
        "index_version": INDEX_VERSION,
        "extensions": sorted(ccfg["extensions"]),
        "max_blob_bytes": ccfg.get("max_blob_bytes", 2_000_000),
        "struct_k": cfg["channels"]["struct"]["k"],
        "struct_max_periodicity": cfg["channels"]["struct"]["max_periodicity"],
        "lex_k": cfg["channels"]["lex"]["k"],
        "com_n": cfg["channels"]["com"]["n"],
        "text_n": cfg["channels"]["text"]["n"],
        "stopwords": sorted(stopwords),
    }
    key_material = {
        "params": params,
        "vocab": vocab_digest,
        "sources": [
            {k: v for k, v in s.items() if k not in ("path",)} for s in sources
        ],
    }
    key = hashlib.sha256(json.dumps(key_material, sort_keys=True).encode()).hexdigest()[:20]
    return Plan(refs_dir, sources, missing, missing_boundary, params, vocab_digest, key)


# ---------------------------------------------------------------------------
# Enumeration


def enumerate_blobs(plan: Plan):
    """Unique corpus blobs in first-seen order: (id, source index, tag, path, language)."""
    exts = set(plan.params["extensions"])
    max_bytes = plan.params["max_blob_bytes"]
    seen = set()
    blobs = []
    skipped_large = 0
    for si, src in enumerate(plan.sources):
        if src["kind"] in ("git-tags", "git-head"):
            revs = src["tags"] if src["kind"] == "git-tags" else [("HEAD", src["head"])]
            for tag, commit in revs:
                listing = _git(src["path"], "ls-tree", "-r", "-l", "-z", "--full-tree", commit, binary=True)
                for entry in listing.split(b"\0"):
                    if not entry:
                        continue
                    meta, _, path = entry.partition(b"\t")
                    parts = meta.split()
                    if len(parts) < 4 or parts[1] != b"blob":
                        continue
                    path = path.decode("utf-8", "replace")
                    dot = path.rfind(".")
                    if dot < 0 or path[dot:].lower() not in exts:
                        continue
                    sha = parts[2].decode()
                    if sha in seen:
                        continue
                    seen.add(sha)
                    if parts[3] != b"-" and int(parts[3]) > max_bytes:
                        skipped_large += 1
                        continue
                    blobs.append((sha, si, tag, path, language_of(path)))
        else:
            for rel, size, _mtime in src["files"]:
                full = os.path.join(src["path"], rel)
                if size > max_bytes:
                    skipped_large += 1
                    continue
                with open(full, "rb") as f:
                    sha = "dir:" + hashlib.sha1(f.read()).hexdigest()
                if sha in seen:
                    continue
                seen.add(sha)
                blobs.append((sha, si, src["name"], rel, language_of(rel)))
    return blobs, skipped_large


# ---------------------------------------------------------------------------
# Parallel fingerprinting

_W = {}  # per-worker state


def _worker_init(plan_sources, params, mask, stopwords):
    _W["sources"] = plan_sources
    _W["params"] = params
    _W["lexer"] = Lexer(mask, stopwords)
    _W["ids"] = IdSpace()
    _W["catfile"] = {}


def _read_blob(si, sha_or_path, path):
    src = _W["sources"][si]
    if src["kind"] == "dir":
        with open(os.path.join(src["path"], path), "rb") as f:
            return f.read()
    proc = _W["catfile"].get(si)
    if proc is None:
        proc = subprocess.Popen(
            ["git", "-C", src["path"], "cat-file", "--batch"], stdin=subprocess.PIPE, stdout=subprocess.PIPE
        )
        _W["catfile"][si] = proc
    proc.stdin.write(sha_or_path.encode() + b"\n")
    proc.stdin.flush()
    header = proc.stdout.readline().split()
    if len(header) < 3 or header[1] != b"blob":
        raise CorpusError(f"git cat-file could not read blob {sha_or_path}")
    size = int(header[2])
    data = proc.stdout.read(size)
    proc.stdout.read(1)
    return data


def grouped_hashes(seqs, channel, n, idspace):
    """k-gram hashes over several word sequences (no k-gram crosses two):
    (hashes, first line, last line)."""
    empty = np.empty(0, np.uint64), np.empty(0, np.uint32), np.empty(0, np.uint32)
    if not seqs:
        return empty
    toks, lines, groups = [], [], []
    for g, (words, wlines) in enumerate(seqs):
        if len(words) >= n:
            toks.extend(words)
            lines.extend(wlines)
            groups.extend([g] * len(words))
    if not toks:
        return empty
    ids = idspace.get(channel).ids(toks)
    h, starts = kgram_hashes(ids, n, np.asarray(groups, dtype=np.int64))
    lines = np.asarray(lines, dtype=np.uint32)
    return h, lines[starts], lines[starts + n - 1]


def fingerprint_lexed(lexed, params, idspace):
    """Corpus-side hashes per channel for one lexed file: {channel: (hashes, lines)}."""
    out = {}
    family = STRUCT_FAMILY.get(lexed.language)
    if family and lexed.struct:
        ids = idspace.get("struct", family).ids(lexed.struct)
        h, starts = kgram_hashes(ids, params["struct_k"])
        keep = periodicity(ids, params["struct_k"]) < params["struct_max_periodicity"]
        h, starts = h[keep], starts[keep]
        out["struct"] = (h, np.asarray(lexed.struct_lines, dtype=np.uint32)[starts])
    if lexed.lex:
        ids = idspace.get("lex").ids(lexed.lex)
        h, starts = kgram_hashes(ids, params["lex_k"])
        out["lex"] = (h, np.asarray(lexed.lex_lines, dtype=np.uint32)[starts])
    if lexed.comments:
        h, first, _last = grouped_hashes(lexed.comments, "com", params["com_n"], idspace)
        out["com"] = (h, first)
    if lexed.texts:
        h, first, _last = grouped_hashes([(w, l) for w, l, _ in lexed.texts], "text", params["text_n"], idspace)
        out["text"] = (h, first)
    return out


def _work(batch):
    params = _W["params"]
    lexer = _W["lexer"]
    acc = {c: ([], [], []) for c in CHANNELS}
    tokens = 0
    for gi, si, sha, path, lang in batch:
        data = _read_blob(si, sha, path)
        text = data.decode("utf-8", "replace")
        try:
            lexed = lexer.lex(text, lang, want_struct=True, want_text_strings=lang not in ("css", "scss", "ftl", "html"))
        except (ValueError, RecursionError):
            continue
        tokens += len(lexed.struct) + len(lexed.lex)
        for channel, (h, lines) in fingerprint_lexed(lexed, params, _W["ids"]).items():
            if len(h) == 0:
                continue
            uh, first = np.unique(h, return_index=True)
            acc[channel][0].append(uh)
            acc[channel][1].append(lines[first])
            acc[channel][2].append(np.full(len(uh), gi, dtype=np.uint32))
    result = {}
    for c, (hs, ls, bs) in acc.items():
        if hs:
            result[c] = (np.concatenate(hs), np.concatenate(ls), np.concatenate(bs))
    return result, tokens


def _batches(blobs, target=48):
    batch = []
    for gi, (sha, si, tag, path, lang) in enumerate(blobs):
        batch.append((gi, si, sha, path, lang))
        if len(batch) >= target:
            yield batch
            batch = []
    if batch:
        yield batch


def build_index(plan: Plan, cache_root: str, mask, stopwords, workers=None, log=print):
    t0 = time.monotonic()
    blobs, skipped_large = enumerate_blobs(plan)
    t_enum = time.monotonic() - t0
    acc = {c: ([], [], []) for c in CHANNELS}
    tokens = 0
    ctx = multiprocessing.get_context("fork")
    with ctx.Pool(
        processes=workers or os.cpu_count(),
        initializer=_worker_init,
        initargs=(plan.sources, plan.params, mask, stopwords),
    ) as pool:
        for result, n in pool.imap(_work, _batches(blobs), chunksize=1):
            tokens += n
            for c, (h, l, b) in result.items():
                acc[c][0].append(h)
                acc[c][1].append(l)
                acc[c][2].append(b)
    t_lex = time.monotonic() - t0 - t_enum

    os.makedirs(cache_root, exist_ok=True)
    tmp = os.path.join(cache_root, f".tmp-{plan.key}-{os.getpid()}")
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    counts = {}
    for c, (hs, ls, bs) in acc.items():
        if hs:
            h = np.concatenate(hs)
            uh, first = np.unique(h, return_index=True)
            np.save(os.path.join(tmp, f"{c}.h.npy"), uh)
            np.save(os.path.join(tmp, f"{c}.b.npy"), np.concatenate(bs)[first])
            np.save(os.path.join(tmp, f"{c}.l.npy"), np.concatenate(ls)[first])
            counts[c] = int(len(uh))
        else:
            for suffix, dt in (("h", np.uint64), ("b", np.uint32), ("l", np.uint32)):
                np.save(os.path.join(tmp, f"{c}.{suffix}.npy"), np.empty(0, dt))
            counts[c] = 0
    elapsed = time.monotonic() - t0
    meta = {
        "index_version": INDEX_VERSION,
        "key": plan.key,
        "plan": plan.summary(),
        "params": plan.params,
        "sources": [s["name"] for s in plan.sources],
        "blobs": [[sha, si, tag, path] for sha, si, tag, path, _lang in blobs],
        "stats": {
            "blobs": len(blobs),
            "skipped_large_blobs": skipped_large,
            "tokens": tokens,
            "fingerprints": counts,
            "seconds": round(elapsed, 2),
            "seconds_enumerate": round(t_enum, 2),
            "seconds_lex": round(t_lex, 2),
        },
    }
    with open(os.path.join(tmp, "meta.json"), "w") as f:
        json.dump(meta, f)
    final = os.path.join(cache_root, plan.key)
    shutil.rmtree(final, ignore_errors=True)
    os.rename(tmp, final)
    # Keep only the current index: each one is a few hundred MB.
    for name in os.listdir(cache_root):
        if name != plan.key:
            shutil.rmtree(os.path.join(cache_root, name), ignore_errors=True)
    log(f"similarity: indexed {len(blobs)} blobs from {plan.summary()['tags']} tags in {elapsed:.1f}s")
    return Index(final)


class Index:
    def __init__(self, path):
        self.path = path
        with open(os.path.join(path, "meta.json")) as f:
            self.meta = json.load(f)
        self.arrays = {}
        for c in CHANNELS:
            self.arrays[c] = tuple(
                np.load(os.path.join(path, f"{c}.{s}.npy"), mmap_mode="r") for s in ("h", "b", "l")
            )

    def lookup(self, channel, hashes: np.ndarray):
        """(found mask, blob index, corpus line) for each hash."""
        h, b, l = self.arrays[channel]
        if len(h) == 0 or len(hashes) == 0:
            z = np.zeros(len(hashes), dtype=bool)
            return z, np.zeros(len(hashes), np.uint32), np.zeros(len(hashes), np.uint32)
        idx = np.searchsorted(h, hashes)
        idx = np.minimum(idx, len(h) - 1)
        found = np.asarray(h[idx]) == hashes
        return found, np.asarray(b[idx]), np.asarray(l[idx])

    def blob(self, gi):
        sha, si, tag, path = self.meta["blobs"][int(gi)]
        return self.meta["sources"][si], tag, path


def open_or_build(plan: Plan, cache_root: str, mask, stopwords, log=print, rebuild=False):
    path = os.path.join(cache_root, plan.key)
    if not rebuild and os.path.exists(os.path.join(path, "meta.json")):
        return Index(path), False
    return build_index(plan, cache_root, mask, stopwords, log=log), True
