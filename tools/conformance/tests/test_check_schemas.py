#!/usr/bin/env python3
"""The parallel validator must retain every serial diagnostic in file order."""
import json
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from jsonschema import Draft202012Validator

SCRIPT = Path(__file__).resolve().parents[1] / 'check_schemas.py'


def shared_validator(name):
    spec = importlib.util.spec_from_file_location("schema_checks", SCRIPT)
    checks = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(checks)
    schemas = checks.load_schemas(SCRIPT.parents[2] / "conformance")
    return checks.validator(schemas, name)


class SchemaChecks(unittest.TestCase):
    def protocol_begin(self):
        schema = json.loads((SCRIPT.parents[2] / "conformance/schema/protocol.json").read_text())
        Draft202012Validator.check_schema(schema)
        reply = {"ok": True, "scenario_id": "sync/route-metadata", "steps": 0,
                 "sync": {"endpoints": {"actors": "http://proxy.example.invalid:40000/",
                                        "converge": "http://127.0.0.1:40001/",
                                        "upstream": "192.0.2.10:40001", "recorder": None},
                          "account": {"username": "fixture-user", "password": "fixture-password"},
                          "route": {"proxy": "meguri-edge-" + "a" * 32 + "-" + "b" * 32,
                                    "worker": "c" * 32}}}
        return Draft202012Validator(schema), reply

    def test_begin_sync_metadata_requires_route_and_accepts_mixed_or_replay(self):
        validate, reply = self.protocol_begin()
        self.assertEqual(list(validate.iter_errors(reply)), [])
        reply["sync"]["route"] = None
        self.assertEqual(list(validate.iter_errors(reply)), [])
        del reply["sync"]["route"]
        self.assertTrue(list(validate.iter_errors(reply)))
        del reply["sync"]  # A collection-only begin has no sync metadata.
        self.assertEqual(list(validate.iter_errors(reply)), [])

    def test_begin_sync_route_is_closed_and_requires_both_tokens(self):
        validate, reply = self.protocol_begin()
        route = reply["sync"]["route"]
        malformed = [True, False, 0, "", [], {}, {"proxy": route["proxy"]},
                     {"worker": route["worker"]}, dict(route, extra="unexpected")]
        for field in ("proxy", "worker"):
            for value in (None, True, 1, [], {}):
                malformed.append(dict(route, **{field: value}))
        for index, bad in enumerate(malformed):
            with self.subTest(case=index):
                reply["sync"]["route"] = bad
                self.assertTrue(list(validate.iter_errors(reply)))
        reply["sync"]["route"] = route
        reply["sync"]["extra"] = True
        self.assertTrue(list(validate.iter_errors(reply)))

    def test_begin_sync_route_tokens_have_exact_lowercase_hex_shapes(self):
        validate, reply = self.protocol_begin()
        route = reply["sync"]["route"]
        for field in ("proxy", "worker"):
            value = route[field]
            malformed = ["", value.upper(), value[:-1], value + "0", value + "\n",
                         value + "\r\n", " " + value, value + " ", value[:-1] + "g"]
            if field == "proxy":
                malformed += ["meguri-oracle-" + "b" * 32,
                              "meguri-edge-" + "a" * 31 + "-" + "b" * 33,
                              "meguri-edge-" + "a" * 33 + "-" + "b" * 31]
            for index, bad in enumerate(malformed):
                with self.subTest(field=field, case=index):
                    reply["sync"]["route"] = dict(route, **{field: bad})
                    self.assertTrue(list(validate.iter_errors(reply)))
        reply["sync"]["route"] = route
        self.assertEqual(list(validate.iter_errors(reply)), [])

    def test_review_session_is_a_strict_collection_or_actor_local_marker(self):
        validate = shared_validator("scenario")
        observation = shared_validator("observation")
        for actors in (None, ["client_a", "meguri"]):
            base = {"id": "sync/review-session",
                    "clock": {"tz": "UTC", "start": "2030-01-15T10:00:00"}}
            if actors is not None:
                base["actors"] = actors
            steps = [{"review_session": "begin"}, {"review_session": "end"}]
            if actors is not None:
                steps += [{"actor": "meguri"}, {"review_session": "begin"}]
            self.assertEqual(list(validate.iter_errors(dict(base, steps=steps))), [])
            for bad in (None, True, 1, 1.5, [], ["begin"], {}, {"begin": None},
                        {"end": None}, {"kind": "begin"}, "", "Begin", "pause"):
                with self.subTest(actors=actors, argument=bad):
                    self.assertTrue(list(validate.iter_errors(
                        dict(base, steps=[{"review_session": bad}]))))
            for step in ({"review_session": "begin", "reopen": {}},
                         {"review_session": "begin", "extra": True},
                         {"review_sessions": "begin"}):
                self.assertTrue(list(validate.iter_errors(dict(base, steps=[step]))), step)
        # Markers produce the ordinary empty step output, with no invented key.
        self.assertEqual(list(observation.iter_errors({"steps": [{}, {}]})), [])
        self.assertTrue(list(observation.iter_errors({"steps": [{"review_session": {}}]})))

    def test_sync_rejection_list_and_shapes_remain_closed(self):
        validate = shared_validator("observation")
        errors = ("auth_failed", "clock_incorrect", "network",
                  "sanity_check_failed", "db_check_required")
        for error in errors:
            self.assertEqual(list(validate.iter_errors({"steps": [{"sync": {"error": error}}]})), [])
            for bad in ({"error": error, "extra": True},
                        {"error": error, "result": "ok"}):
                self.assertTrue(list(validate.iter_errors({"steps": [{"sync": bad}]})), bad)
        for bad in ({"error": "unknown"}, {"error": None}, {"error": True},
                    {"error": 1}, {"error": ["network"]}, {"error": {"kind": "network"}},
                    {}, [], "network", None):
            self.assertTrue(list(validate.iter_errors({"steps": [{"sync": bad}]})), bad)
        for result in ("ok", "no_changes", "normal_sync", "full_sync", "full_upload", "full_download"):
            self.assertEqual(list(validate.iter_errors({"steps": [{"sync": {"result": result}}]})), [])

    def test_parallel_and_serial_report_all_errors_in_identical_order(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            conf = root / 'conformance'
            (conf / 'schema').mkdir(parents=True)
            (conf / 'scenarios').mkdir()
            (conf / 'goldens').mkdir()
            (conf / 'goldens-apple').mkdir()
            for name in ('scenario', 'observation', 'golden', 'area', 'deviations', 'overlay'):
                schema = {'$schema': 'https://json-schema.org/draft/2020-12/schema',
                          '$id': f'https://schema.example.invalid/{name}', 'type': 'object',
                          'required': ['count'], 'properties': {'count': {'type': 'integer'}}}
                (conf / 'schema' / f'{name}.schema.json').write_text(json.dumps(schema))
            for name, value in [('a', {'count': 1, 'text': 'line\u2028next'}),
                                ('b', {'count': 'wrong'}), ('c', {}), ('d', {'count': 2})]:
                (conf / 'scenarios' / f'{name}.json').write_text(json.dumps(value))
            (conf / 'goldens' / 'e.json').write_text('broken')
            (conf / 'goldens-apple' / 'f.json').write_text('{"count": 3}')
            cli = root / 'cli'
            cli.write_text('#!' + sys.executable + '\nimport json,sys\nfrom pathlib import Path\n'
                           'args=sys.argv[3:]\n'
                           'with Path(__file__).with_suffix(".calls").open("a") as calls:\n'
                           ' calls.write(json.dumps(args)+"\\n")\n'
                           'mode=args[0]=="--observation-json"\n'
                           'assert args[int(mode)]=="--"\n'
                           'for filename in args[int(mode)+1:]:\n'
                           ' assert (Path(filename).parent.name in ("goldens", "goldens-apple"))==mode\n'
                           ' try: item={"value":json.loads(Path(filename).read_text())}\n'
                           ' except ValueError: item={"error":"invalid fixture"}\n'
                           ' item["path"]=filename\n'
                           ' print(json.dumps(item,ensure_ascii=False))\n')
            cli.chmod(0o755)
            reports = []
            for jobs in (1, 4):
                report = root / f'report-{jobs}.json'
                out = subprocess.run([sys.executable, '-B', '-I', str(SCRIPT), '--cli', str(cli),
                                      '--conformance', str(conf), '--report', str(report),
                                      '--jobs', str(jobs)], capture_output=True, text=True)
                self.assertEqual(out.returncode, 1, out.stderr)
                self.assertTrue(report.exists(), out.stderr)
                reports.append(json.loads(report.read_text()))
            self.assertEqual(reports[0], reports[1])
            self.assertEqual(reports[0]['files'], 6)
            self.assertEqual([e['file'] for e in reports[0]['errors']],
                             ['goldens/e.json', 'scenarios/b.json', 'scenarios/c.json'])
            self.assertIn('invalid fixture', reports[0]['errors'][0]['message'])
            self.assertIn('integer', reports[0]['errors'][1]['message'])
            self.assertIn('required', reports[0]['errors'][2]['message'])
            calls = [json.loads(line) for line in cli.with_suffix('.calls').read_text().splitlines()]
            self.assertEqual(len(calls), 4)
            for authored, observations in (calls[:2], calls[2:]):
                self.assertEqual(authored, ['--', *[str(conf / 'scenarios' / f'{name}.json')
                                                    for name in ('a', 'b', 'c', 'd')]])
                self.assertEqual(observations, ['--observation-json', '--',
                                                str(conf / 'goldens-apple/f.json'),
                                                str(conf / 'goldens/e.json')])

    def test_reference_declarations_are_strict_and_resolve_before_execution(self):
        spec = importlib.util.spec_from_file_location("schema_checks", SCRIPT)
        checks = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(checks)
        schema = json.loads((SCRIPT.parents[2] / "conformance/schema/area.schema.json").read_text())
        validate = checks.Draft202012Validator(schema)
        declaration = {"reason": "Scope example.", "spec": "conformance/dsl#D9.4"}
        metadata = {"area": "example", "reference": {"nested/case": declaration}}
        self.assertEqual(list(validate.iter_errors(metadata)), [])
        files = [("scenarios/example/nested/case.yaml", {"value": {}}),
                 ("scenarios/example/ordinary.json", {"value": {}})]
        def resolve(area):
            return checks.check_reference_resolution(files + [("scenarios/example/area.yml", {"value": area})])
        self.assertEqual(resolve(metadata), [])
        self.assertIn("names no scenario", resolve({"reference": {"unknown": declaration}})[0]["message"])
        self.assertIn("also matches active", resolve(dict(metadata, active=["nested/*"]))[0]["message"])
        self.assertIn("also matches active", resolve(dict(metadata, active=["nested/ca?e"]))[0]["message"])
        self.assertEqual(resolve(dict(metadata, active=["ordinary", "nested/[case]"])), [])
        for reference in [None, [], {"nested/*": declaration}, {"nested/case": {}},
                          {"nested/case": dict(declaration, extra=True)},
                          {"nested/case": dict(declaration, reason=" ")},
                          {"nested/case": dict(declaration, reason="two\nlines")},
                          {"nested/case": dict(declaration, spec="R704")}]:
            self.assertTrue(list(validate.iter_errors(dict(metadata, reference=reference))), reference)

    def test_worker_count_must_be_positive(self):
        out = subprocess.run([sys.executable, '-B', '-I', str(SCRIPT), '--cli', 'unused',
                              '--conformance', 'unused', '--report', 'unused', '--jobs', '0'],
                             capture_output=True, text=True)
        self.assertEqual(out.returncode, 2)
        self.assertIn('positive', out.stderr)


if __name__ == '__main__':
    unittest.main()
