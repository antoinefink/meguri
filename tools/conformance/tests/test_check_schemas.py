#!/usr/bin/env python3
"""The parallel validator must retain every serial diagnostic in file order."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'check_schemas.py'


class SchemaChecks(unittest.TestCase):
    def test_parallel_and_serial_report_all_errors_in_identical_order(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            conf = root / 'conformance'
            (conf / 'schema').mkdir(parents=True)
            (conf / 'scenarios').mkdir()
            (conf / 'goldens').mkdir()
            for name in ('scenario', 'observation', 'golden', 'area', 'deviations', 'overlay'):
                schema = {'$schema': 'https://json-schema.org/draft/2020-12/schema',
                          '$id': f'https://schema.example.invalid/{name}', 'type': 'object',
                          'required': ['count'], 'properties': {'count': {'type': 'integer'}}}
                (conf / 'schema' / f'{name}.schema.json').write_text(json.dumps(schema))
            for name, value in [('a', {'count': 1, 'text': 'line\u2028next'}),
                                ('b', {'count': 'wrong'}), ('c', {}), ('d', {'count': 2})]:
                (conf / 'scenarios' / f'{name}.json').write_text(json.dumps(value))
            (conf / 'goldens' / 'e.json').write_text('broken')
            cli = root / 'cli'
            cli.write_text('#!' + sys.executable + '\nimport json,sys\nfrom pathlib import Path\n'
                           'for filename in sys.argv[3:]:\n'
                           ' try: item={"value":json.loads(Path(filename).read_text())}\n'
                           ' except ValueError: item={"error":"invalid fixture"}\n'
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
            self.assertEqual(reports[0]['files'], 5)
            self.assertEqual([e['file'] for e in reports[0]['errors']],
                             ['goldens/e.json', 'scenarios/b.json', 'scenarios/c.json'])
            self.assertIn('invalid fixture', reports[0]['errors'][0]['message'])
            self.assertIn('integer', reports[0]['errors'][1]['message'])
            self.assertIn('required', reports[0]['errors'][2]['message'])

    def test_worker_count_must_be_positive(self):
        out = subprocess.run([sys.executable, '-B', '-I', str(SCRIPT), '--cli', 'unused',
                              '--conformance', 'unused', '--report', 'unused', '--jobs', '0'],
                             capture_output=True, text=True)
        self.assertEqual(out.returncode, 2)
        self.assertIn('positive', out.stderr)


if __name__ == '__main__':
    unittest.main()
