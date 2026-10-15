# SPDX-License-Identifier: AGPL-3.0-or-later
"""Overlay goldens of the macOS native lane (float-determinism.md F7, F8).

Used by the harness only (record-apple, compare-apple), never by a worker:

- scalar_diff: the differing scalars of a Linux golden and an Apple run, in
  pointer order, or the first structural difference (F7.2 steps 3-5);
- the fp-libm-boundary class (F8): which pointers are next-state days or
  memory values, the unrounded interval of a day entry (F8.2 rule 3), the
  rounding-boundary test (rule 4) and the class of every entry;
- overlay files: their document, freshness (F7.3) and application (F9 step 2).

Pure Python: the pinned anki package is not imported here.
"""

from __future__ import annotations

import hashlib
import math
import re
from typing import Any

from . import fsrs_model
from .canon import jcs
from .numbers import to_f32

RATINGS = ("again", "hard", "good", "easy")
DAY_COLUMNS = {0: "again", 3: "hard", 6: "good", 9: "easy"}
MEMORY_COLUMNS = {1: "again", 2: "again", 4: "hard", 5: "hard", 7: "good", 8: "good", 10: "easy", 11: "easy"}
STABILITY_COLUMNS = frozenset({1, 4, 7, 10})
# A rating's day entry is in the class through the button order when the
# rating before it has one in the class (sched/fsrs-transitions T5).
FOLLOWS = {"good": "hard", "easy": "good"}

DAY_GAP = 1
UNROUNDED_TOLERANCE = 1e-5
BOUNDARY_TOLERANCE = 1e-5
# F8.3: a stability within max(relative 1e-4, 1e-5 days), a difficulty
# within relative 1e-5.
STABILITY_RELATIVE = 1e-4
STABILITY_FLOOR = 1e-5
DIFFICULTY_RELATIVE = 1e-5
RECOMPUTE_TOLERANCE = 1e-9

# 0.9 as the f32 the crate uses (F8.2 rule 3), widened.
REFERENCE_RETENTION = to_f32(0.9)
# sched/fuzz Z3: (start, end, factor), the factors the f32 values nearest
# 0.15, 0.1 and 0.05, evaluated here in 64-bit float.
FUZZ_RANGES = ((2.5, 7.0, to_f32(0.15)), (7.0, 20.0, to_f32(0.1)), (20.0, 3.4028234663852886e38, to_f32(0.05)))
FUZZ_START = 2.5

_NEXT_STATES = re.compile(r"^(/steps/(?:0|[1-9][0-9]*)/next_states/#c[1-9][0-9]*)/(again|hard|good|easy)/(.+)$")
_FSRS_NEXT = re.compile(r"^(/steps/(?:0|[1-9][0-9]*)/fsrs_next/rows/(?:0|[1-9][0-9]*))/(0|[1-9][0-9]*)$")
_DAY_TAILS = ("scheduled_days", "review/scheduled_days")
_MEMORY_TAILS = tuple(
    f"{prefix}memory_state/{member}"
    for prefix in ("", "learning/", "review/")
    for member in ("stability", "difficulty")
)

ABSENT = object()


# --- pointers ------------------------------------------------------------------


def pointer(path: list) -> str:
    return "".join("/" + str(p).replace("~", "~0").replace("/", "~1") for p in path)


def _tokens(ptr: str) -> list[str]:
    if not ptr.startswith("/"):
        raise ValueError(f"{ptr!r} is not a JSON pointer")
    return [t.replace("~1", "/").replace("~0", "~") for t in ptr[1:].split("/")]


def resolve(value: Any, ptr: str) -> Any:
    """The value at ptr, or ABSENT."""
    for token in _tokens(ptr):
        if isinstance(value, dict):
            if token not in value:
                return ABSENT
            value = value[token]
        elif isinstance(value, list):
            if not re.fullmatch(r"0|[1-9][0-9]*", token) or int(token) >= len(value):
                return ABSENT
            value = value[int(token)]
        else:
            return ABSENT
    return value


def _set_at(value: Any, ptr: str, new: Any) -> None:
    *parents, last = _tokens(ptr)
    for token in parents:
        value = value[int(token)] if isinstance(value, list) else value[token]
    if isinstance(value, list):
        value[int(last)] = new
    else:
        value[last] = new


def is_scalar(v: Any) -> bool:
    return v is None or isinstance(v, (bool, int, float, str))


def same_scalar(a: Any, b: Any) -> bool:
    """Equal JSON values, number kind included (1 and 1.0 differ)."""
    return type(a) is type(b) and a == b


# --- F7.2: value-by-value comparison -------------------------------------------


class Structural(Exception):
    """A member or item on one side only, or a scalar against a container."""

    def __init__(self, ptr: str) -> None:
        super().__init__(ptr)
        self.pointer = ptr


def scalar_diff(linux: Any, apple: Any, path: list | None = None, out: list | None = None) -> list[dict]:
    """[{pointer, linux, apple}] for every differing scalar, in pointer order
    (members by key, items by index); Structural on a structural difference."""
    path = [] if path is None else path
    out = [] if out is None else out
    if isinstance(linux, dict) and isinstance(apple, dict):
        if set(linux) != set(apple):
            missing = sorted(set(linux) ^ set(apple))[0]
            raise Structural(pointer(path + [missing]))
        for k in sorted(linux):
            scalar_diff(linux[k], apple[k], path + [k], out)
    elif isinstance(linux, list) and isinstance(apple, list):
        if len(linux) != len(apple):
            raise Structural(pointer(path + [min(len(linux), len(apple))]))
        for i, (a, b) in enumerate(zip(linux, apple)):
            scalar_diff(a, b, path + [i], out)
    elif is_scalar(linux) and is_scalar(apple):
        if not same_scalar(linux, apple):
            out.append({"pointer": pointer(path) or "/", "linux": linux, "apple": apple})
    else:
        raise Structural(pointer(path) or "/")
    return out


# --- F8: the fp-libm-boundary class ---------------------------------------------


def classify(ptr: str) -> tuple[str, str, str, str] | None:
    """(kind, set pointer, rating, detail) of a next-state day or memory
    pointer (F8.1); None for every other pointer. A day entry's detail is the
    pointer of the sibling stability its unrounded interval comes from; a
    memory entry's is its member, "stability" or "difficulty"."""
    m = _NEXT_STATES.match(ptr)
    if m:
        set_ptr, rating, tail = m.groups()
        if tail in _DAY_TAILS:
            stability = f"{set_ptr}/{rating}/{tail.removesuffix('scheduled_days')}memory_state/stability"
            return "day", set_ptr, rating, stability
        if tail in _MEMORY_TAILS:
            return "memory", set_ptr, rating, tail.rsplit("/", 1)[1]
        return None
    m = _FSRS_NEXT.match(ptr)
    if m:
        set_ptr, column = m.group(1), int(m.group(2))
        if column in DAY_COLUMNS:
            return "day", set_ptr, DAY_COLUMNS[column], f"{set_ptr}/{column + 1}"
        if column in MEMORY_COLUMNS:
            member = "stability" if column in STABILITY_COLUMNS else "difficulty"
            return "memory", set_ptr, MEMORY_COLUMNS[column], member
    return None


def fuzz_delta(x: float) -> float:
    """sched/fuzz Z3 in 64-bit float."""
    if x < FUZZ_START:
        return 0.0
    delta = 1.0
    for start, end, factor in FUZZ_RANGES:
        delta += factor * max(0.0, min(x, end) - start)
    return delta


def unrounded_interval(stability: float, retention: float, decay: float) -> float:
    """F8.2 rule 3: S (r^(-1/w) - 1) / (n^(-1/w) - 1) in 64-bit float, every
    input the f32 the model used, widened."""
    s, r, w = to_f32(stability), to_f32(retention), to_f32(decay)
    return s * (r ** (-1.0 / w) - 1.0) / (REFERENCE_RETENTION ** (-1.0 / w) - 1.0)


def near_half(p: float) -> bool:
    """p within 1e-5 x max(1, p) of a half-integer."""
    return abs(p - (math.floor(p) + 0.5)) <= BOUNDARY_TOLERANCE * max(1.0, p)


def at_boundary(u_linux: float, u_apple: float) -> bool:
    """F8.2 rule 4, first part: some point of B(u_L) or B(u_A) near a half."""
    for u in (u_linux, u_apple):
        d = fuzz_delta(u)
        if any(near_half(p) for p in (u, u - d, u + d)):
            return True
    return False


def _is_float(v: Any) -> bool:
    return isinstance(v, float) and not isinstance(v, bool)


def _is_int(v: Any) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def memory_in_class(entry: dict, member: str) -> bool:
    """F8.3, for a memory entry whose member is "stability" or "difficulty"."""
    lin, app = entry["linux"], entry["apple"]
    if not (_is_float(lin) and _is_float(app)) or lin == app:
        return False
    m = max(abs(lin), abs(app))
    if member == "stability":
        return abs(app - lin) <= max(STABILITY_RELATIVE * m, STABILITY_FLOOR)
    return abs(app - lin) <= DIFFICULTY_RELATIVE * m


def day_rules_1_2(entry: dict) -> bool:
    """F8.2 rules 1 and 2."""
    lin, app = entry["linux"], entry["apple"]
    if not (_is_int(lin) and _is_int(app)) or abs(lin - app) != DAY_GAP:
        return False
    u = entry.get("unrounded")
    if not isinstance(u, dict):
        return False
    u_l, u_a = u.get("linux"), u.get("apple")
    if not all(isinstance(x, (int, float)) and not isinstance(x, bool) and math.isfinite(x) and x > 0 for x in (u_l, u_a)):
        return False
    return abs(u_a - u_l) <= UNROUNDED_TOLERANCE * u_l


MEMORY_REASONS = {
    "stability": "stabilities differ by more than relative 1e-4 and by more than 1e-5, or are not both floats",
    "difficulty": "difficulties differ by more than relative 1e-5, or are not both floats",
}


def outside_class(entries: list[dict]) -> list[dict]:
    """The entries of one overlay outside the fp-libm-boundary class (F8),
    each with a reason. Day entries must carry `unrounded` as recorded."""
    out = []
    in_class_days: set[tuple[str, str]] = set()
    pending: list[tuple[dict, str, str]] = []
    for entry in entries:
        kind_info = classify(entry["pointer"])
        if kind_info is None:
            out.append({**entry, "reason": "not a next-state day or memory value"})
            continue
        kind, set_ptr, rating, detail = kind_info
        if kind == "memory":
            if not memory_in_class(entry, detail):
                out.append({**entry, "reason": MEMORY_REASONS[detail]})
            continue
        if not day_rules_1_2(entry):
            out.append({**entry, "reason": "days differ by other than 1, or the unrounded intervals are missing or apart"})
            continue
        u = entry["unrounded"]
        if at_boundary(u["linux"], u["apple"]):
            in_class_days.add((set_ptr, rating))
        else:
            pending.append((entry, set_ptr, rating))
    # The button order: Good follows Hard, Easy follows Good.
    for rating in ("hard", "good", "easy"):
        for entry, set_ptr, r in pending:
            if r == rating and FOLLOWS.get(r) and (set_ptr, FOLLOWS[r]) in in_class_days:
                in_class_days.add((set_ptr, r))
    for entry, set_ptr, rating in pending:
        if (set_ptr, rating) not in in_class_days:
            out.append({**entry, "reason": "no rounding boundary explains the one-day difference"})
    return sorted(out, key=lambda e: _pointer_key(e["pointer"]))


def _pointer_key(ptr: str) -> list:
    return [(0, int(t), "") if t.isdigit() else (1, 0, t) for t in _tokens(ptr)]


# --- unrounded intervals of day entries (F7.2 step 6) ---------------------------


class FsrsInputs:
    """Desired retention and decay per next-state set of one scenario run."""

    def __init__(self, scenario: dict, fsrs_context: dict | None) -> None:
        self.scenario = scenario
        self.context = fsrs_context or {}

    def retention_and_decay(self, set_ptr: str) -> tuple[float, float] | None:
        m = re.match(r"^/steps/(0|[1-9][0-9]*)/fsrs_next/rows/(0|[1-9][0-9]*)$", set_ptr)
        if m is None:
            ctx = self.context.get(set_ptr)
            if not isinstance(ctx, dict):
                return None
            return ctx["desired_retention"], ctx["decay"]
        step, row = int(m.group(1)), int(m.group(2))
        request = fsrs_next_request(self.scenario, step)
        if request is None or row >= len(request["inputs"]):
            return None
        inp = request["inputs"][row]
        return inp["dr"], fsrs_model.decay(request["param_sets"][inp["params"]])


def fsrs_next_request(scenario: dict, step: int) -> dict | None:
    """The fsrs_next request of an observe step of the scenario."""
    steps = scenario.get("steps", [])
    if step >= len(steps) or not isinstance(steps[step], dict) or "observe" not in steps[step]:
        return None
    for spec in steps[step]["observe"]:
        if isinstance(spec, dict) and "fsrs_next" in spec:
            return spec["fsrs_next"]
    return None


def add_unrounded(entries: list[dict], linux_obs: Any, apple_obs: Any, inputs: FsrsInputs) -> None:
    """Day entries get unrounded {linux, apple}, each from that side's
    stability of the same rating (the Apple side's from apple_obs)."""
    for entry in entries:
        info = classify(entry["pointer"])
        if info is None or info[0] != "day":
            continue
        _, set_ptr, _, stability_ptr = info
        context = inputs.retention_and_decay(set_ptr)
        s_l, s_a = resolve(linux_obs, stability_ptr), resolve(apple_obs, stability_ptr)
        if context is None or not all(isinstance(s, (int, float)) and not isinstance(s, bool) for s in (s_l, s_a)):
            continue
        r, w = context
        entry["unrounded"] = {"linux": unrounded_interval(s_l, r, w), "apple": unrounded_interval(s_a, r, w)}


# --- overlay files ----------------------------------------------------------------


def observations_sha256(observations: Any) -> str:
    return hashlib.sha256(jcs(observations).encode("utf-8")).hexdigest()


def document(scenario_id: str, scenario_sha256: str, golden: dict, hello: dict, entries: list[dict]) -> dict:
    """The overlay file's value (F7.1); entry members in the schema's set."""
    keep = ("pointer", "linux", "apple", "unrounded")
    return {
        "scenario_id": scenario_id,
        "scenario_sha256": scenario_sha256,
        "linux_observations_sha256": observations_sha256(golden["observations"]),
        "oracle": {k: hello[k] for k in ("anki_version", "driver_sha256", "lock_sha256", "platform")},
        "entries": [{k: e[k] for k in keep if k in e} for e in entries],
    }


def staleness(overlay: dict, scenario_sha256: str, golden: dict, pin: dict) -> str | None:
    """Why an overlay is stale (F7.3), or None when it is fresh."""
    if overlay.get("scenario_sha256") != scenario_sha256:
        return "its scenario changed"
    if overlay.get("linux_observations_sha256") != observations_sha256(golden.get("observations")):
        return "its Linux golden's observations changed"
    oracle = overlay.get("oracle") or {}
    if oracle.get("anki_version") != pin.get("anki_version"):
        return "it was recorded with another anki version"
    if not pin.get("macos_lock_sha256") or oracle.get("lock_sha256") != pin.get("macos_lock_sha256"):
        return "it was recorded from another macOS lock"
    return None


def apply(observations: Any, entries: list[dict]) -> Any:
    """F9 step 2: the Linux observations with each entry's apple value at its
    pointer; ValueError when an entry does not match them."""
    import copy

    out = copy.deepcopy(observations)
    for entry in entries:
        here = resolve(out, entry["pointer"])
        if here is ABSENT or not is_scalar(here) or not same_scalar(here, entry["linux"]):
            raise ValueError(f"{entry['pointer']} does not hold the entry's linux value")
        _set_at(out, entry["pointer"], entry["apple"])
    return out
