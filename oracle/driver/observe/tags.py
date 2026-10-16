# SPDX-License-Identifier: AGPL-3.0-or-later
"""tag_tree (specs/conformance/tag-ops.md G9): the collection's tag tree.

The value is the root's children as Anki's tag tree gives them, in its order,
each {name, collapsed, children} with the node's level dropped. Reading the
tree changes nothing.
"""

from __future__ import annotations

from ..errors import invalid
from ..registry import observation


def _node(node) -> dict:
    return {"name": node.name, "collapsed": node.collapsed, "children": [_node(c) for c in node.children]}


@observation("tag_tree")
def tag_tree(session, args):
    if args not in (None, []):
        raise invalid("tag_tree takes no arguments")
    return [_node(child) for child in session.col.tags.tree().children]
