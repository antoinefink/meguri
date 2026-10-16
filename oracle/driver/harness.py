# SPDX-License-Identifier: AGPL-3.0-or-later
"""Worker pool, golden recording, live comparison and bump diff reports.

  python -m driver.harness record  --scenarios DIR --goldens DIR [--filter F]... [--jobs N] [--report FILE]
  python -m driver.harness compare --scenarios DIR --goldens DIR [--filter F]... [--jobs N] [--report FILE]
  python -m driver.harness diff-report --old DIR --new DIR --report FILE [--markdown FILE]
  python -m driver.harness hash FILE...
  python -m driver.harness check --dir DIR --label TEXT --out DIR [--now INSTANT] [--tz ZONE]
                                 [--deck NAME]... [--render-note ID]...
  python -m driver.harness record-apple  --scenarios DIR --goldens DIR --overlays DIR --pin FILE
                                         [--filter F]... [--jobs N] [--report FILE]
  python -m driver.harness compare-apple --scenarios DIR --goldens DIR --overlays DIR --pin FILE
                                         [--filter F]... [--jobs N] [--report FILE]

Runs inside the oracle image without libfaketime (entrypoint.sh harness), or
natively on an arm64 Mac (oracle/macos/run-native.sh harness). Each worker is a
separate `entrypoint.sh worker` (`run-native.sh worker`) process with its own
TZ, clock file and scratch directory, so workers never share a clock, a zone or
a collection (R1871, R1872). The pool is keyed by TZ and sized by --jobs.

`record` is the only writer of goldens: one file per scenario at the same
relative path under --goldens, with a .json suffix. Exit status: 0 when every
selected scenario recorded or matched, 1 otherwise, 2 for usage errors.

`record-apple` and `compare-apple` (native lane only; float-determinism.md
F6.4, F7) need fresh Linux goldens (against --pin, conformance/oracle.json) and
accept only workers that answer macos-arm64, the pinned anki_version and the
pin's macos_lock_sha256. On a worker without faketime they run only scenarios
tagged clock-free and report the others as clock_unsupported. record-apple is
the only writer of --overlays/<id>.json: it compares the native run with the
Linux golden value by value, writes every differing scalar as an entry (day
entries with their unrounded intervals, from the run's fsrs_context or the
scenario's fsrs_next inputs), writes nothing on a structural difference,
deletes an overlay that no longer differs, and lists entries outside the
fp-libm-boundary class (F8), which fail the scenario. compare-apple compares a
native run with the golden plus its overlay, exactly.

`check` (tools/mg oracle check, specs/conformance/collection-check.md K1-K9)
derives the clock and zone from the directory's manifest when they are not
given, starts one worker in that zone and writes the report to <out>/check.json
(exit 0), or {kind, detail} to <out>/check-error.json (exit 1) with a K7 kind.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from . import overlay as ov
from .canon import SCENARIO_SUFFIXES, ScenarioFormatError, golden_text, load_scenario_file, scenario_sha256
from .errors import install_excepthook

ORACLE_HOME = Path(os.environ.get("ORACLE_HOME", Path(__file__).resolve().parent.parent))
# The native lane's launcher takes the same `worker` subcommand.
ENTRYPOINT = ORACLE_HOME / ("macos/run-native.sh" if sys.platform == "darwin" else "entrypoint.sh")
ORACLE_KEYS = ("anki_version", "lock_sha256", "driver_sha256")
NATIVE_PLATFORM = "macos-arm64"
CLOCK_FREE = "clock-free"
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
    golden: dict | None = None  # record-apple, compare-apple: the fresh Linux golden
    overlay: dict | None = None  # compare-apple: the scenario's overlay, when it has one


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


def run_pool(items: list[Item], jobs: int, extra: dict | None = None, accept=None) -> None:
    """Run every prepared item on workers keyed by TZ (in place). extra:
    members added to each run request; accept(hello): None, or the error of
    every item a worker with that hello would have run."""
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
        for fut in [pool.submit(_run_chunk, tz, chunk, extra, accept) for tz, chunk in tasks]:
            fut.result()


def clock_free(scenario: Any) -> bool:
    tags = scenario.get("tags") if isinstance(scenario, dict) else None
    return isinstance(tags, list) and CLOCK_FREE in tags


def _run_chunk(tz: str, chunk: list[Item], extra: dict | None = None, accept=None) -> None:
    worker = None
    try:
        for item in chunk:
            try:
                if worker is None:
                    worker = WorkerProcess(tz)
                item.hello = worker.hello
                refused = accept(worker.hello) if accept else None
                if refused:
                    item.error = refused
                    continue
                if worker.hello.get("time_control") == "none" and not clock_free(item.scenario):
                    # float-determinism.md F6.3: a worker on the real clock runs
                    # only scenarios whose observations hold at any instant.
                    item.error = {
                        "kind": "clock_unsupported",
                        "detail": "the worker runs on the real clock and the scenario is not tagged clock-free",
                    }
                    continue
                started = time.perf_counter()
                item.response = worker.request({"cmd": "run", "id": item.rel, "scenario": item.scenario, **(extra or {})})
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


# --- the macOS native lane (float-determinism.md F6.4, F7) ----------------------


def _load_pin(path: str) -> dict | None:
    try:
        pin = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        print(f"harness: cannot read the pin {path}: {exc}", file=sys.stderr)
        return None
    if not isinstance(pin, dict) or not all(isinstance(pin.get(k), str) for k in ("anki_version", "lock_sha256")):
        print(f"harness: {path} has no anki_version and lock_sha256", file=sys.stderr)
        return None
    return pin


def native_refusal(pin: dict):
    """accept() for run_pool: a native worker must answer macos-arm64, the
    pinned anki_version and the pin's macos_lock_sha256 (F6.2)."""

    def accept(hello: dict) -> dict | None:
        want = {"platform": NATIVE_PLATFORM, "anki_version": pin.get("anki_version"),
                "lock_sha256": pin.get("macos_lock_sha256")}
        got = {k: hello.get(k) for k in want}
        if not pin.get("macos_lock_sha256") or got != want:
            return {"kind": "oracle_mismatch", "detail": f"the worker answers {got}, the pin wants {want}"}
        return None

    return accept


def _fresh_golden(item: Item, goldens: Path, pin: dict) -> None:
    """Sets item.golden, or item.error when the Linux golden is missing,
    unreadable or stale (dsl.md D10): such a scenario is not run."""
    path = goldens / f"{item.rel}.json"
    if not path.exists():
        item.error = {"kind": "missing_golden", "detail": f"{path.as_posix()} does not exist; record it first"}
        return
    try:
        golden = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        item.error = {"kind": "invalid_golden", "detail": str(exc)}
        return
    if not isinstance(golden, dict) or golden.get("scenario_sha256") != item.sha256:
        item.error = {"kind": "stale_golden", "detail": "the golden was recorded for another scenario text"}
    elif (golden.get("oracle") or {}).get("lock_sha256") != pin["lock_sha256"]:
        item.error = {"kind": "stale_golden", "detail": "the golden was recorded with another oracle lock"}
    else:
        item.golden = golden


def _select_apple(args, pin: dict, before_run=None) -> list[Item] | None:
    root = Path(args.scenarios)
    if not root.is_dir():
        print(f"harness: scenario directory {root} does not exist", file=sys.stderr)
        return None
    items = find_scenarios(root, args.filter or [])
    for item in items:
        prepare(item)
        if item.error is None:
            _fresh_golden(item, Path(args.goldens), pin)
        if item.error is None and before_run is not None:
            before_run(item)
    started = time.perf_counter()
    extra = {"fsrs_context": True} if args.command == "record-apple" else None
    run_pool(items, args.jobs, extra=extra, accept=native_refusal(pin))
    args._wall_s = time.perf_counter() - started
    return items


def _not_run(item: Item) -> dict:
    kind = item.error.get("kind") if isinstance(item.error, dict) else None
    status = kind if kind in ("clock_unsupported", "missing_golden", "stale_golden", "stale_overlay", "invalid_overlay") else "error"
    return {"status": status, "error": item.error}


def _entry_counts(entries: list[dict]) -> dict:
    kinds = [(ov.classify(e["pointer"]) or ("other", "", "", ""))[0] for e in entries]
    rows = {ov.classify(e["pointer"])[1] for e in entries if ov.classify(e["pointer"]) and "/fsrs_next/" in e["pointer"]}
    return {
        "entries": len(entries),
        "day_entries": kinds.count("day"),
        "memory_entries": kinds.count("memory"),
        "other_entries": kinds.count("other"),
        "differing_rows": len(rows),
    }


def _fsrs_next_inputs(scenario: Any) -> int:
    steps = scenario.get("steps", []) if isinstance(scenario, dict) else []
    return sum(len(ov.fsrs_next_request(scenario, i)["inputs"]) for i in range(len(steps))
               if ov.fsrs_next_request(scenario, i) is not None)


def cmd_record_apple(args) -> int:
    pin = _load_pin(args.pin)
    if pin is None:
        return 2
    items = _select_apple(args, pin)
    if items is None:
        return 2
    overlays = Path(args.overlays)
    for item in items:
        path = overlays / f"{item.rel}.json"
        if item.error:
            item.result = _not_run(item)
            continue
        linux_obs, apple_obs = item.golden["observations"], item.response["observations"]
        try:
            entries = ov.scalar_diff(linux_obs, apple_obs)
        except ov.Structural as exc:
            # F7.2 step 4: nothing is written, the existing overlay stays.
            item.result = {"status": "structural", "pointer": exc.pointer}
            continue
        base = {"overlay": path.as_posix(), "fsrs_next_inputs": _fsrs_next_inputs(item.scenario)}
        if not entries:
            deleted = path.exists()
            if deleted:
                path.unlink()
            item.result = {"status": "no_difference", **base, "deleted": deleted, **_entry_counts([])}
            continue
        ov.add_unrounded(entries, linux_obs, apple_obs, ov.FsrsInputs(item.scenario, item.response.get("fsrs_context")))
        outside = ov.outside_class(entries)
        doc = ov.document(item.scenario.get("id"), item.sha256, item.golden, item.hello, entries)
        _write_atomic(path, golden_text(doc))
        item.result = {
            "status": "outside_class" if outside else "recorded",
            **base,
            **_entry_counts(entries),
            "outside_class": outside,
            "entries_written": doc["entries"],
        }
    report_extra = {"overlay_totals": _overlay_totals(items)}
    return _report(args, "record-apple", items, ok_statuses={"recorded", "no_difference"}, extra=report_extra)


def _overlay_totals(items: list[Item]) -> dict:
    keys = ("fsrs_next_inputs", "entries", "day_entries", "memory_entries", "other_entries", "differing_rows")
    totals = {k: sum(i.result.get(k, 0) for i in items) for k in keys}
    totals["outside_class"] = sum(len(i.result.get("outside_class", [])) for i in items)
    totals["overlays_written"] = sum(1 for i in items if i.result.get("status") in ("recorded", "outside_class"))
    totals["overlays_deleted"] = sum(1 for i in items if i.result.get("deleted"))
    inputs = totals["fsrs_next_inputs"]
    totals["days_rate"] = totals["day_entries"] / (4 * inputs) if inputs else None
    return totals


def cmd_compare_apple(args) -> int:
    pin = _load_pin(args.pin)
    if pin is None:
        return 2
    overlays = Path(args.overlays)

    def load_overlay(item: Item) -> None:
        path = overlays / f"{item.rel}.json"
        if not path.exists():
            return
        try:
            doc = json.loads(path.read_text(encoding="utf-8"))
            if not isinstance(doc, dict) or not isinstance(doc.get("entries"), list):
                raise ValueError("an overlay is an object with entries")
        except (json.JSONDecodeError, UnicodeDecodeError, ValueError) as exc:
            item.error = {"kind": "invalid_overlay", "detail": str(exc)}
            return
        why = ov.staleness(doc, item.sha256, item.golden, pin)
        if why:
            item.error = {"kind": "stale_overlay", "detail": f"{path.as_posix()}: {why}"}
            return
        item.overlay = doc

    items = _select_apple(args, pin, before_run=load_overlay)
    if items is None:
        return 2
    for item in items:
        if item.error:
            item.result = _not_run(item)
            continue
        expected = item.golden["observations"]
        applied = 0
        if item.overlay is not None:
            try:
                expected = ov.apply(expected, item.overlay["entries"])
            except (ValueError, KeyError, TypeError) as exc:
                item.result = {"status": "invalid_overlay", "error": {"detail": str(exc)}}
                continue
            applied = len(item.overlay["entries"])
        diffs = json_diff(expected, item.response["observations"])
        item.result = {"status": "fail" if diffs else "pass", "overlay_applied": applied, "diff_count": len(diffs)}
        if diffs:
            item.result["first_step"] = _first_step(diffs)
            item.result["diffs"] = diffs[:MAX_DIFFS]
    return _report(args, "compare-apple", items, ok_statuses={"pass"})


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


def _report(args, command: str, items: list[Item], ok_statuses: set[str], extra: dict | None = None) -> int:
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
        **(extra or {}),
        "scenarios": entries,
    }
    if hello and command.endswith("-apple"):
        report["oracle"].update(platform=hello.get("platform"), time_control=hello.get("time_control"))
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


# --- check (specs/conformance/collection-check.md) ------------------------------

CHECK_NOW = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(Z|[+-][0-9]{2}:[0-9]{2})$")
NOTE_ID = re.compile(r"^-?[0-9]+$")
CHECK_ERROR_KINDS = ("usage", "no_collection", "open_failed", "oracle_error")


class CheckUsage(ValueError):
    pass


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def zone_for_offset(offset_secs: Any) -> str:
    """K1: the zone for a manifest's utc_offset_secs. IANA's Etc zones invert
    the sign: one hour east of UTC is Etc/GMT-1."""
    if not _is_int(offset_secs):
        raise CheckUsage("the manifest's utc_offset_secs is not an integer; give --tz")
    if offset_secs == 0:
        return "UTC"
    hours, rest = divmod(offset_secs, 3600)
    if rest or not -12 <= hours <= 14:
        raise CheckUsage(f"utc_offset_secs {offset_secs} is not a whole number of hours from -12 to +14; give --tz")
    return f"Etc/GMT{'-' if hours > 0 else '+'}{abs(hours)}"


def parse_check_now(text: str) -> int:
    """K1: --now as clock.start with an offset or Z, whole seconds."""
    if not CHECK_NOW.match(text):
        raise CheckUsage(f"--now {text!r} is not YYYY-MM-DDTHH:MM:SS followed by Z or +HH:MM")
    try:
        secs = int(datetime.fromisoformat(text).timestamp())
    except ValueError:
        raise CheckUsage(f"--now {text!r} is not a date-time") from None
    if secs < 0:
        raise CheckUsage("--now is before 1970")
    return secs


def parse_note_id(text: str) -> int:
    if not NOTE_ID.match(text) or not -(2**63) <= int(text) < 2**63:
        raise CheckUsage(f"--render-note {text!r} is not a note id in decimal")
    return int(text)


def check_defaults(directory: Path, now: str | None, tz: str | None) -> tuple[int, str]:
    """K1: (epoch seconds, zone) from --now and --tz, or from manifest.json."""
    manifest: dict = {}
    path = directory / "manifest.json"
    if (now is None or tz is None) and path.is_file():
        try:
            manifest = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, json.JSONDecodeError):
            raise CheckUsage("manifest.json is not JSON; give --now and --tz") from None
        if not isinstance(manifest, dict):
            raise CheckUsage("manifest.json is not an object; give --now and --tz")
    if now is not None:
        secs = parse_check_now(now)
    elif _is_int(manifest.get("now_ms")) and manifest["now_ms"] >= 0:
        secs = manifest["now_ms"] // 1000
    else:
        raise CheckUsage("give --now: the directory has no manifest now_ms")
    if tz is not None:
        zone = tz
    elif "utc_offset_secs" in manifest:
        zone = zone_for_offset(manifest["utc_offset_secs"])
    else:
        raise CheckUsage("give --tz: the directory has no manifest utc_offset_secs")
    try:
        ZoneInfo(zone)
    except (ZoneInfoNotFoundError, ValueError):
        raise CheckUsage(f"--tz {zone!r} is not in the worker's time zone database") from None
    return secs, zone


def _check_error(out: Path, kind: str, detail: str) -> int:
    _write_atomic(out / "check-error.json", json.dumps({"detail": detail, "kind": kind}, sort_keys=True) + "\n")
    print(f"harness: check {kind}: {detail}", file=sys.stderr)
    return 1


def cmd_check(args) -> int:
    out, directory = Path(args.out), Path(args.dir)
    if not (directory / "collection.anki2").is_file():
        return _check_error(out, "no_collection", f"{args.label}/collection.anki2 does not exist")
    try:
        now, tz = check_defaults(directory, args.now, args.tz)
        notes = [parse_note_id(text) for text in args.render_note or []]
    except CheckUsage as exc:
        return _check_error(out, "usage", str(exc))
    request = {"cmd": "check", "dir": str(directory), "now": now, "decks": args.deck or [], "render_notes": notes}
    try:
        with WorkerProcess(tz) as worker:
            resp = worker.request(request)
            hello = worker.hello
    except WorkerDied as exc:
        return _check_error(out, "oracle_error", str(exc))
    if not resp.get("ok"):
        error = resp.get("error") or {}
        kind = error.get("kind") if error.get("kind") in ("no_collection", "open_failed") else "oracle_error"
        return _check_error(out, kind, json.dumps(error, sort_keys=True))
    report = {"report_version": 1, "oracle": {k: hello[k] for k in ORACLE_KEYS}, "dir": args.label, **resp["check"]}
    _write_atomic(out / "check.json", golden_text(report))
    print(f"harness: check done in {resp.get('elapsed_ms')} ms in the worker", file=sys.stderr)
    return 0


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
    for name in ("record-apple", "compare-apple"):
        sp = sub.add_parser(name)
        sp.add_argument("--scenarios", required=True)
        sp.add_argument("--goldens", required=True)
        sp.add_argument("--overlays", required=True)
        sp.add_argument("--pin", required=True)
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
    c = sub.add_parser("check")
    c.add_argument("--dir", required=True)
    c.add_argument("--label", required=True)
    c.add_argument("--out", required=True)
    c.add_argument("--now")
    c.add_argument("--tz")
    c.add_argument("--deck", action="append")
    c.add_argument("--render-note", action="append")
    args = p.parse_args(argv)
    if getattr(args, "jobs", 1) < 1:
        p.error("--jobs must be at least 1")
    return {
        "record": cmd_record, "compare": cmd_compare, "diff-report": cmd_diff_report, "hash": cmd_hash,
        "check": cmd_check, "record-apple": cmd_record_apple, "compare-apple": cmd_compare_apple,
    }[args.command](args)


if __name__ == "__main__":
    install_excepthook()
    sys.exit(main())
