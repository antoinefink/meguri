"""Nightly second opinion: JPlag with Anki tags as old submissions and Meguri as
the new one. The .jplag report embeds the compared sources (Anki's included),
so it stays in the main checkout's build/ and is opened with
`java -jar jplag.jar -M VIEW <file>`."""

from __future__ import annotations

import csv
import io
import json
import os
import shutil
import subprocess
import tarfile
import tempfile

from .config import glob_match, load_config
from .corpus import _RELEASE_TAG, version_tuple
from .scan import read_list


def _tags(anki, cfg):
    jcfg, ccfg = cfg["jplag"], cfg["corpus"]
    out = subprocess.run(["git", "-C", anki, "tag", "--list"], capture_output=True, text=True, check=True).stdout
    lo, hi = version_tuple(ccfg["tags"]["min"]), version_tuple(ccfg["tags"]["max"])
    release = sorted((t for t in out.split() if _RELEASE_TAG.match(t) and lo <= version_tuple(t) <= hi), key=version_tuple)
    chosen, missing = [], []
    for t in jcfg["tags"]:
        name = ccfg["tags"]["max"] if t == "latest" else t
        if name in release:
            if name not in chosen:
                chosen.append(name)
        else:
            missing.append(t)
    return chosen, missing


def _export_tag(anki, tag, suffixes, dest):
    archive = subprocess.run(["git", "-C", anki, "archive", "--format=tar", tag], capture_output=True, check=True).stdout
    count = 0
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        for member in tar.getmembers():
            if member.isfile() and os.path.splitext(member.name)[1] in suffixes:
                target = os.path.join(dest, member.name)
                os.makedirs(os.path.dirname(target), exist_ok=True)
                with tar.extractfile(member) as src, open(target, "wb") as f:
                    shutil.copyfileobj(src, f)
                count += 1
    return count


def _write_summary(summary, out):
    path = os.path.join(out, "jplag-summary.json")
    with open(path, "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps({"status": summary["status"], "summary": path}))
    return {"pass": 0, "warn": 0}.get(summary["status"], 2)


def main(args, log):
    cfg = load_config(args.config)
    jcfg = cfg["jplag"]
    anki = os.path.join(args.refs, cfg["corpus"]["anki_repo"])
    tags, missing = _tags(anki, cfg)
    os.makedirs(args.out, exist_ok=True)
    summary = {"status": "pass", "tags": tags, "missing_tags": missing, "languages": {}}
    if missing or not tags:
        summary["status"] = "error"
        summary["error"] = (
            f"required JPlag references are incomplete: {len(missing)} configured tag(s) missing; "
            f"{len(tags)} tag(s) selected"
        )
        return _write_summary(summary, args.out)
    files = read_list(args.files_from, args.null)
    files = [p for p in files if not any(glob_match(p, g) for g in cfg["scan"].get("exclude") or [])]
    work = tempfile.mkdtemp(prefix="jplag-")
    try:
        for lang, suffixes in jcfg["languages"].items():
            mine = [p for p in files if os.path.splitext(p)[1] in suffixes and os.path.isfile(os.path.join(args.root, p))]
            entry = {"meguri_files": len(mine)}
            summary["languages"][lang] = entry
            if not mine:
                entry["skipped"] = "no Meguri files in this language"
                continue
            new_root = os.path.join(work, lang, "new", "meguri")
            for p in mine:
                os.makedirs(os.path.dirname(os.path.join(new_root, p)), exist_ok=True)
                shutil.copyfile(os.path.join(args.root, p), os.path.join(new_root, p))
            old_root = os.path.join(work, lang, "old")
            for tag in tags:
                n = _export_tag(anki, tag, suffixes, os.path.join(old_root, tag))
                log(f"jplag: {lang}: {tag}: {n} files")
            result = os.path.join(args.out, lang)
            cmd = [
                "java", "-jar", args.jar, "-l", lang, "-M", "RUN", "--csv-export", "--overwrite",
                "--cluster-skip", "-n", "-1", "-p", ",".join(suffixes),
                "-r", result, "-new", os.path.dirname(new_root), "-old", old_root,
            ]
            r = subprocess.run(cmd, capture_output=True, text=True)
            if r.returncode != 0:
                log(r.stdout[-4000:] + r.stderr[-4000:])
                entry["error"] = f"jplag exited with {r.returncode}"
                summary["status"] = "error"
                continue
            entry["report"] = result + ".jplag"
            comparisons = []
            with open(os.path.join(result, "results.csv"), newline="") as f:
                for row in csv.DictReader(f):
                    pair = {row["submissionName1"], row["submissionName2"]}
                    if "new/meguri" not in pair:
                        continue
                    other = (pair - {"new/meguri"}).pop()
                    comparisons.append(
                        {
                            "tag": other.split("/", 1)[-1],
                            "average": float(row["averageSimilarity"]),
                            "max": float(row["maxSimilarity"]),
                        }
                    )
            entry["comparisons"] = sorted(comparisons, key=lambda c: -c["max"])
            top = max((c["max"] for c in comparisons), default=0.0)
            entry["max_similarity"] = top
            if top >= jcfg["warn_similarity"] and summary["status"] == "pass":
                summary["status"] = "warn"
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return _write_summary(summary, args.out)
