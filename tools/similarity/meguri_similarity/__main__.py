"""Command line: python3 -m meguri_similarity {index,scan,jplag} ...

Exit codes: 0 pass (or warn), 1 fail (a file is flagged), 2 error.
Logs go to stderr; stdout carries one JSON summary line. `scan` also writes
<out>/summary.json, with the reason when it fails with an error.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time

from . import report
from . import test_data
from .config import ConfigError, load_allowlist, load_config, load_vocab
from .corpus import CorpusError, open_or_build, plan_corpus
from .lexer import KEYWORDS, Lexer
from .scan import Scanner, read_list, read_text

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


class InputError(Exception):
    """Scan input the scanner refuses."""


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def checkout_paths(root, paths):
    """The given paths relative to `root`, normalized and deduplicated: `./x`,
    `a/../x`, `a//x` and `<root>/x` all become `x`, so the scope rules, the
    allowlist and the reports see one spelling. A path outside root is an
    InputError."""
    base = os.path.abspath(root)
    out, outside = set(), []
    for path in paths:
        rel = os.path.relpath(os.path.normpath(os.path.join(base, path)), base)
        if rel == os.pardir or rel.startswith(os.pardir + os.sep):
            outside.append(path)
        else:
            out.add(rel)
    if outside:
        raise InputError(f"paths outside the scanned tree {root}: {', '.join(outside)}")
    return sorted(out)


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


def cmd_scan(args):
    t_start = time.monotonic()
    paths = list(args.paths)
    if args.files_from:
        paths += read_list(args.files_from, args.null)
    paths = checkout_paths(args.root, paths)
    data_inputs, pairs = None, 0
    if getattr(args, "test_data", False):
        cfg = load_config(args.config)
        data_inputs, pairs = test_data.inputs(args.root, paths, cfg["corpus"]["max_blob_bytes"])
    cfg, stop, mask, index, corpus = _prepare(args)
    allow = load_allowlist(args.allowlist)
    t_scan = time.monotonic()
    lexer = Lexer(mask, stop)
    scanner = Scanner(cfg, index, lexer, allow)
    files, skipped, missing = [], 0, []
    for path in ([] if data_inputs is not None else paths):
        if not scanner.in_scope(path):
            skipped += 1
            continue
        if not os.path.isfile(os.path.join(args.root, path)):
            # Listed for the scan but not there: a caller bug (or a race) that
            # must not pass silently.
            missing.append(path)
            continue
        files.append(scanner.scan_file(path, read_text(args.root, path)))
    oracle_output = []
    if data_inputs is not None:
        files = [scanner.scan_test_data(item["path"], item["text"]) for item in data_inputs]
        rule = cfg["scan"].get("oracle_goldens")
        pin = test_data.oracle_pin(args.root, rule.get("pin")) if isinstance(rule, dict) else None
        scenarios = {os.path.splitext(item["path"][len(test_data.ROOTS[0]) + 1:])[0]: item
                     for item in data_inputs if item["path"].startswith(test_data.ROOTS[0] + "/")}
        for f, item in zip(files, data_inputs):
            if f.status != "flagged" or pin is None or not f.path.startswith(test_data.ROOTS[1] + "/"):
                continue
            scenario = scenarios.get(os.path.splitext(f.path[len(test_data.ROOTS[1]) + 1:])[0])
            sha = test_data.scenario_sha256(scenario["path"], scenario["text"]) if scenario else None
            if test_data.oracle_recorded(f.path, item["text"], pin, sha):
                f.status = test_data.ORACLE_OUTPUT
                oracle_output.append(f.path)
    scan_seconds = time.monotonic() - t_scan
    flagged = [f for f in files if f.status == "flagged"]
    errors = [f for f in files if f.status == "error"]
    status = "error" if errors else "fail" if flagged else "warn" if missing else "pass"
    summary = {
        "status": status,
        "counts": {
            "scanned": len(files),
            "flagged": len(flagged),
            "allowlisted": sum(1 for f in files if f.status == "allowlisted"),
            "oracle_output": len(oracle_output),
            "clean": sum(1 for f in files if f.status == "clean"),
            "errors": len(errors),
            "skipped": skipped,
            "missing": len(missing),
        },
        "thresholds": cfg["channels"],
        "timing": {"scan_seconds": round(scan_seconds, 3), "total_seconds": round(time.monotonic() - t_start, 3)},
        "corpus": corpus,
    }
    if data_inputs is not None:
        def meaningful(file):
            return any(
                channel.fingerprints >= (cfg["channels"][name]["min_fingerprints"] if name == "lex" else 1)
                for name, channel in file.channels.items()
            )
        coverage = [{"path": file.path,
                     "fingerprints": {name: channel.fingerprints for name, channel in file.channels.items()},
                     "meaningful": meaningful(file)} for file in files]
        summary["test_data"] = {
            "pairs": pairs,
            "manifest": [{"path": item["path"], "sha256": item["sha256"]} for item in data_inputs],
            "coverage": coverage,
            "limited": [item["path"] for item in coverage if not item["meaningful"]],
            # Goldens whose matches are recorded oracle output: reported, not failing.
            "oracle_output": oracle_output,
            "limits": "Similarity does not establish provenance or exclude copying below the configured fingerprint limits.",
        }
        if summary["test_data"]["limited"] and status == "pass":
            status = summary["status"] = "warn"
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
    summary["missing"] = missing
    summary["reports"] = paths_out
    with open(os.path.join(args.out, "summary.json"), "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps({"status": status, "summary": os.path.join(args.out, "summary.json")}))
    return {"pass": 0, "warn": 0, "fail": 1}.get(status, 2)


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
    p.add_argument("--null", action="store_true", help="paths in --files-from are NUL-terminated")
    p.add_argument("--out", required=True, help="report directory")
    p.add_argument("--test-data", "--search-data", dest="test_data", action="store_true",
                   help="explicit paired conformance scenario/golden data scan (--search-data: the old name)")
    p.add_argument("paths", nargs="*")
    p.set_defaults(func=cmd_scan)
    p = sub.add_parser("jplag", help="nightly JPlag comparison against selected Anki tags")
    p.add_argument("--refs", required=True)
    p.add_argument("--config", default=os.path.join(HERE, "config.yaml"))
    p.add_argument("--root", required=True)
    p.add_argument("--files-from", required=True)
    p.add_argument("--null", action="store_true", help="paths in --files-from are NUL-terminated")
    p.add_argument("--out", required=True)
    p.add_argument("--jar", default=os.environ.get("JPLAG_JAR", "/opt/jplag/jplag.jar"))
    p.set_defaults(func=cmd_jplag)
    args = ap.parse_args(argv)
    try:
        return args.func(args)
    except (ConfigError, CorpusError, InputError, test_data.TestDataError) as e:
        log(f"similarity: {e}")
        result = {"status": "error", "error": str(e)}
        if args.cmd == "scan":
            os.makedirs(args.out, exist_ok=True)
            with open(os.path.join(args.out, "summary.json"), "w") as f:
                json.dump(result, f)
        print(json.dumps(result))
        return 2


if __name__ == "__main__":
    sys.exit(main())
