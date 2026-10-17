# SPDX-License-Identifier: AGPL-3.0-or-later
"""render_status [(#cN, ...)]: whether stored cards render for review, as a
status only (specs/conformance/render-ops.md Q4).

Value: {"#cN": {ord, status} | null}, status "ok", "empty" or
"template_error"; null for a card that no longer exists. Without arguments:
every card with a logical id, in logical-id order.

Each card is the backend's review render of an existing card (browser False:
the review templates, never the browser's), partial for Python, as
collection_check.render_card renders it for collection-check K8. A template
error raised by the render is template_error; the backend raises the question
side's error before its blank check and the answer side's after it, so the
status is Q4.2's first match. Otherwise the response's empty flag is empty,
else ok. No full render: no text, length, error kind or side is reported.
A card of a standard note type whose ordinal has no template is the backend's
invalid-input error, an anki_error run error.

The backend caches note types and db_exec does not clear that cache, so the
collection reopens first: cards render from the templates the file holds.
"""

from __future__ import annotations

import anki.errors
from anki.template import TemplateRenderContext

from ..errors import invalid
from ..registry import observation


def _status(card) -> str:
    ctx = TemplateRenderContext.from_existing_card(card, browser=False)
    try:
        partial = ctx._partially_render()
    except anki.errors.TemplateError:
        return "template_error"
    return "empty" if partial.is_empty else "ok"


@observation("render_status")
def render_status(session, args):
    targets = session.card_targets(args)
    refs = [ref for ref, _ in targets]
    if len(set(refs)) != len(refs):
        raise invalid("render_status names a card twice")
    session.reopen()
    out = {}
    for ref, cid in targets:
        if session.col.db.scalar("select count() from cards where id = ?", cid) == 0:
            out[ref] = None
            continue
        card = session.col.get_card(cid)
        out[ref] = {"ord": card.ord, "status": _status(card)}
    return out
