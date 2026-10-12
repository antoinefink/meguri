#!/usr/bin/env python3
"""Validate tools/mg result.json files against tools/mg-result.schema.json.

Usage: validate_result.py RESULT.json...  (exit 1 and one line per error if any is invalid)
Runs in the dev container (python3-jsonschema).
"""
import json
import pathlib
import sys

from jsonschema import Draft202012Validator, FormatChecker

SCHEMA = pathlib.Path(__file__).resolve().parents[1] / "mg-result.schema.json"


def main(paths):
    schema = json.loads(SCHEMA.read_text())
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    errors = 0
    for path in paths:
        instance = json.loads(pathlib.Path(path).read_text())
        for error in sorted(validator.iter_errors(instance), key=lambda e: list(e.path)):
            location = "/".join(str(p) for p in error.path) or "(root)"
            print(f"{path}: {location}: {error.message}")
            errors += 1
    return 1 if errors or not paths else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
