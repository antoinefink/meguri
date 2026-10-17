# SPDX-License-Identifier: AGPL-3.0-or-later
"""Observations, one module per area. Importing registers them.

Every value is plain JSON keyed by logical ids; f32 values are shortest
round-trip f32 decimals; no localized text (R1884, R1887).
"""

from . import (  # noqa: F401
    cards, cloze, db_dump, decks, diag, fsrs_next, ids, notes, render_status, schema, sched, search, tags, template,
    text, timing,
)
