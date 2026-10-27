# SPDX-License-Identifier: AGPL-3.0-or-later
"""Check Media and the two actions on its report (specs/conformance/
media-ops.md F14).

check_media (op): {} -> {unused, missing, missing_notes, renamed, dirs,
    oversize, extracted, trash_count, field_templates}
delete_unused_media (op): {} -> {count}
tag_missing_media (op): {} -> {count}

check_media is Anki's Check Media call. Its response gives unused, missing
and the notes with missing media as values; renamed, dirs, oversize, the
extracted count, the trash count and the field-referencing templates exist
only in the report text, in the backend's language. The driver reads that
text line by line against the backend's own translations, rendered with
sentinel arguments (the U+2068/U+2069 marks Anki strips from the file lines
are stripped from the patterns too), as ops/maintenance.py reads Check
Database. A line that matches nothing it expects, a list whose length differs
from its count line, a template it cannot place, or a rename the folder does
not show after the run (new name absent, old name present) is a run error,
never a guess. A renamed line splits at its last separator: an old name may
hold '>' (Anki renames it for that), a new name never does. The text never
leaves the driver (R1887).

Lists are sorted by UTF-8 bytes, renamed is an object, missing_notes are in
ascending note id order as logical ids (the real id when a note has none).
field_templates are in C9's order of maintenance/check-media.md (note type
name under unicase, then bytes; template ordinal; front before back): Anki
lists them in its note type cache's order.

The latest output (its unused names and missing note ids) is kept in the
actor's memory for the two actions; a handoff forgets it, so a scenario check
refuses an action with no check_media since the start or the latest handoff,
and enforces F14.2's rule on the steps between check_media and
delete_unused_media. delete_unused_media trashes the list with Anki's trash
call, 25 names per call in list order, as Anki's dialog does;
tag_missing_media adds the tag with the collection's bulk tag add (one Update
Tag undo step).
"""

from __future__ import annotations

import os
import re
import unicodedata
from typing import Any, Callable

from ..errors import DriverError, invalid
from ..registry import op, scenario_check

KEY = "check_media"
TAG = "missing-media"
BATCH = 25
ISOLATION = str.maketrans("", "", "⁨⁩")
S0, S1, S2 = "", "", ""
SENTINEL = 9876543
MEGS = 4321.5
# Steps F14.2 allows between check_media and delete_unused_media.
BETWEEN = {"observe", "advance", "set_clock", "tag_missing_media"}


def _plain(text: str) -> str:
    return text.translate(ISOLATION)


def _unreadable(what: str) -> DriverError:
    return DriverError("internal_error", detail=f"the Check Media report holds {what} the driver cannot read")


def _pattern(text: str, sentinels: tuple[str, ...], split_last: bool = False) -> re.Pattern:
    """A full-match pattern of a message rendered with sentinel arguments:
    each sentinel becomes a group. Every group but the last is lazy, so a
    split happens at the first separator; with split_last, only the last
    group is lazy, so it happens at the last one."""
    out, rest = "", text
    last = len(sentinels) - 1
    for i, s in enumerate(sentinels):
        parts = rest.split(s)
        if len(parts) != 2:
            raise DriverError("internal_error", detail="a Check Media message does not show its argument once")
        lazy = i == last if split_last else i < last
        out += re.escape(parts[0]) + ("(.+?)" if lazy else "(.+)")
        rest = parts[1]
    return re.compile(out + re.escape(rest))


def _count_matcher(render: Callable[..., str]) -> Callable[[str], "int | None"]:
    one = _plain(render(count=1))
    other = _plain(render(count=SENTINEL)).split(str(SENTINEL))
    if len(other) != 2:
        raise DriverError("internal_error", detail="a Check Media count message does not show its count once")
    pattern = re.compile(re.escape(other[0]) + "([0-9]+)" + re.escape(other[1]))

    def match(line: str) -> int | None:
        if line == one:
            return 1
        m = pattern.fullmatch(line)
        return int(m.group(1)) if m else None

    return match


def _trash_matcher(tr) -> Callable[[str], "int | None"]:
    """The trash line shows the count and the size in MB; only the count is
    read."""
    patterns = []
    for count in (1, SENTINEL):
        text = _plain(tr.media_check_trash_count(count=count, megs=MEGS))
        megs = str(MEGS)
        if text.count(megs) != 1:
            raise DriverError("internal_error", detail="the trash message does not show its size once")
        head, tail = text.split(megs)
        if count == 1:
            patterns.append((re.compile(re.escape(head) + "[0-9.,]+" + re.escape(tail)), 1))
        else:
            parts = head.split(str(SENTINEL))
            if len(parts) != 2:
                raise DriverError("internal_error", detail="the trash message does not show its count once")
            patterns.append((re.compile(re.escape(parts[0]) + "([0-9]+)" + re.escape(parts[1]) + "[0-9.,]+" + re.escape(tail)), None))

    def match(line: str) -> int | None:
        for pattern, fixed in patterns:
            m = pattern.fullmatch(line)
            if m:
                return fixed if fixed is not None else int(m.group(1))
        return None

    return match


def parse_report(tr, report: str) -> dict:
    """The report's counts, lists and templates."""
    header = tr.media_check_template_references_field_header()
    templates: list[tuple[str, str, str]] = []
    marker = "\n" + header + "\n"
    at = report.find(marker)
    if at >= 0:
        tail, report = report[at + len(marker):], report[:at + 1]
        line_pattern = _pattern(_plain(tr.media_check_notetype_template(notetype=S0, card_type=S1, side=S2)), (S0, S1, S2))
        sides = {_plain(tr.card_templates_front_template()): "front", _plain(tr.card_templates_back_template()): "back"}
        for line in tail.split("\n"):
            if line == "":
                continue
            m = line_pattern.fullmatch(line)
            if not m or m.group(3) not in sides:
                raise _unreadable("a template line")
            templates.append((m.group(1), m.group(2), sides[m.group(3)]))

    counts = {
        "missing": _count_matcher(tr.media_check_missing_count),
        "unused": _count_matcher(tr.media_check_unused_count),
        "extracted": _count_matcher(tr.media_check_extracted_count),
        "renamed": _count_matcher(tr.media_check_renamed_count),
        "oversize": _count_matcher(tr.media_check_oversize_count),
        "dirs": _count_matcher(tr.media_check_subfolder_count),
        "trash": _trash_matcher(tr),
    }
    headers = {
        _plain(tr.media_check_renamed_header()): "renamed",
        _plain(tr.media_check_oversize_header()): "oversize",
        _plain(tr.media_check_subfolder_header()): "dirs",
        _plain(tr.media_check_missing_header()): "missing",
        _plain(tr.media_check_unused_header()): "unused",
    }
    files = {
        # Anki renames exactly the names holding a disallowed character, '>'
        # among them, so an old name may hold the separator and a new name
        # never does: split at the last one.
        "renamed": _pattern(_plain(tr.media_check_renamed_file(old=S0, new=S1)), (S0, S1), split_last=True),
        "oversize": _pattern(_plain(tr.media_check_oversize_file(filename=S0)), (S0,)),
        "dirs": _pattern(_plain(tr.media_check_subfolder_file(filename=S0)), (S0,)),
        "missing": _pattern(_plain(tr.media_check_missing_file(filename=S0)), (S0,)),
        "unused": _pattern(_plain(tr.media_check_unused_file(filename=S0)), (S0,)),
    }
    found: dict[str, int] = {}
    lists: dict[str, list] = {k: [] for k in files}
    section: str | None = None
    top = True
    for line in report.split("\n"):
        if line == "":
            section, top = None, False
            continue
        if top:
            # Summary lines keep Anki's isolation marks; file lines do not.
            plain = _plain(line)
            hits = [(k, n) for k, match in counts.items() if (n := match(plain)) is not None]
            if len(hits) != 1 or hits[0][0] in found:
                raise _unreadable("a summary line")
            found[hits[0][0]] = hits[0][1]
            continue
        if section is None:
            plain = _plain(line)
            if plain not in headers or lists[headers[plain]]:
                raise _unreadable("a section header")
            section = headers[plain]
            continue
        m = files[section].fullmatch(line)
        if not m:
            raise _unreadable("a file line")
        lists[section].append(m.groups() if section == "renamed" else m.group(1))
    for kind in ("missing", "unused"):
        if kind not in found:
            raise _unreadable("no line for a count it always shows")
    for kind in files:
        if found.get(kind, 0) != len(lists[kind]):
            raise _unreadable("a list whose length differs from its count")
    renamed = dict(lists["renamed"])
    if len(renamed) != len(lists["renamed"]):
        raise _unreadable("a file renamed twice")
    return {
        "lists": lists,
        "renamed": renamed,
        "extracted": found.get("extracted", 0),
        "trash_count": found.get("trash", 0),
        "templates": templates,
    }


def _fold(name: str) -> str:
    return unicodedata.normalize("NFC", name).casefold()


def _check_renamed(folder: str, renamed: dict[str, str]) -> None:
    """After the run each new name is in the folder and its old name is not
    (unless the two are NFC and case variants, which a case-insensitive
    volume shows as one entry): a line split at the wrong separator fails
    here instead of reaching the output."""
    for old, new in renamed.items():
        if not os.path.lexists(os.path.join(folder, new)):
            raise _unreadable("a renamed file whose new name is not in the folder")
        if _fold(old) != _fold(new) and os.path.lexists(os.path.join(folder, old)):
            raise _unreadable("a renamed file whose old name is still in the folder")


def _by_bytes(names) -> list[str]:
    return sorted(names, key=lambda n: n.encode("utf-8"))


def _template_order(col, templates: list[tuple[str, str, str]]) -> list[dict]:
    """C9's order: note type name (unicase, then bytes), template ordinal,
    front before back."""
    names = sorted({t[0] for t in templates})
    rank: dict[str, int] = {}
    if names:
        sql = " UNION ALL ".join("SELECT ? AS n" for _ in names) + " ORDER BY n COLLATE unicase, n"
        rank = {n: i for i, n in enumerate(col.db.list(sql, *names))}
    keyed = []
    for notetype, template, side in templates:
        model = col.models.by_name(notetype)
        ords = [t["ord"] for t in (model or {}).get("tmpls", []) if t["name"] == template]
        if len(ords) != 1:
            raise _unreadable("a template it cannot place")
        keyed.append(((rank[notetype], ords[0], side != "front"), {"notetype": notetype, "template": template, "side": side}))
    return [entry for _, entry in sorted(keyed, key=lambda k: k[0])]


def _no_arguments(name: str, args: Any) -> None:
    if not isinstance(args, dict) or args:
        raise invalid(f"{name} takes {{}}")


@op("check_media")
def check_media(session, args):
    _no_arguments("check_media", args)
    col = session.col
    session.current.data.pop(KEY, None)
    response = col.media.check()
    report = parse_report(col.tr, response.report)
    if sorted(report["lists"]["missing"]) != sorted(response.missing) or sorted(report["lists"]["unused"]) != sorted(response.unused):
        raise _unreadable("lists that differ from the response")
    _check_renamed(col.media.dir(), report["renamed"])
    note_ids = sorted(set(response.missing_media_notes))
    session.current.data[KEY] = {"unused": _by_bytes(response.unused), "missing_notes": note_ids}
    return {
        "unused": _by_bytes(response.unused),
        "missing": _by_bytes(response.missing),
        "missing_notes": [session.note_ref(n) if n in session.notes else n for n in note_ids],
        "renamed": dict(sorted(report["renamed"].items(), key=lambda kv: kv[0].encode("utf-8"))),
        "dirs": _by_bytes(report["lists"]["dirs"]),
        "oversize": _by_bytes(report["lists"]["oversize"]),
        "extracted": report["extracted"],
        "trash_count": report["trash_count"],
        "field_templates": _template_order(col, report["templates"]),
    }


def _kept(session, name: str) -> dict:
    kept = session.current.data.get(KEY)
    if kept is None:
        raise invalid(f"{name} needs an earlier check_media step")
    return kept


@op("delete_unused_media")
def delete_unused_media(session, args):
    _no_arguments("delete_unused_media", args)
    names = _kept(session, "delete_unused_media")["unused"]
    for start in range(0, len(names), BATCH):
        session.col.media.trash_files(names[start:start + BATCH])
    return {"count": len(names)}


@op("tag_missing_media")
def tag_missing_media(session, args):
    _no_arguments("tag_missing_media", args)
    note_ids = _kept(session, "tag_missing_media")["missing_notes"]
    return {"count": session.col.tags.bulk_add(note_ids, TAG).count}


@scenario_check
def check_media_actions(scenario: dict) -> None:
    """F14.2, F14.3: the actions act on a check_media step of their own
    segment, and only F14.2's steps run between check_media and
    delete_unused_media."""
    since: list[str] | None = None
    for index, step in enumerate(scenario.get("steps", []) or []):
        if not isinstance(step, dict) or len(step) != 1:
            continue
        ((name, _args),) = step.items()
        if name == "handoff":
            since = None
        elif name == "check_media":
            since = []
        elif name in ("delete_unused_media", "tag_missing_media"):
            if since is None:
                raise invalid(f"{name} needs a check_media step since the start or the latest handoff", step=index)
            if name == "delete_unused_media" and any(s not in BETWEEN for s in since):
                raise invalid("only observe, advance, set_clock and tag_missing_media may run between check_media "
                              "and delete_unused_media (media-ops.md F14.2)", step=index)
            since.append(name)
        elif since is not None:
            since.append(name)
