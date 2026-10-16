#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Validates the conformance tree against its JSON schemas (tools/mg goldens-check).

  python3 -B -I tools/conformance/check_schemas.py --cli <meguri-cli> --conformance conformance --report FILE

YAML files are parsed by `meguri-cli conformance to-json`, so the values are
the ones the runner and the scenario hash see (YAML 1.2 core schema subset).
Needs python3-jsonschema (the dev image has it).
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

SCHEMAS = ("scenario", "observation", "golden", "area", "deviations", "overlay")


def load_schemas(root: Path) -> dict[str, dict]:
    schemas = {}
    for name in SCHEMAS:
        schema = json.loads((root / "schema" / f"{name}.schema.json").read_text(encoding="utf-8"))
        Draft202012Validator.check_schema(schema)
        schemas[name] = schema
    return schemas


def validator(schemas: dict[str, dict], name: str) -> Draft202012Validator:
    schema = schemas[name]
    try:
        from referencing import Registry, Resource

        registry = Registry().with_resources((s["$id"], Resource.from_contents(s)) for s in schemas.values())
        return Draft202012Validator(schema, registry=registry)
    except ImportError:  # jsonschema before 4.18
        from jsonschema import RefResolver

        store = {s["$id"]: s for s in schemas.values()}
        return Draft202012Validator(schema, resolver=RefResolver(schema["$id"], schema, store=store))


def schema_for(rel: str) -> str | None:
    """The schema of a file, by its path relative to the conformance root."""
    name = rel.rsplit("/", 1)[-1]
    if rel == "deviations.yaml":
        return "deviations"
    if rel.startswith("scenarios/"):
        if name in ("area.yaml", "area.yml"):
            return "area"
        if name.endswith((".yaml", ".yml", ".json")):
            return "scenario"
    if rel.startswith("goldens/") and name.endswith(".json"):
        return "golden"
    if rel.startswith("goldens-apple/") and name.endswith(".json"):
        return "overlay"
    return None


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--cli", required=True)
    p.add_argument("--conformance", required=True)
    p.add_argument("--report", required=True)
    args = p.parse_args(argv)
    root = Path(args.conformance)
    errors: list[dict] = []
    try:
        schemas = load_schemas(root)
    except Exception as exc:  # noqa: BLE001 - reported, not raised
        errors.append({"file": "schema", "message": f"invalid schema: {exc}"})
        schemas = {}
    files = sorted(
        (p.relative_to(root).as_posix(), p)
        for p in root.rglob("*")
        if p.is_file() and schema_for(p.relative_to(root).as_posix())
    )
    if schemas and files:
        out = subprocess.run(
            [args.cli, "conformance", "to-json", *[str(p) for _, p in files]],
            capture_output=True, text=True, check=False,
        )
        # One JSON value per "\n"-terminated line. Not splitlines(): it also
        # breaks at U+2028, U+0085 and other characters JSON strings carry raw.
        parsed = [json.loads(line) for line in out.stdout.split("\n") if line.strip()]
        if len(parsed) != len(files):
            errors.append({"file": "to-json", "message": f"meguri-cli parsed {len(parsed)} of {len(files)} files: {out.stderr[-2000:]}"})
        validators = {name: validator(schemas, name) for name in SCHEMAS}
        for (rel, _), item in zip(files, parsed):
            if "error" in item:
                errors.append({"file": rel, "message": item["error"]})
                continue
            for e in sorted(validators[schema_for(rel)].iter_errors(item["value"]), key=lambda e: list(e.path)):
                where = "/" + "/".join(str(x) for x in e.path)
                errors.append({"file": rel, "message": f"{where}: {e.message}"})
    for e in errors:
        print(f"schema: {e['file']}: {e['message']}", file=sys.stderr)
    print(f"schema: {len(files)} files, {len(errors)} errors", file=sys.stderr)
    Path(args.report).write_text(json.dumps({"files": len(files), "errors": errors}, indent=2) + "\n", encoding="utf-8")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
