# SPDX-License-Identifier: AGPL-3.0-or-later
"""What the FSRS model of a preset uses, for conformance tooling only
(sched/fsrs-transitions T2.1, T2.2; float-determinism.md F7.2, F8.2).

Pure Python, no anki import: the overlay recorder uses it too.
"""

from __future__ import annotations

from .numbers import to_f32

DEFAULT_DECAY = 0.1542  # the empty list: the crate's FSRS-6 defaults
OLD_DECAY = 0.5  # 17 or 19 values (FSRS-4.5 and FSRS-5)
DECAY_RANGE = (0.1, 0.8)  # the crate clips the 21st value to it


def params_key(n: int) -> str | None:
    """The schema-11 preset key a list of n values is stored under; None for
    the empty list (all three keys empty)."""
    return {0: None, 17: "fsrsWeights", 19: "fsrsParams5", 21: "fsrsParams6"}[n]


def decay(params: list[float]) -> float:
    """The decay the model uses for this parameter list, as an f32 widened."""
    if not params:
        return to_f32(DEFAULT_DECAY)
    if len(params) < 21:
        return to_f32(OLD_DECAY)
    lo, hi = (to_f32(x) for x in DECAY_RANGE)
    return min(max(to_f32(params[20]), lo), hi)
