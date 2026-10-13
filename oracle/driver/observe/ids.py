# SPDX-License-Identifier: AGPL-3.0-or-later
"""ids [(#nN | #cN, ...)]: real note and card ids by logical id.

Ids come from the frozen clock (milliseconds, +1 on collision), and fuzz seeds
derive from card ids, so scenarios that depend on them observe them here.
"""

from __future__ import annotations

from ..errors import invalid
from ..registry import observation


@observation("ids")
def ids(session, args):
    if not args:
        out = {f"#n{i + 1}": nid for i, nid in enumerate(session.notes)}
        out.update({f"#c{i + 1}": cid for i, cid in enumerate(session.cards)})
        return out
    if not isinstance(args, list):
        raise invalid("ids takes note or card references")
    return {ref: session.resolve(ref, ref[1:2] if isinstance(ref, str) else "") for ref in args}
