# SPDX-License-Identifier: AGPL-3.0-or-later
"""Note types. Scenarios define their own (R1959): Anki's stock note types,
templates and CSS never reach an observation through the driver.

setup.notetypes: [{name, kind?: standard|cloze, fields: [names],
                   templates: [{name, front, back}], css?: text}]
"""

from __future__ import annotations

from ..errors import invalid
from ..registry import setup_section

NOTETYPE_KEYS = {"name", "kind", "fields", "templates", "css"}
TEMPLATE_KEYS = {"name", "front", "back"}


@setup_section("notetypes", order=40)
def notetypes(session, items):
    if not isinstance(items, list):
        raise invalid("setup.notetypes is a list")
    models = session.col.models
    for item in items:
        if not isinstance(item, dict) or not isinstance(item.get("name"), str):
            raise invalid("a note type is a mapping with a name")
        unknown = sorted(set(item) - NOTETYPE_KEYS)
        if unknown:
            raise invalid(f"unknown note type keys: {', '.join(unknown)}")
        kind = item.get("kind", "standard")
        if kind not in ("standard", "cloze"):
            raise invalid("note type kind is standard or cloze")
        fields, templates = item.get("fields"), item.get("templates")
        if not isinstance(fields, list) or not fields or not all(isinstance(f, str) for f in fields):
            raise invalid("fields is a non-empty list of names")
        if not isinstance(templates, list) or not templates:
            raise invalid("templates is a non-empty list")
        nt = models.new(item["name"])
        nt["type"] = 1 if kind == "cloze" else 0
        nt["css"] = item.get("css", "")
        for name in fields:
            models.add_field(nt, models.new_field(name))
        for tmpl in templates:
            if not isinstance(tmpl, dict) or set(tmpl) - TEMPLATE_KEYS or "name" not in tmpl:
                raise invalid("a template is {name, front, back}")
            t = models.new_template(tmpl["name"])
            t["qfmt"] = tmpl.get("front", "")
            t["afmt"] = tmpl.get("back", "")
            models.add_template(nt, t)
        session.notetypes[item["name"]] = models.add_dict(nt).id
