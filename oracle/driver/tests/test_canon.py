# SPDX-License-Identifier: AGPL-3.0-or-later
"""Scenario YAML subset, RFC 8785 hashing input and golden text layout."""

from __future__ import annotations

import hashlib
import json
import unittest

from driver.canon import ScenarioFormatError, golden_text, jcs, load_yaml_text, scenario_sha256


class Yaml12Core(unittest.TestCase):
    def test_scalars_follow_the_yaml_1_2_core_schema(self):
        doc = load_yaml_text(
            "a: yes\nb: on\nc: 017\nd: 0o17\ne: 0x1F\nf: 2030-01-15\ng: 1:20\nh: true\ni: ~\nj: 1.5e3\nk: '17'\nl: No\n"
        )
        self.assertEqual(
            doc,
            {"a": "yes", "b": "on", "c": 17, "d": 15, "e": 31, "f": "2030-01-15", "g": "1:20",
             "h": True, "i": None, "j": 1500.0, "k": "17", "l": "No"},
        )

    def test_constructs_outside_the_json_subset_are_rejected(self):
        for text in (
            "a: &x 1\nb: *x\n",  # alias
            "base: {a: 1}\nderived:\n  <<: {a: 2}\n",  # merge key
            "a: !!binary aGk=\n",  # explicit tag
            "a: 1\na: 2\n",  # duplicate key
            "1: a\n",  # non-string key
            "a: .inf\n",  # no JSON form
        ):
            with self.subTest(text=text):
                with self.assertRaises(ScenarioFormatError):
                    load_yaml_text(text)

    def test_core_schema_tags_are_rejected_too(self):
        # Each resolves to an allowed type, but scenarios carry no tags at all.
        for text in (
            "a: !!str true\n",
            "a: !!int '1'\n",
            "a: !!float 1\n",
            "a: !!bool true\n",
            "a: !!null ''\n",
            "a: !!map {b: 1}\n",
            "a: !!seq [1]\n",
            "!!map {a: 1}\n",
            "a: [!!str x]\n",
            "!!str a: 1\n",  # on a key
            "a: ! x\n",  # the non-specific tag
            "%TAG !e! tag:yaml.org,2002:\n---\na: !e!str x\n",
        ):
            with self.subTest(text=text):
                with self.assertRaises(ScenarioFormatError):
                    load_yaml_text(text)
        self.assertEqual(load_yaml_text("a: 'true'\nb: \"1\"\nc: x!y\n"), {"a": "true", "b": "1", "c": "x!y"})

    def test_hash_ignores_comments_key_order_and_layout(self):
        a = load_yaml_text("id: x\n# a comment\nclock: {tz: UTC, start: '2030-01-01T00:00:00'}\nsteps: []\n")
        b = load_yaml_text("steps: []\nclock:\n  start: '2030-01-01T00:00:00'\n  tz: UTC\nid: x\n")
        self.assertEqual(scenario_sha256(a), scenario_sha256(b))
        c = load_yaml_text("id: x\nclock: {tz: UTC, start: '2030-01-01T00:00:01'}\nsteps: []\n")
        self.assertNotEqual(scenario_sha256(a), scenario_sha256(c))

    def test_hash_is_sha256_of_the_jcs_text(self):
        value = {"b": [1, 2.5, "é"], "a": None}
        self.assertEqual(jcs(value), '{"a":null,"b":[1,2.5,"é"]}')
        self.assertEqual(scenario_sha256(value), hashlib.sha256('{"a":null,"b":[1,2.5,"é"]}'.encode()).hexdigest())


class Rfc8785(unittest.TestCase):
    """Number and string vectors from RFC 8785 (section 3.2.2.3, appendix B)."""

    def test_numbers(self):
        cases = {
            0.0: "0",
            -0.0: "0",
            5e-324: "5e-324",
            1.7976931348623157e308: "1.7976931348623157e+308",
            9007199254740992: "9007199254740992",
            295147905179352830000.0: "295147905179352830000",
            1e21: "1e+21",
            9.999999999999997e22: "9.999999999999997e+22",
            0.000001: "0.000001",
            1e-7: "1e-7",
            333333333.3333333: "333333333.3333333",
            4.5: "4.5",
            0.002: "0.002",
            100.0: "100",
            -1.5: "-1.5",
        }
        for value, text in cases.items():
            with self.subTest(value=value):
                self.assertEqual(jcs(value), text)

    def test_strings_and_key_order(self):
        self.assertEqual(jcs("€$\u000f\nA'B\"\\\\\"/"), '"€$\\u000f\\nA\'B\\"\\\\\\\\\\"/"')
        # Keys sort by UTF-16 code units: U+10000 (D800 DC00) sorts before U+E000.
        self.assertEqual(jcs({"": 1, "\U00010000": 2, "a": 3}), '{"a":3,"\U00010000":2,"":1}')


class GoldenText(unittest.TestCase):
    def test_layout(self):
        text = golden_text({"b": {"rows": [[1, 2], [3, 4]], "x": [1.5, "s", None]}, "a": {}})
        self.assertEqual(
            text,
            '{\n  "a": {},\n  "b": {\n    "rows": [\n      [1, 2],\n      [3, 4]\n    ],\n'
            '    "x": [1.5, "s", null]\n  }\n}\n',
        )
        self.assertEqual(json.loads(text), {"a": {}, "b": {"rows": [[1, 2], [3, 4]], "x": [1.5, "s", None]}})


if __name__ == "__main__":
    unittest.main()
