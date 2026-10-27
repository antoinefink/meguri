#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Validates the conformance tree against its JSON schemas (tools/mg goldens-check).

  python3 -B -I tools/conformance/check_schemas.py --cli <meguri-cli> --conformance conformance --report FILE

Authored files use `meguri-cli conformance to-json` with the runner's D2
limits. Golden/overlay JSON explicitly uses its observation conversion mode
to retain exact signed/unsigned 64-bit integers (observations.md O6).
Needs python3-jsonschema (the dev image has it).
"""

from __future__ import annotations

import argparse
from concurrent.futures import ProcessPoolExecutor
import json
import re
import subprocess
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

SCHEMAS = ("scenario", "observation", "golden", "area", "deviations", "overlay")
VALIDATORS: dict[str, Draft202012Validator] = {}


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


def init_validators(schemas: dict[str, dict]) -> None:
    global VALIDATORS
    VALIDATORS = {name: validator(schemas, name) for name in SCHEMAS}


def check_item(task: tuple[str, dict]) -> list[dict]:
    rel, item = task
    if "error" in item:
        return [{"file": rel, "message": item["error"]}]
    errors = []
    for error in sorted(VALIDATORS[schema_for(rel)].iter_errors(item["value"]), key=lambda e: list(e.path)):
        where = "/" + "/".join(str(x) for x in error.path)
        errors.append({"file": rel, "message": f"{where}: {error.message}"})
    return errors


def check_reference_resolution(tasks: list[tuple[str, dict]]) -> list[dict]:
    """D9.4 resolution uses paths, exact relative ids and D9.1's two globs."""
    ids = set()
    for rel, _ in tasks:
        if schema_for(rel) == "scenario":
            for suffix in (".yaml", ".yml", ".json"):
                if rel.endswith(suffix):
                    ids.add(rel[len("scenarios/"):-len(suffix)])
                    break
    errors = []
    for rel, item in tasks:
        if schema_for(rel) != "area" or "error" in item:
            continue
        area = item.get("value")
        # Shape errors are already reported by the strict area schema.
        if not isinstance(area, dict) or not isinstance(area.get("reference", {}), dict):
            continue
        prefix = rel[len("scenarios/"):].rsplit("/", 1)[0]
        active = area.get("active", [])
        active = active if isinstance(active, list) else []
        for name in area.get("reference", {}):
            full = prefix + "/" + name
            message = None
            if name.endswith((".yaml", ".yml", ".json")):
                message = f"reference {name!r} must have no suffix"
            elif full not in ids:
                message = f"reference {name!r} names no scenario"
            elif any(isinstance(pattern, str) and re.fullmatch(
                    re.escape(pattern).replace(r"\*", ".*").replace(r"\?", "."), name)
                    for pattern in active):
                message = f"reference {name!r} also matches active"
            if message:
                errors.append({"file": rel, "message": message})
    return errors


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--cli", required=True)
    p.add_argument("--conformance", required=True)
    p.add_argument("--report", required=True)
    p.add_argument("--jobs", type=int, default=4, help="schema workers (default: 4; 1 runs serially)")
    args = p.parse_args(argv)
    if args.jobs < 1:
        p.error("--jobs must be positive")
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
        tasks = []
        for observations in (False, True):
            # The path/schema context selects the mode, never document values.
            group = [(rel, path) for rel, path in files
                     if (schema_for(rel) in ("golden", "overlay")) == observations]
            if not group:
                continue
            mode = ["--observation-json"] if observations else []
            out = subprocess.run(
                [args.cli, "conformance", "to-json", *mode, "--", *[str(path) for _, path in group]],
                capture_output=True, text=True, check=False,
            )
            # Not splitlines(): JSON strings can carry raw U+2028 and U+0085.
            parsed = [json.loads(line) for line in out.stdout.split("\n") if line.strip()]
            if len(parsed) != len(group):
                errors.append({"file": "to-json", "message": f"meguri-cli parsed {len(parsed)} of {len(group)} files: {out.stderr[-2000:]}"})
            if out.returncode not in (0, 1) or (out.returncode == 1 and not any("error" in item for item in parsed)):
                errors.append({"file": "to-json", "message": f"meguri-cli exited {out.returncode}: {out.stderr[-2000:]}"})
            for (rel, path), item in zip(group, parsed):
                if item.get("path") != str(path):
                    errors.append({"file": rel, "message": "meguri-cli returned a mismatched path"})
                else:
                    tasks.append((rel, item))
        tasks.sort(key=lambda task: task[0])
        if args.jobs == 1 or len(tasks) < 2:
            init_validators(schemas)
            for task in tasks:
                errors.extend(check_item(task))
        else:
            # map preserves sorted file order; failures propagate and fail the job.
            with ProcessPoolExecutor(max_workers=min(args.jobs, len(tasks)),
                                     initializer=init_validators, initargs=(schemas,)) as workers:
                for item_errors in workers.map(check_item, tasks):
                    errors.extend(item_errors)
        errors.extend(check_reference_resolution(tasks))
    for e in errors:
        print(f"schema: {e['file']}: {e['message']}", file=sys.stderr)
    print(f"schema: {len(files)} files, {len(errors)} errors", file=sys.stderr)
    Path(args.report).write_text(json.dumps({"files": len(files), "errors": errors}, indent=2) + "\n", encoding="utf-8")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
