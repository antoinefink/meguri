"""Command line: python3 -m meguri_similarity {index,scan,jplag} ...

Exit codes: 0 pass (or warn), 1 fail (a file is flagged), 2 error.
Logs go to stderr; stdout carries one JSON summary line.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time

from . import report
from .config import ConfigError, load_allowlist, load_config, load_vocab
from .corpus import CorpusError, open_or_build, plan_corpus
from .lexer import KEYWORDS, Lexer
from .scan import Scanner, read_text

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def stopwords_of(cfg):
    sw = cfg["channels"]["lex"].get("string_stopwords", [])
    if sw == "sql-keywords":
        return KEYWORDS["sql"]
    return frozenset(w.lower() for w in sw)


def _common(p):
    p.add_argument("--refs", required=True, help="directory holding the Anki repository (refs/)")
    p.add_argument("--config", default=os.path.join(HERE, "config.yaml"))
    p.add_argument("--cache", required=True, help="index cache directory (build/similarity-index)")
    p.add_argument("--vocab-root", help="checkout whose specs/*/interop-vocab.txt mask the lex channel")
    p.add_argument("--rebuild", action="store_true", help="rebuild the index even when cached")


def _prepare(args):
    cfg = load_config(args.config)
    stop = stopwords_of(cfg)
    mask, digest = load_vocab(args.vocab_root)
    plan = plan_corpus(args.refs, cfg, digest, stop)
    t0 = time.monotonic()
    index, built = open_or_build(plan, args.cache, mask, stop, log=log, rebuild=args.rebuild)
    corpus = dict(plan.summary())
    corpus.update(
        {
            "built": built,
            "index_seconds": round(time.monotonic() - t0, 2),
            "blobs": index.meta["stats"]["blobs"],
            "build_seconds": index.meta["stats"]["seconds"],
            "fingerprints": index.meta["stats"]["fingerprints"],
        }
    )
    return cfg, stop, mask, index, corpus


def cmd_index(args):
    _cfg, _stop, _mask, _index, corpus = _prepare(args)
    print(json.dumps({"status": "pass", "corpus": corpus}))
    return 0


def _read_list(path):
    with open(path, encoding="utf-8") as f:
        return [line.rstrip("\n") for line in f if line.strip()]


def cmd_scan(args):
    t_start = time.monotonic()
    cfg, stop, mask, index, corpus = _prepare(args)
    allow = load_allowlist(args.allowlist)
    t_scan = time.monotonic()
    lexer = Lexer(mask, stop)
    scanner = Scanner(cfg, index, lexer, allow)
    paths = list(args.paths)
    if args.files_from:
        paths += _read_list(args.files_from)
    files, skipped, missing = [], 0, 0
    for path in sorted(set(paths)):
        if not scanner.in_scope(path):
            skipped += 1
            continue
        if not os.path.isfile(os.path.join(args.root, path)):
            missing += 1
            continue
        files.append(scanner.scan_file(path, read_text(args.root, path)))
    scan_seconds = time.monotonic() - t_scan
    flagged = [f for f in files if f.status == "flagged"]
    errors = [f for f in files if f.status == "error"]
    status = "error" if errors else "fail" if flagged else "pass"
    summary = {
        "status": status,
        "counts": {
            "scanned": len(files),
            "flagged": len(flagged),
            "allowlisted": sum(1 for f in files if f.status == "allowlisted"),
            "clean": sum(1 for f in files if f.status == "clean"),
            "errors": len(errors),
            "skipped": skipped,
            "missing": missing,
        },
        "thresholds": cfg["channels"],
        "timing": {"scan_seconds": round(scan_seconds, 3), "total_seconds": round(time.monotonic() - t_start, 3)},
        "corpus": corpus,
    }
    paths_out = report.write(args.out, files, summary)
    # The summary carries the implementer view of each flag only.
    summary["flags"] = [
        {
            "path": f.path,
            "channel": name,
            "reasons": c.reasons,
            "regions": [[r.start, r.end] for r in c.regions[:20]],
        }
        for f in flagged
        for name, c in f.channels.items()
        if c.flagged
    ]
    summary["errors"] = [f.error for f in errors]
    summary["reports"] = paths_out
    with open(os.path.join(args.out, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps({"status": status, "summary": os.path.join(args.out, "summary.json")}))
    return {"pass": 0, "fail": 1}.get(status, 2)


def cmd_jplag(args):
    from . import jplag

    return jplag.main(args, log)


def main(argv=None):
    ap = argparse.ArgumentParser(prog="meguri_similarity")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("index", help="build (or reuse) the corpus index")
    _common(p)
    p.set_defaults(func=cmd_index)
    p = sub.add_parser("scan", help="scan Meguri files")
    _common(p)
    p.add_argument("--root", required=True, help="Meguri tree the paths are relative to")
    p.add_argument("--allowlist", default=os.path.join(HERE, "allowlist.yaml"))
    p.add_argument("--files-from", help="file listing paths to scan, one per line")
    p.add_argument("--out", required=True, help="report directory")
    p.add_argument("paths", nargs="*")
    p.set_defaults(func=cmd_scan)
    p = sub.add_parser("jplag", help="nightly JPlag comparison against selected Anki tags")
    p.add_argument("--refs", required=True)
    p.add_argument("--config", default=os.path.join(HERE, "config.yaml"))
    p.add_argument("--root", required=True)
    p.add_argument("--files-from", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--jar", default=os.environ.get("JPLAG_JAR", "/opt/jplag/jplag.jar"))
    p.set_defaults(func=cmd_jplag)
    args = ap.parse_args(argv)
    try:
        return args.func(args)
    except (ConfigError, CorpusError) as e:
        log(f"similarity: {e}")
        print(json.dumps({"status": "error", "error": str(e)}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
