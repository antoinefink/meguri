import copy
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import card_runtime_colors as colors


class ColorReferenceTests(unittest.TestCase):
    def setUp(self):
        self.facts = {"schema_version": 1, "compiler": {"version": "1.102.0"}, "cases": [
            {"property": "--probe-" + chr(97 + i), "appearance": "light", "color": "#ffffff",
             "channel": "alpha", "scale_percent": -60, "expected": "rgba(255, 255, 255, 0.4)"}
            for i in range(17)]}
        self.css = ".probe {\n" + "\n".join("  --probe-%d: %s;" % (i, c["expected"])
                                               for i, c in enumerate(self.facts["cases"])) + "\n}"

    def test_fixed_calculations_are_compiled_without_source_assets(self):
        program = colors.program(self.facts)
        self.assertIn('color.scale(#ffffff, $alpha: -60%)', program)
        self.assertEqual(program.count("color.scale("), 17)
        report = colors.result(self.facts, self.css, "input-digest")
        self.assertEqual(report["totals"], {"cases": 17, "pass": 17, "mismatch": 0})
        self.assertEqual(set(report["cases"][0]), {"property", "appearance", "value"})

    def test_malformed_or_executable_inputs_are_rejected(self):
        for key, value in (("channel", "alpha); @use 'bad'"), ("color", "red; bad"),
                           ("scale_percent", True), ("scale_percent", 101), ("property", "--a; bad")):
            facts = copy.deepcopy(self.facts)
            facts["cases"][0][key] = value
            with self.assertRaises(ValueError):
                colors.program(facts)
        self.facts["cases"].append(self.facts["cases"][0])
        with self.assertRaises(ValueError):
            colors.program(self.facts)

    def test_output_must_be_complete_and_preserve_exact_text(self):
        for css in (self.css.replace("--probe-0:", "--probe-1:"), self.css.replace("--probe-0:", "--other:")):
            with self.assertRaises(ValueError):
                colors.result(self.facts, css, "digest")
        report = colors.result(self.facts, self.css.replace("0.4", "0.400", 1), "digest")
        self.assertEqual(report["totals"]["mismatch"], 1)

    def test_archive_integrity_is_checked_before_extraction_or_execution(self):
        with tempfile.TemporaryDirectory() as temp:
            source, output = Path(temp) / "facts.json", Path(temp) / "output.json"
            source.write_text(json.dumps(self.facts))
            with mock.patch.object(colors.urllib.request, "urlopen", return_value=io.BytesIO(b"wrong")), \
                    mock.patch.object(colors.subprocess, "run") as command, \
                    mock.patch.object(colors.tarfile, "open") as extract:
                with self.assertRaises(ValueError):
                    colors.main(source, output)
                extract.assert_not_called()
                command.assert_not_called()
                self.assertFalse(output.exists())

    def test_verified_archive_and_version_produce_only_neutral_json(self):
        packed = io.BytesIO()
        with tarfile.open(fileobj=packed, mode="w:gz") as archive:
            item = tarfile.TarInfo("dart-sass/sass")
            item.size = 4
            archive.addfile(item, io.BytesIO(b"fake"))
        data = packed.getvalue()
        with tempfile.TemporaryDirectory() as temp:
            source, output = Path(temp) / "facts.json", Path(temp) / "output.json"
            source.write_text(json.dumps(self.facts))
            for version in ("0.0.0", colors.VERSION):
                with mock.patch.object(colors, "ARCHIVE_BYTES", len(data)), \
                        mock.patch.object(colors, "ARCHIVE_SHA256", hashlib.sha256(data).hexdigest()), \
                        mock.patch.object(colors.urllib.request, "urlopen", return_value=io.BytesIO(data)), \
                        mock.patch.object(colors.subprocess, "run", side_effect=[
                            subprocess.CompletedProcess([], 0, version + "\n"),
                            subprocess.CompletedProcess([], 0, self.css)]) as command:
                    if version != colors.VERSION:
                        with self.assertRaises(ValueError):
                            colors.main(source, output)
                        self.assertFalse(output.exists())
                    else:
                        self.assertEqual(colors.main(source, output), 0)
                        self.assertEqual(json.loads(output.read_text())["totals"]["pass"], 17)
                        self.assertIn("--stdin", command.call_args.args[0])


if __name__ == "__main__":
    unittest.main()
