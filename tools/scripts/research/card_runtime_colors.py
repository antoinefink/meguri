"""Compile only independently specified color calculations, returning JSON facts."""

import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile
import urllib.request


VERSION = "1.102.0"
URL = "https://github.com/sass/dart-sass/releases/download/1.102.0/dart-sass-1.102.0-linux-arm64.tar.gz"
ARCHIVE_SHA256 = "82a5b53decbd2ed1d9ef3452e039e3f57ca936f80e96407dd1add1653c282763"
ARCHIVE_BYTES = 4426364


def program(facts):
    cases = facts.get("cases", [])
    if facts.get("schema_version") != 1 or facts.get("compiler", {}).get("version") != VERSION or len(cases) != 17:
        raise ValueError("expected the fixed 17-case compiler contract")
    lines = ['@use "sass:color";', ".probe {"]
    seen = set()
    for i, case in enumerate(cases):
        prop, appearance = case["property"], case["appearance"]
        color, channel, percent = case["color"], case["channel"], case["scale_percent"]
        if (not isinstance(prop, str) or not re.fullmatch(r"--[a-z][a-z-]*", prop)
                or appearance not in ("light", "dark") or (prop, appearance) in seen
                or not isinstance(color, str) or not re.fullmatch(r"#[0-9a-fA-F]{6}", color)
                or channel not in ("alpha", "lightness", "saturation")
                or type(percent) not in (int, float) or not -100 <= percent <= 100
                or not isinstance(case["expected"], str)):
            raise ValueError("invalid neutral color input")
        seen.add((prop, appearance))
        lines.append("  --probe-%d: #{color.scale(%s, $%s: %s%%)};" % (i, color, channel, percent))
    return "\n".join(lines + ["}"])


def result(facts, css, input_sha256):
    values = re.findall(r"^  --probe-(\d+): ([^;\n]+);$", css, re.MULTILINE)
    if [int(i) for i, _ in values] != list(range(17)):
        raise ValueError("compiler returned incomplete or duplicated declarations")
    cases = [{"property": c["property"], "appearance": c["appearance"], "value": value}
             for c, (_, value) in zip(facts["cases"], values)]
    passed = sum(value["value"] == case["expected"] for value, case in zip(cases, facts["cases"]))
    return {"schema_version": 1, "compiler": {"version": VERSION, "url": URL, "archive_sha256": ARCHIVE_SHA256},
            "input_sha256": input_sha256, "cases": cases,
            "totals": {"cases": 17, "pass": passed, "mismatch": 17 - passed}}


def main(source, output):
    data = Path(source).read_bytes()
    facts = json.loads(data)
    scss = program(facts)
    with urllib.request.urlopen(URL, timeout=60) as response:
        archive = response.read(ARCHIVE_BYTES + 1)
    if len(archive) != ARCHIVE_BYTES or hashlib.sha256(archive).hexdigest() != ARCHIVE_SHA256:
        raise ValueError("public compiler archive failed its pinned size or SHA256 check")
    with tempfile.TemporaryDirectory() as temp:
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as packed:
            packed.extractall(temp, filter="data")
        compiler = Path(temp) / "dart-sass" / "sass"
        version = subprocess.run([compiler, "--version"], check=True, capture_output=True, text=True, timeout=10)
        if version.stdout.strip() != VERSION:
            raise ValueError("compiler version does not match its pin")
        compiled = subprocess.run([compiler, "--stdin", "--style=expanded", "--no-source-map"],
                                  input=scss, check=True, capture_output=True, text=True, timeout=30)
    report = result(facts, compiled.stdout, hashlib.sha256(data).hexdigest())
    Path(output).write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return int(report["totals"]["mismatch"] != 0)


if __name__ == "__main__":
    try:
        sys.exit(main(*sys.argv[1:]))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as exc:
        # Fixed public inputs only; no source excerpts or compiler output.
        print("public compiler check failed (%s)" % type(exc).__name__, file=sys.stderr)
        sys.exit(2)
