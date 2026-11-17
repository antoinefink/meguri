#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Writes the floating-point probe corpus (specs/conformance/float-determinism.md F2).

    python3 -B -I conformance/corpora/fp-probe/generate.py [--root <checkout>]

Output, relative to the checkout:
  conformance/corpora/fp-probe/inputs.jsonl          100,000 FSRS review-card inputs,
                                                     one JSON object per line (F2.1)
  conformance/corpora/fp-probe/param-sets.json       the parameter sets by name (F2.3)
  conformance/scenarios/fp-probe/corpus/
    next-states-000.json .. next-states-099.json     the same inputs, 1,000 per file, in
                                                     one `fsrs_next` request each (F5)
    fuzz-table.json                                  10,000 (card id, reps) pairs x 20
                                                     intervals in one `fuzz_table` request (F4)

The inputs come from a seeded generator: the same Python version writes the same
files (3.13.1 wrote the committed ones). Never edit the output by hand; regenerate,
then re-record the goldens (`tools/mg oracle record fp-probe`) and the overlays
(`tools/mg oracle record --platform apple fp-probe`) in the same change.

Every f32 value is written as the shortest decimal that both readings of
model/json-numbers N2 (one step to f32, or through the nearest 64-bit float) map to
the same f32; the generator checks this for every value it writes, the parameter
sets included. The generator never computes an expected output: goldens come only
from the oracle (R1878).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
import struct
from fractions import Fraction
from pathlib import Path

SEED = 20261015
FILES, PER_FILE = 100, 1000
FUZZ_PAIRS = 10_000
FUZZ_INTERVALS = [1, 2, 3, 4, 5, 6, 7, 8, 13, 20, 21, 30, 50, 90, 91, 100, 365, 1000, 10000, 36500]
CLOCK = {"start": "2030-01-15T10:00:00", "tz": "UTC"}

# F2.3: parameter sets. `default` is the empty list (the crate's FSRS-6 defaults);
# the others are made-up but plausible optimizer results inside the crate's
# clipping ranges, stored under fsrsParams6 (21), fsrsParams5 (19) and
# fsrsWeights (17).
PARAM_SETS: dict[str, list[str]] = {
    "default": [],
    "fsrs6-a": ["0.1544", "0.8813", "2.9105", "11.2071", "6.8114", "0.7103", "2.6541", "0.0052", "1.6812",
                "0.1801", "0.8724", "1.5932", "0.0712", "0.2891", "1.7524", "0.5103", "2.0542", "0.4817",
                "0.1123", "0.0841", "0.1305"],
    "fsrs6-b": ["0.3105", "1.6012", "3.4407", "9.8016", "7.3402", "0.9105", "3.4012", "0.0201", "1.4307",
                "0.2205", "1.1043", "1.8124", "0.0911", "0.3302", "2.0105", "0.4502", "2.2018", "0.6203",
                "0.0742", "0.1204", "0.3412"],
    "fsrs6-c": ["0.0821", "0.5501", "4.8302", "25.6011", "5.9203", "0.6012", "1.9501", "0.0013", "2.1204",
                "0.1402", "0.7105", "1.2203", "0.0503", "0.2204", "1.3302", "0.6801", "2.8805", "0.3501",
                "0.2402", "0.0405", "0.6003"],
    "fsrs5-a": ["0.3891", "1.2207", "3.3014", "14.9056", "7.1213", "0.5502", "1.1024", "0.0201", "1.5809",
                "0.1611", "1.0457", "1.9102", "0.0899", "0.3108", "2.1503", "0.2602", "2.8104", "0.4703",
                "0.6012"],
    "fsrs4-a": ["0.5512", "1.3904", "4.2051", "11.3107", "5.2209", "1.1503", "0.9102", "0.0405", "1.5903",
                "0.1407", "0.9908", "2.0511", "0.0802", "0.3311", "1.5102", "0.2304", "2.7509"],
}
PARAM_WEIGHTS = {"default": 40, "fsrs6-a": 15, "fsrs6-b": 15, "fsrs6-c": 15, "fsrs5-a": 10, "fsrs4-a": 5}
# The decay each set's model uses (sched/fsrs-transitions T2.1), for realistic
# current intervals only.
DECAY = {"default": 0.1542, "fsrs6-a": 0.1305, "fsrs6-b": 0.3412, "fsrs6-c": 0.6003, "fsrs5-a": 0.5, "fsrs4-a": 0.5}

# The crate's clipping ranges for 21 values (one relearning step, no short-term
# floor), used only to check that the made-up sets need no clipping.
CLIP = [(0.001, 100), (0.001, 100), (0.001, 100), (0.001, 100), (1, 10), (0.001, 4), (0.001, 4),
        (0.001, 0.75), (0, 4.5), (0, 0.8), (0.001, 3.5), (0.001, 5), (0.001, 0.25), (0.001, 0.9),
        (0, 4), (0, 1), (1, 6), (0, 2), (0, 2), (0, 0.8), (0.1, 0.8)]

DR_GRID = [f"0.{n}".rstrip("0") for n in range(70, 100)]  # 0.7, 0.71 .. 0.99
DR_WEIGHTED = {"0.9": 30, "0.85": 10, "0.8": 10, "0.95": 10}
MAX_IVL = [(36500, 75), (3650, 8), (365, 8), (100, 5), (30, 2), (None, 2)]  # None: 1..10

CARD_ID_LOW, CARD_ID_HIGH = 1420070400000, 1893456000000  # 2015-01-01 .. 2030-01-01, ms
MAX_SAFE = 2**53 - 1
REPS_MAX = 4294967294
ELAPSED_MAX = 20000


# --- f32 helpers -----------------------------------------------------------

def f32(x: float) -> float:
    return struct.unpack("<f", struct.pack("<f", x))[0]


def _bits(v: float) -> int:
    return struct.unpack("<I", struct.pack("<f", v))[0]


def _from_bits(b: int) -> float:
    return struct.unpack("<f", struct.pack("<I", b))[0]


def one_step(text: str, v: float) -> bool:
    """True when the exact decimal rounds to the f32 v (nearest, ties to even)."""
    q = Fraction(text)
    b = _bits(v)
    lo = (Fraction(_from_bits(b - 1)) + Fraction(v)) / 2
    hi = (Fraction(v) + Fraction(_from_bits(b + 1))) / 2
    if lo < q < hi:
        return True
    return (q == lo or q == hi) and b % 2 == 0


def f32_text(v: float) -> str:
    """Shortest decimal both N2 readings map to the positive finite f32 v."""
    assert v > 0 and math.isfinite(v) and f32(v) == v
    for p in range(1, 10):
        t = f"{v:.{p}g}"
        if "e" in t:
            t = repr(float(t))
        if f32(float(t)) == v and one_step(t, v):
            if "." not in t and "e" not in t:
                t += ".0"
            return t
    raise AssertionError(f"no safe decimal for {v!r}")


def checked(text: str) -> str:
    """A literal decimal from this file: both readings agree, and it is shortest."""
    v = f32(float(text))
    assert one_step(text, v), text
    assert f32_text(v) == (text if ("." in text or "e" in text) else text + ".0"), (text, f32_text(v))
    return text


# --- inputs ----------------------------------------------------------------

def log_uniform(rng: random.Random, lo: float, hi: float) -> float:
    return math.exp(rng.uniform(math.log(lo), math.log(hi)))


def stored_like(rng: random.Random, x: float, places: int) -> float:
    """80 %: rounded as card data stores it (model/card-data-json C4); 20 %: any f32."""
    if rng.random() < 0.8:
        x = round(x, places)
    return f32(x)


def interval_factor(dr: float, decay: float) -> float:
    return (dr ** (-1 / decay) - 1) / (0.9 ** (-1 / decay) - 1)


def make_input(rng: random.Random, i: int) -> dict[str, object]:
    params = rng.choices(list(PARAM_WEIGHTS), weights=list(PARAM_WEIGHTS.values()))[0]
    if rng.random() < 0.6:
        dr = rng.choices(list(DR_WEIGHTED), weights=list(DR_WEIGHTED.values()))[0]
    else:
        dr = rng.choice(DR_GRID)
    kind, = rng.choices([m for m, _ in MAX_IVL], weights=[w for _, w in MAX_IVL])
    max_ivl = kind if kind is not None else rng.randint(1, 10)

    r = rng.random()
    if r < 0.2:
        s = log_uniform(rng, 0.001, 1.0)
    elif r < 0.9:
        s = log_uniform(rng, 1.0, 3650.0)
    else:
        s = log_uniform(rng, 3650.0, 36500.0)
    s = min(max(stored_like(rng, s, 4), f32(0.001)), 36500.0)

    r = rng.random()
    d = 1.0 if r < 0.03 else 10.0 if r < 0.06 else rng.uniform(1.0, 10.0)
    d = min(max(stored_like(rng, d, 3), 1.0), 10.0)

    if rng.random() < 0.7:
        ivl = s * interval_factor(float(dr), DECAY[params]) * rng.uniform(0.85, 1.15)
        prev = int(min(max(round(ivl), 1), 36500))
    else:
        prev = int(min(max(round(log_uniform(rng, 1, 36500)), 1), 36500))

    r = rng.random()
    if r < 0.05:
        elapsed = 0
    elif r < 0.65:
        elapsed = prev * rng.uniform(0.9, 1.3)
    elif r < 0.80:
        elapsed = prev * rng.uniform(0.0, 0.9)
    elif r < 0.95:
        elapsed = prev * rng.uniform(1.3, 4.0)
    else:
        elapsed = prev * rng.uniform(4.0, 20.0)
    elapsed = int(min(max(round(elapsed), 0), ELAPSED_MAX))

    card_id = rng.randint(1, 10**6) if rng.random() < 0.02 else rng.randint(CARD_ID_LOW, CARD_ID_HIGH - 1)
    r = rng.random()
    reps = 0 if r < 0.01 else rng.randint(10**6, REPS_MAX) if r < 0.03 else int(log_uniform(rng, 1, 500))

    return {
        "card_id": card_id,
        "d": f32_text(d),
        "dr": checked(dr),
        "elapsed_days": elapsed,
        "id": i,
        "max_ivl": max_ivl,
        "params": params,
        "reps": reps,
        "s": f32_text(s),
        "scheduled_days": prev,
    }


def input_line(inp: dict[str, object]) -> str:
    """Compact JSON with sorted keys; f32 members carry their decimal text."""
    parts = []
    for key in sorted(inp):
        value = inp[key]
        text = json.dumps(value) if key == "params" else str(value)
        parts.append(f'"{key}":{text}')
    return "{" + ",".join(parts) + "}"


def param_sets_text(indent: str, close: str) -> str:
    lines = []
    for name in sorted(PARAM_SETS):
        values = ", ".join(checked(t) for t in PARAM_SETS[name])
        lines.append(f'{indent}"{name}": [{values}]')
    return "{\n" + ",\n".join(lines) + "\n" + close + "}"


def next_states_scenario(k: int, inputs: list[dict[str, object]]) -> str:
    first, last = inputs[0]["id"], inputs[-1]["id"]
    head = {
        "clock": CLOCK,
        "description": f"Generated FSRS review-card inputs {first} to {last} of the fp-probe corpus "
                       "(corpora/fp-probe/generate.py).",
        "id": f"fp-probe/corpus/next-states-{k:03d}",
        "tags": ["generated", "clock-free"],
    }
    text = "{\n"
    for key in ("clock", "description", "id"):
        text += f" {json.dumps(key)}: {json.dumps(head[key], sort_keys=True)},\n"
    text += ' "steps": [\n  {"observe": [{"fsrs_next": {\n   "inputs": [\n'
    text += ",\n".join(input_line(inp) for inp in inputs)
    text += "\n   ],\n   \"param_sets\": " + param_sets_text("    ", "   ") + "\n  }}]}\n ],\n"
    text += f' "tags": {json.dumps(head["tags"])}\n}}\n'
    json.loads(text)  # the layout must stay valid JSON
    return text


def fuzz_pairs(rng: random.Random) -> list[tuple[int, int]]:
    seen: set[tuple[int, int]] = set()
    pairs = []
    while len(pairs) < FUZZ_PAIRS:
        r = rng.random()
        if r < 0.80:
            cid = rng.randint(CARD_ID_LOW, CARD_ID_HIGH - 1)
        elif r < 0.90:
            cid = rng.randint(1, 10**6)
        elif r < 0.95:
            cid = rng.randint(MAX_SAFE - 10**6, MAX_SAFE)
        else:
            cid = rng.randint(1, MAX_SAFE)
        r = rng.random()
        reps = rng.randint(0, 200) if r < 0.7 else rng.randint(201, 10**5) if r < 0.9 else rng.randint(10**5, REPS_MAX)
        if (cid, reps) not in seen:
            seen.add((cid, reps))
            pairs.append((cid, reps))
    return pairs


def fuzz_scenario(pairs: list[tuple[int, int]]) -> str:
    setup = {
        "decks": [{"name": "Words", "preset": "Long"}],
        "notes": [{"deck": "Words", "fields": ["sun", "sol"], "notetype": "Pair"}],
        "notetypes": [{"fields": ["Front", "Back"], "name": "Pair",
                       "templates": [{"back": "{{Back}}", "front": "{{Front}}", "name": "Forward"}]}],
        "presets": [{"max_interval_days": 36500, "name": "Long"}],
    }
    text = "{\n"
    text += f' "clock": {json.dumps(CLOCK, sort_keys=True)},\n'
    text += (' "description": "Generated fuzz table: 10,000 (card id, reps) pairs at 20 intervals, '
             'maximum interval 36,500 (corpora/fp-probe/generate.py).",\n')
    text += ' "id": "fp-probe/corpus/fuzz-table",\n'
    text += f' "setup": {json.dumps(setup, sort_keys=True)},\n'
    text += ' "steps": [\n  {"observe": [{"fuzz_table": {\n   "card": "#c1",\n'
    text += f'   "intervals": {json.dumps(FUZZ_INTERVALS)},\n   "pairs": [\n'
    text += ",\n".join(f"[{c},{r}]" for c, r in pairs)
    text += '\n   ]\n  }}]}\n ],\n "tags": ["generated", "clock-free"]\n}\n'
    json.loads(text)
    return text


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=str(Path(__file__).resolve().parents[3]))
    root = Path(ap.parse_args().root)

    for name, values in PARAM_SETS.items():
        assert len(values) in (0, 17, 19, 21), name
        if len(values) == 21:
            for w, (lo, hi) in zip((float(t) for t in values), CLIP):
                assert lo <= w <= hi, (name, w)

    rng = random.Random(SEED)
    inputs = [make_input(rng, i) for i in range(FILES * PER_FILE)]
    pairs = fuzz_pairs(rng)

    corpus = root / "conformance/corpora/fp-probe/inputs.jsonl"
    corpus.write_text("".join(input_line(inp) + "\n" for inp in inputs), encoding="utf-8")
    sets = root / "conformance/corpora/fp-probe/param-sets.json"
    sets.write_text(param_sets_text("  ", "") + "\n", encoding="utf-8")
    json.loads(sets.read_text(encoding="utf-8"))

    out = root / "conformance/scenarios/fp-probe/corpus"
    out.mkdir(parents=True, exist_ok=True)
    for old in out.glob("*.json"):
        old.unlink()
    for k in range(FILES):
        chunk = inputs[k * PER_FILE:(k + 1) * PER_FILE]
        (out / f"next-states-{k:03d}.json").write_text(next_states_scenario(k, chunk), encoding="utf-8")
    (out / "fuzz-table.json").write_text(fuzz_scenario(pairs), encoding="utf-8")

    digest = hashlib.sha256(corpus.read_bytes()).hexdigest()
    print(f"inputs.jsonl: {len(inputs)} inputs, sha256 {digest}")
    print(f"scenarios: {FILES} next-states files, fuzz-table with {len(pairs)} pairs")


if __name__ == "__main__":
    main()
