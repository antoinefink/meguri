# SPDX-License-Identifier: AGPL-3.0-or-later
"""Note types. Scenarios define their own (R1959): Anki's stock note types,
templates and CSS never reach an observation through the driver.

setup.notetypes: [{name, kind?: standard|cloze, fields: [names],
                   templates: [{name, front?, back?, browser_front?,
                                browser_back?}], css?: text}]

browser_front and browser_back (dsl.md D6.4) are the schema-11 bqfmt and
bafmt, set on the template dict before the note type is added: a save stores
them without checking them, so they may hold template errors. Templates are
checked before any collection exists (scenario_check).
"""

from __future__ import annotations

from typing import Any

from ..errors import invalid
from ..registry import scenario_check, setup_section

NOTETYPE_KEYS = {"name", "kind", "fields", "templates", "css"}
# Scenario key -> schema-11 template key.
TEMPLATE_TEXTS = {"front": "qfmt", "back": "afmt", "browser_front": "bqfmt", "browser_back": "bafmt"}
TEMPLATE_KEYS = {"name", *TEMPLATE_TEXTS}


def _check_template(tmpl: Any) -> None:
    if not isinstance(tmpl, dict) or set(tmpl) - TEMPLATE_KEYS or "name" not in tmpl:
        raise invalid("a template is {name, front?, back?, browser_front?, browser_back?}")
    if not all(isinstance(tmpl.get(k, ""), str) for k in TEMPLATE_KEYS):
        raise invalid("a template's name and texts are strings")


@scenario_check
def check_templates(scenario: dict) -> None:
    """The templates of setup.notetypes, before any collection; the rest of a
    note type is checked when the section runs."""
    setup = scenario.get("setup")
    items = setup.get("notetypes") if isinstance(setup, dict) else None
    for item in items if isinstance(items, list) else []:
        templates = item.get("templates") if isinstance(item, dict) else None
        for tmpl in templates if isinstance(templates, list) else []:
            _check_template(tmpl)


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
        if not isinstance(item.get("css", ""), str):
            raise invalid("css is a string")
        for tmpl in templates:
            _check_template(tmpl)
        nt = models.new(item["name"])
        nt["type"] = 1 if kind == "cloze" else 0
        nt["css"] = item.get("css", "")
        for name in fields:
            models.add_field(nt, models.new_field(name))
        for tmpl in templates:
            t = models.new_template(tmpl["name"])
            for key, legacy in TEMPLATE_TEXTS.items():
                t[legacy] = tmpl.get(key, "")
            models.add_template(nt, t)
        session.notetypes[item["name"]] = models.add_dict(nt).id
