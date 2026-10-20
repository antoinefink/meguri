# SPDX-License-Identifier: AGPL-3.0-or-later
"""The studied deck's queue, the deck list's counts and a finished deck
(specs/conformance/queue-ops.md Q2-Q4).

queue -> {cards, kinds, counts}: the backend's queued cards of the current deck
  with a fetch limit of max(1, the collection's card count), in the order it
  returns them (learning cards due now, the main queue, then the learning cards
  within the learn-ahead limit); each card as #cN, or its real id as a string
  when no logical id names it; kinds from each entry's queue (new, learning,
  review); counts {new, learning, review} as returned with the cards. The
  backend keeps the queue it built: scenarios read it only where Q2.3 says a
  read shows a fresh build.
deck_counts -> [{name, new, learning, review, children}]: the deck list's call,
  the deck tree with counts at the current instant (the backend runs the day
  query and its unbury check first), the root's children in the tree's order;
  name is the node's last name component, learning its learn count.
congrats -> {learn_remaining, secs_until_next_learn, review_remaining,
  new_remaining, have_sched_buried, have_user_buried}: the backend's
  congratulations info for the current deck, those six fields only.

"""

from __future__ import annotations

from anki import scheduler_pb2

from ..errors import invalid
from ..registry import observation

KINDS = {
    scheduler_pb2.QueuedCards.NEW: "new",
    scheduler_pb2.QueuedCards.LEARNING: "learning",
    scheduler_pb2.QueuedCards.REVIEW: "review",
}


def _no_args(name: str, args) -> None:
    if args not in (None, []):
        raise invalid(f"{name} takes no arguments")


@observation("queue")
def queue(session, args):
    _no_args("queue", args)
    col = session.col
    card_count = col.db.scalar("select count() from cards")
    queued = col.sched.get_queued_cards(fetch_limit=max(1, card_count))
    return {
        "cards": [session.card_ref(entry.card.id) for entry in queued.cards],
        "kinds": [KINDS[entry.queue] for entry in queued.cards],
        "counts": {"new": queued.new_count, "learning": queued.learning_count, "review": queued.review_count},
    }


def _node(node) -> dict:
    return {
        "name": node.name,
        "new": node.new_count,
        "learning": node.learn_count,
        "review": node.review_count,
        "children": [_node(child) for child in node.children],
    }


@observation("deck_counts")
def deck_counts(session, args):
    _no_args("deck_counts", args)
    return [_node(child) for child in session.col.sched.deck_due_tree().children]


@observation("congrats")
def congrats(session, args):
    _no_args("congrats", args)
    info = session.col.sched.congratulations_info()
    return {
        "learn_remaining": info.learn_remaining,
        "secs_until_next_learn": info.secs_until_next_learn,
        "review_remaining": info.review_remaining,
        "new_remaining": info.new_remaining,
        "have_sched_buried": info.have_sched_buried,
        "have_user_buried": info.have_user_buried,
    }

