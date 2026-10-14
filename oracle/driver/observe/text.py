# SPDX-License-Identifier: AGPL-3.0-or-later
"""text {fns: [name, ...], inputs: [input, ...]}: pure text functions (text area).

Value: {name: [output for each input, in input order]} for every requested
function. The meaning of each function is specs/text/*.md; the request and
value shapes are observations.md (`text`).

The scenario's collection is never read or written. Functions that need Anki's
note-saving path, card generation, media folder or SQLite collation run on one
scratch collection per worker process, created on first use in the worker's
scratch directory and kept until the process ends. Nothing a function does
there can change a later output: every note is new, and every media file is
removed again right after it is written.

  strip_markup, strip_markup_keep_media  StripHtml, normal and media-names mode
  text_line, text_line_keep_media        HtmlToTextLine, without and with media names
  media_refs                             ExtractMediaFiles
  iri_encode, iri_decode                 EncodeIriPaths, DecodeIriPaths
  media_filename                         the name AddMediaFile stores in an empty media
                                         folder; inputs that lowercasing would change
                                         are invalid (lowercasing new names is
                                         media-store behavior, not the name transform)
  sort_field, checksum                   add a note [input, ""]; the sort-field text
                                         (checked against the stored sfld) and csum
  field_empty                            add a note ["x", input] to a note type whose
                                         second template shows only the second field:
                                         empty when that card is not generated
  unicase_compare                        [a, b] pairs: -1, 0 or 1 under COLLATE unicase
"""

from __future__ import annotations

import os
import tempfile

from anki.card_rendering_pb2 import StripHtmlRequest
from anki.collection import Collection

from ..errors import DriverError, invalid
from ..registry import observation

STRING_FNS = (
    "strip_markup",
    "strip_markup_keep_media",
    "text_line",
    "text_line_keep_media",
    "media_refs",
    "iri_encode",
    "iri_decode",
    "media_filename",
    "sort_field",
    "checksum",
    "field_empty",
)
PAIR_FNS = ("unicase_compare",)
FNS = STRING_FNS + PAIR_FNS

_DEFAULT_DECK = 1


class _Scratch:
    """The worker's scratch collection: note type T with fields A and B and
    templates {{A}} and {{B}}, the collection's default settings."""

    def __init__(self, root: str) -> None:
        self.dir = tempfile.mkdtemp(prefix="text-", dir=root)
        self.col = Collection(os.path.join(self.dir, "collection.anki2"))
        models = self.col.models
        nt = models.new("T")
        for name in ("A", "B"):
            models.add_field(nt, models.new_field(name))
        for i, name in enumerate(("A", "B")):
            t = models.new_template(f"T{i + 1}")
            t["qfmt"] = "{{%s}}" % name
            t["afmt"] = "{{%s}}" % name
            models.add_template(nt, t)
        self.notetype = models.get(models.add_dict(nt).id)
        self.written = 0

    def add(self, first: str, second: str) -> int:
        note = self.col.new_note(self.notetype)
        note.fields[0] = first
        note.fields[1] = second
        self.col.add_note(note, _DEFAULT_DECK)
        return note.id


_scratch: _Scratch | None = None


def _get_scratch(session) -> _Scratch:
    global _scratch
    if _scratch is None:
        _scratch = _Scratch(os.path.dirname(session.dir))
    return _scratch


def _strip(col, text: str, keep_media: bool) -> str:
    mode = StripHtmlRequest.PRESERVE_MEDIA_FILENAMES if keep_media else StripHtmlRequest.NORMAL
    return col._backend.strip_html(text=text, mode=mode)


def _sort_field_and_checksum(scratch: _Scratch, text: str) -> tuple[str, int]:
    col = scratch.col
    nid = scratch.add(text, "")
    saved = col.get_note(nid).fields[0]
    sfld = _strip(col, saved, keep_media=True)
    # The text is what the stored sfld was bound from: equal under the
    # column's affinity (an integer-looking text compares as its number).
    if col.db.scalar("select count() from notes where id = ? and sfld = ?", nid, sfld) != 1:
        raise DriverError("internal_error", detail="sort-field text does not match the stored sfld")
    return sfld, col.db.scalar("select csum from notes where id = ?", nid)


def _field_is_empty(scratch: _Scratch, text: str) -> bool:
    nid = scratch.add("x", text)
    cards = scratch.col.db.scalar("select count() from cards where nid = ?", nid)
    return cards == 1


def _media_filename(scratch: _Scratch, text: str) -> str:
    if text.lower() != text:
        raise invalid("media_filename inputs must not change when lowercased")
    scratch.written += 1
    name = scratch.col.media.write_data(text, b"meguri-text-%d" % scratch.written)
    path = os.path.join(scratch.col.media.dir(), name)
    if os.path.isfile(path):
        os.remove(path)
    return name


def _unicase_compare(scratch: _Scratch, pair: list) -> int:
    a, b = pair
    return scratch.col.db.scalar(
        "select case when ? = ? collate unicase then 0 when ? < ? collate unicase then -1 else 1 end",
        a, b, a, b,
    )


def _check(args) -> tuple[list[str], list]:
    if not isinstance(args, dict) or set(args) != {"fns", "inputs"}:
        raise invalid("text takes {fns, inputs}")
    fns, inputs = args["fns"], args["inputs"]
    if not isinstance(fns, list) or not fns or not all(isinstance(f, str) for f in fns):
        raise invalid("text fns is a non-empty list of function names")
    unknown = sorted(set(fns) - set(FNS))
    if unknown:
        raise invalid(f"unknown text functions: {', '.join(unknown)}")
    if len(set(fns)) != len(fns):
        raise invalid("a text function is listed twice")
    if not isinstance(inputs, list):
        raise invalid("text inputs is a list")
    pairs = [f for f in fns if f in PAIR_FNS]
    if pairs and len(pairs) != len(fns):
        raise invalid("unicase_compare takes pairs and cannot share a request with string functions")
    if pairs:
        ok = all(isinstance(p, list) and len(p) == 2 and all(isinstance(s, str) for s in p) for p in inputs)
        if not ok:
            raise invalid("unicase_compare inputs are [string, string] pairs")
    elif not all(isinstance(s, str) for s in inputs):
        raise invalid("text inputs are strings")
    return fns, inputs


@observation("text")
def text(session, args):
    fns, inputs = _check(args)
    scratch = _get_scratch(session)
    col = scratch.col
    backend = col._backend
    saved: dict[str, tuple[str, int]] = {}

    def sort_and_sum(s: str) -> tuple[str, int]:
        if s not in saved:
            saved[s] = _sort_field_and_checksum(scratch, s)
        return saved[s]

    impl = {
        "strip_markup": lambda s: _strip(col, s, keep_media=False),
        "strip_markup_keep_media": lambda s: _strip(col, s, keep_media=True),
        "text_line": lambda s: backend.html_to_text_line(text=s, preserve_media_filenames=False),
        "text_line_keep_media": lambda s: backend.html_to_text_line(text=s, preserve_media_filenames=True),
        "media_refs": lambda s: list(backend.extract_media_files(s)),
        "iri_encode": lambda s: backend.encode_iri_paths(s),
        "iri_decode": lambda s: backend.decode_iri_paths(s),
        "media_filename": lambda s: _media_filename(scratch, s),
        "sort_field": lambda s: sort_and_sum(s)[0],
        "checksum": lambda s: sort_and_sum(s)[1],
        "field_empty": lambda s: _field_is_empty(scratch, s),
        "unicase_compare": lambda p: _unicase_compare(scratch, p),
    }
    return {fn: [impl[fn](x) for x in inputs] for fn in fns}
