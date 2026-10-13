# SPDX-License-Identifier: AGPL-3.0-or-later
"""Worker pool, golden recording, live comparison and bump diff reports.

  python -m driver.harness record  --scenarios DIR --goldens DIR [--filter F]... [--jobs N] [--report FILE]
  python -m driver.harness compare --scenarios DIR --goldens DIR [--filter F]... [--jobs N] [--report FILE]
  python -m driver.harness diff-report --old DIR --new DIR --report FILE [--markdown FILE]
  python -m driver.harness hash FILE...

Runs inside the oracle image without libfaketime (entrypoint.sh harness). Each
worker is a separate `entrypoint.sh worker` process with its own TZ, clock file
and scratch directory, so workers never share a clock, a zone or a collection
(R1871, R1872). The pool is keyed by TZ and sized by --jobs.

`record` is the only writer of goldens: one file per scenario at the same
relative path under --goldens, with a .json suffix. Exit status: 0 when every
selected scenario recorded or matched, 1 otherwise, 2 for usage errors.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .canon import SCENARIO_SUFFIXES, ScenarioFormatError, golden_text, load_scenario_file, scenario_sha256

ORACLE_HOME = Path(os.environ.get("ORACLE_HOME", Path(__file__).resolve().parent.parent))
ENTRYPOINT = ORACLE_HOME / "entrypoint.sh"
ORACLE_KEYS = ("anki_version", "lock_sha256", "driver_sha256")
MAX_DIFFS = 20
# Per-directory metadata files of the conformance tree, not scenarios (U4).
NOT_SCENARIOS = {"area.yaml", "area.yml"}


class WorkerDied(RuntimeError):
    pass


class WorkerProcess:
    """One long-lived worker: `entrypoint.sh worker` speaking JSON lines."""

    _seq = 0
    _lock = threading.Lock()

    def __init__(self, tz: str, scratch_root: str | None = None, extra_env: dict | None = None) -> None:
        with WorkerProcess._lock:
            WorkerProcess._seq += 1
            n = WorkerProcess._seq
        root = scratch_root or os.environ.get("ORACLE_SCRATCH_ROOT") or tempfile.gettempdir()
        self.scratch = tempfile.mkdtemp(prefix=f"w{n}-", dir=root)
        env = dict(os.environ, TZ=tz, ORACLE_SCRATCH=self.scratch, **(extra_env or {}))
        self.tz = tz
        self.proc = subprocess.Popen(
            [str(ENTRYPOINT), "worker"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            env=env,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.hello = self.request({"cmd": "hello"})
        if not self.hello.get("ok"):
            raise WorkerDied(f"hello failed: {self.hello}")

    def request(self, req: dict) -> dict:
        try:
            self.proc.stdin.write(json.dumps(req, ensure_ascii=False) + "\n")
            self.proc.stdin.flush()
        except (BrokenPipeError, OSError) as exc:
            raise WorkerDied(f"worker exited ({self.proc.poll()})") from exc
        line = self.proc.stdout.readline()
        if not line:
            raise WorkerDied(f"worker exited ({self.proc.wait()})")
        try:
            return json.loads(line)
        except json.JSONDecodeError as exc:
            raise WorkerDied(f"worker wrote a non-JSON line: {line[:200]!r}") from exc

    def close(self) -> None:
        try:
            if self.proc.stdin:
                self.proc.stdin.close()
            self.proc.wait(timeout=30)
        except (subprocess.TimeoutExpired, OSError):
            self.proc.kill()
            self.proc.wait()
        finally:
            shutil.rmtree(self.scratch, ignore_errors=True)

    def __enter__(self) -> "WorkerProcess":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()


@dataclass
class Item:
    path: Path
    rel: str  # relative path without suffix, e.g. smoke/ids-frozen
    suffix: str
    scenario: Any = None
    sha256: str = ""
    tz: str = ""
    error: dict | None = None
    response: dict | None = None
    hello: dict | None = None
    wall_ms: float = 0.0
    result: dict = field(default_factory=dict)


def find_scenarios(root: Path, filters: list[str]) -> list[Item]:
    items = []
    for p in sorted(root.rglob("*")):
        if not p.is_file() or p.suffix not in SCENARIO_SUFFIXES or p.name in NOT_SCENARIOS:
            continue
        rel_full = p.relative_to(root).as_posix()
        rel = rel_full[: -len(p.suffix)]
        if filters and not any(_matches(f, rel, rel_full) for f in filters):
            continue
        items.append(Item(path=p, rel=rel, suffix=p.suffix))
    return items


def _matches(f: str, rel: str, rel_full: str) -> bool:
    f = f.strip("/")
    for suffix in SCENARIO_SUFFIXES:
        if f.endswith(suffix):
            f = f[: -len(suffix)]
    return rel == f or rel.startswith(f + "/") or fnmatch.fnmatchcase(rel, f) or fnmatch.fnmatchcase(rel_full, f)


def prepare(item: Item) -> None:
    try:
        item.scenario = load_scenario_file(item.path)
        item.sha256 = scenario_sha256(item.scenario)
    except (ScenarioFormatError, OSError, UnicodeDecodeError) as exc:
        item.error = {"kind": "invalid_scenario", "detail": str(exc)}
        return
    clock = item.scenario.get("clock") if isinstance(item.scenario, dict) else None
    tz = clock.get("tz") if isinstance(clock, dict) else None
    if not isinstance(tz, str):
        item.error = {"kind": "invalid_scenario", "detail": "clock.tz is missing"}
        return
    item.tz = tz


def run_pool(items: list[Item], jobs: int) -> None:
    """Run every prepared item on workers keyed by TZ (in place)."""
    runnable = [i for i in items if i.error is None]
    if not runnable:
        return
    by_tz: dict[str, list[Item]] = defaultdict(list)
    for item in runnable:
        by_tz[item.tz].append(item)
    tasks = []
    for tz in sorted(by_tz):
        group = by_tz[tz]
        k = max(1, min(len(group), round(jobs * len(group) / len(runnable))))
        tasks += [(tz, group[i::k]) for i in range(k)]
    with ThreadPoolExecutor(max_workers=max(1, jobs)) as pool:
        for fut in [pool.submit(_run_chunk, tz, chunk) for tz, chunk in tasks]:
            fut.result()


def _run_chunk(tz: str, chunk: list[Item]) -> None:
    worker = None
    try:
        for item in chunk:
            try:
                if worker is None:
                    worker = WorkerProcess(tz)
                started = time.perf_counter()
                item.response = worker.request({"cmd": "run", "id": item.rel, "scenario": item.scenario})
                item.wall_ms = (time.perf_counter() - started) * 1000
                item.hello = worker.hello
                if not item.response.get("ok"):
                    item.error = item.response.get("error", {"kind": "unknown"})
            except WorkerDied as exc:
                item.error = {"kind": "worker_died", "detail": str(exc)}
                if worker is not None:
                    worker.close()
                worker = None
    finally:
        if worker is not None:
            worker.close()


# --- diffs --------------------------------------------------------------------


def _ptr(path: list[str]) -> str:
    return "".join("/" + str(p).replace("~", "~0").replace("/", "~1") for p in path)


# A missing key or list item. Unequal to every JSON value (a string sentinel
# would equal that string); reports show it as "<absent>".
_ABSENT = object()


def _shown(value: Any) -> Any:
    return "<absent>" if value is _ABSENT else value


def json_diff(expected: Any, actual: Any, path: list[str] | None = None, out: list | None = None) -> list[dict]:
    """Exact structural differences as JSON pointers; types must match too."""
    path = path or []
    out = [] if out is None else out
    if isinstance(expected, dict) and isinstance(actual, dict):
        for k in sorted(set(expected) | set(actual)):
            json_diff(expected.get(k, _ABSENT), actual.get(k, _ABSENT), path + [k], out)
    elif isinstance(expected, list) and isinstance(actual, list):
        for i in range(max(len(expected), len(actual))):
            e = expected[i] if i < len(expected) else _ABSENT
            a = actual[i] if i < len(actual) else _ABSENT
            json_diff(e, a, path + [str(i)], out)
    elif type(expected) is not type(actual) or expected != actual:
        out.append({"pointer": _ptr(path) or "/", "expected": _shown(expected), "actual": _shown(actual)})
    return out


def _first_step(diffs: list[dict]) -> int | None:
    for d in diffs:
        parts = d["pointer"].split("/")
        if len(parts) > 2 and parts[1] == "steps" and parts[2].isdigit():
            return int(parts[2])
    return None


# --- commands -------------------------------------------------------------------


def _golden(item: Item) -> dict:
    clock = item.scenario["clock"]
    return {
        "scenario_id": item.scenario.get("id"),
        "scenario_sha256": item.sha256,
        "oracle": {k: item.hello[k] for k in ORACLE_KEYS},
        "tz": item.tz,
        "clock": {"start": clock.get("start"), "mode": clock.get("mode", "frozen")},
        "observations": item.response["observations"],
    }


def _write_atomic(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.tmp")
    tmp.write_text(text, encoding="utf-8", newline="\n")
    os.replace(tmp, path)


def cmd_record(args) -> int:
    items = _select(args)
    if items is None:
        return 2
    goldens = Path(args.goldens)
    for item in items:
        golden = goldens / f"{item.rel}.json"
        if item.error:
            item.result = {"status": "error", "error": item.error}
            continue
        _write_atomic(golden, golden_text(_golden(item)))
        item.result = {"status": "recorded", "golden": golden.as_posix()}
    return _report(args, "record", items, ok_statuses={"recorded"})


def cmd_compare(args) -> int:
    items = _select(args)
    if items is None:
        return 2
    goldens = Path(args.goldens)
    for item in items:
        golden_path = goldens / f"{item.rel}.json"
        if item.error:
            item.result = {"status": "error", "error": item.error}
            continue
        if not golden_path.exists():
            item.result = {"status": "missing", "golden": golden_path.as_posix()}
            continue
        try:
            golden = json.loads(golden_path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError) as exc:
            item.result = {"status": "invalid_golden", "golden": golden_path.as_posix(), "error": {"detail": str(exc)}}
            continue
        if golden.get("scenario_sha256") != item.sha256:
            item.result = {"status": "stale_scenario", "golden": golden_path.as_posix()}
            continue
        diffs = json_diff(golden.get("observations"), item.response["observations"])
        stale_oracle = golden.get("oracle", {}).get("lock_sha256") != item.hello["lock_sha256"]
        status = "stale_oracle" if stale_oracle else ("fail" if diffs else "pass")
        item.result = {"status": status, "golden": golden_path.as_posix(), "diff_count": len(diffs)}
        if diffs:
            item.result["first_step"] = _first_step(diffs)
            item.result["diffs"] = diffs[:MAX_DIFFS]
    return _report(args, "compare", items, ok_statuses={"pass"})


def _select(args) -> list[Item] | None:
    root = Path(args.scenarios)
    if not root.is_dir():
        print(f"harness: scenario directory {root} does not exist", file=sys.stderr)
        return None
    items = find_scenarios(root, args.filter or [])
    for item in items:
        prepare(item)
    started = time.perf_counter()
    run_pool(items, args.jobs)
    args._wall_s = time.perf_counter() - started
    return items


def _report(args, command: str, items: list[Item], ok_statuses: set[str]) -> int:
    totals: dict[str, int] = defaultdict(int)
    hello = next((i.hello for i in items if i.hello), None)
    entries = []
    worker_ms = []
    for item in items:
        status = item.result["status"]
        totals[status] += 1
        entry = {"scenario": item.rel, "path": item.path.as_posix(), "tz": item.tz, **item.result}
        if item.response and item.response.get("ok"):
            entry["worker_ms"] = item.response.get("elapsed_ms")
            entry["wall_ms"] = round(item.wall_ms, 3)
            worker_ms.append(item.response.get("elapsed_ms", 0))
        entries.append(entry)
        print(f"harness: {command} {status:<14} {item.rel}", file=sys.stderr)
    report = {
        "command": command,
        "oracle": {k: hello[k] for k in ORACLE_KEYS} if hello else None,
        "jobs": args.jobs,
        "totals": dict(sorted(totals.items())),
        "scenario_count": len(items),
        "wall_s": round(getattr(args, "_wall_s", 0.0), 3),
        "mean_worker_ms": round(sum(worker_ms) / len(worker_ms), 3) if worker_ms else None,
        "scenarios": entries,
    }
    if args.report:
        _write_atomic(Path(args.report), json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    else:
        print(json.dumps(report, indent=2, ensure_ascii=False))
    return 0 if items and all(i.result["status"] in ok_statuses for i in items) else 1


def cmd_diff_report(args) -> int:
    old_root, new_root = Path(args.old), Path(args.new)
    old_files = {p.relative_to(old_root).as_posix() for p in old_root.rglob("*.json")} if old_root.is_dir() else set()
    new_files = {p.relative_to(new_root).as_posix() for p in new_root.rglob("*.json")} if new_root.is_dir() else set()
    by_group: dict[tuple[str, str], int] = defaultdict(int)
    scenarios = []
    oracles = {"old": None, "new": None}
    for rel in sorted(old_files | new_files):
        area = rel.split("/", 1)[0] if "/" in rel else "."
        if rel not in new_files:
            scenarios.append({"golden": rel, "area": area, "status": "not_recorded"})
            continue
        new = json.loads((new_root / rel).read_text(encoding="utf-8"))
        oracles["new"] = oracles["new"] or new.get("oracle")
        if rel not in old_files:
            scenarios.append({"golden": rel, "area": area, "status": "added"})
            continue
        old = json.loads((old_root / rel).read_text(encoding="utf-8"))
        oracles["old"] = oracles["old"] or old.get("oracle")
        diffs = json_diff(old.get("observations"), new.get("observations"))
        if not diffs:
            scenarios.append({"golden": rel, "area": area, "status": "unchanged"})
            continue
        kinds: dict[str, int] = defaultdict(int)
        for d in diffs:
            parts = d["pointer"].split("/")
            kind = parts[3] if len(parts) > 3 and parts[1] == "steps" else "(structure)"
            kinds[kind] += 1
            by_group[(area, kind)] += 1
        scenarios.append(
            {"golden": rel, "area": area, "status": "changed", "kinds": dict(sorted(kinds.items())),
             "diff_count": len(diffs), "diffs": diffs[:MAX_DIFFS]}
        )
    counts: dict[str, int] = defaultdict(int)
    for s in scenarios:
        counts[s["status"]] += 1
    report = {
        "oracle": oracles,
        "totals": dict(sorted(counts.items())),
        "changed_observations": [
            {"area": a, "kind": k, "count": n} for (a, k), n in sorted(by_group.items())
        ],
        "scenarios": scenarios,
    }
    _write_atomic(Path(args.report), json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    if args.markdown:
        _write_atomic(Path(args.markdown), _markdown(report))
    print(
        "harness: diff-report " + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())),
        file=sys.stderr,
    )
    return 0


def _markdown(report: dict) -> str:
    lines = ["# Oracle bump: behavior differences", ""]
    old, new = report["oracle"]["old"] or {}, report["oracle"]["new"] or {}
    lines.append(f"Old oracle: anki {old.get('anki_version')} (lock {str(old.get('lock_sha256'))[:12]})")
    lines.append(f"New oracle: anki {new.get('anki_version')} (lock {str(new.get('lock_sha256'))[:12]})")
    lines.append("")
    lines.append("| Status | Goldens |")
    lines.append("|---|---|")
    lines += [f"| {k} | {v} |" for k, v in report["totals"].items()]
    lines.append("")
    if report["changed_observations"]:
        lines.append("## Changed observations by area and kind")
        lines.append("")
        lines.append("| Area | Observation | Changed values |")
        lines.append("|---|---|---|")
        lines += [f"| {g['area']} | {g['kind']} | {g['count']} |" for g in report["changed_observations"]]
        lines.append("")
        lines.append("## Changed goldens")
        lines.append("")
        for s in report["scenarios"]:
            if s["status"] != "changed":
                continue
            lines.append(f"### {s['golden']} ({s['diff_count']} values)")
            lines.append("")
            for d in s["diffs"]:
                lines.append(f"- `{d['pointer']}`: {json.dumps(d['expected'])} -> {json.dumps(d['actual'])}")
            lines.append("")
    others = [s for s in report["scenarios"] if s["status"] in ("added", "not_recorded")]
    if others:
        lines.append("## Added or not recorded")
        lines.append("")
        lines += [f"- {s['golden']}: {s['status']}" for s in others]
        lines.append("")
    return "\n".join(lines)


def cmd_hash(args) -> int:
    rc = 0
    for f in args.files:
        try:
            print(f"{scenario_sha256(load_scenario_file(Path(f)))}  {f}")
        except (ScenarioFormatError, OSError) as exc:
            print(f"harness: {f}: {exc}", file=sys.stderr)
            rc = 1
    return rc


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="driver.harness")
    sub = p.add_subparsers(dest="command", required=True)
    for name in ("record", "compare"):
        sp = sub.add_parser(name)
        sp.add_argument("--scenarios", required=True)
        sp.add_argument("--goldens", required=True)
        sp.add_argument("--filter", action="append")
        sp.add_argument("--jobs", type=int, default=min(12, os.cpu_count() or 1))
        sp.add_argument("--report")
    dr = sub.add_parser("diff-report")
    dr.add_argument("--old", required=True)
    dr.add_argument("--new", required=True)
    dr.add_argument("--report", required=True)
    dr.add_argument("--markdown")
    h = sub.add_parser("hash")
    h.add_argument("files", nargs="+")
    args = p.parse_args(argv)
    if getattr(args, "jobs", 1) < 1:
        p.error("--jobs must be at least 1")
    return {"record": cmd_record, "compare": cmd_compare, "diff-report": cmd_diff_report, "hash": cmd_hash}[
        args.command
    ](args)


if __name__ == "__main__":
    sys.exit(main())
