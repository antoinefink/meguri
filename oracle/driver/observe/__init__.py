# SPDX-License-Identifier: AGPL-3.0-or-later
"""Observations, one module per area. Importing registers them.

Every value is plain JSON keyed by logical ids; f32 values are shortest
round-trip f32 decimals; no localized text (R1884, R1887).
"""

from . import (  # noqa: F401
    browser_rows, card_excerpts, card_extractors, cards, cloze, db_dump, decks, diag, fsrs_next, ids, image_occlusion, labels, notes, package_contents, package_listing, queue, render, render_status, schema, sched, search, studied_today, tags, template,
    text, timing, type_answer,
)
