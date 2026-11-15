# SPDX-License-Identifier: AGPL-3.0-or-later
import copy
import pathlib
import tempfile
import unittest

from validation import Artifacts, binding, decode, same_value, unique_named, validate
from request import check_request


class ExactObservations(unittest.TestCase):
    def test_bool_is_not_integer(self):
        self.assertFalse(same_value(True, 1))

    def test_tuple_order_is_significant(self):
        self.assertFalse(same_value([1, 2], [2, 1]))

    def test_duplicate_observation_names_are_rejected(self):
        with self.assertRaises(ValueError):
            unique_named([{"name": "stamp", "value": 1}, {"name": "stamp", "value": 2}])

    def test_wide_json_integer_requires_decimal_string(self):
        with self.assertRaises(ValueError):
            decode(b'{"id":9007199254740993}')
        self.assertEqual(decode(b'{"id":"9007199254740993"}')["id"], "9007199254740993")

    def test_duplicate_json_members_are_rejected(self):
        with self.assertRaises(ValueError):
            decode(b'{"id":1,"id":2}')

    def test_nonfinite_exponent_is_rejected(self):
        with self.assertRaises(ValueError): decode(b'{"value":1e999}')

    def test_request_rejects_missing_or_extra_members(self):
        with self.assertRaises(ValueError): check_request({})
        with self.assertRaises(ValueError):
            check_request({'format':'meguri-sync-request-v1','operation':'smoke','peer':'reference','client':'desktop','command':'unexpected'})

    def test_request_preserves_actual_target(self):
        value = {'format':'meguri-sync-request-v1','operation':'smoke','peer':'reference','client':'mobile'}
        self.assertEqual(check_request(value)['client'], 'mobile')


class EvidenceValidation(unittest.TestCase):
    """Made-up validator documents; these are never reference evidence."""
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.store = Artifacts(self.temp.name)
        self.schema = decode(pathlib.Path('/opt/reference/schema.json').read_bytes())
        zero = '0' * 64
        self.client = {"kind": "desktop", "version": "26.09.4", "build": "validator-fixture", "runtime_sha256": zero}
        self.server = {"kind": "reference_runtime", "version": "26.09.4", "build": "validator-fixture", "runtime_sha256": zero}
        input_bytes = b'{"synthetic":"validator-only"}'
        input_hash = self.store.put(input_bytes)
        fixture_hash = self.store.put_json({"format": "meguri-sync-fixtures-v1", "entries": [{"sha256": input_hash, "bytes": len(input_bytes)}]})
        case = {"id": "P-T01:validator-only", "family": "P-T01", "classification": "current_probe",
                "spec_ids": ["sync/server-transport#T2"], "variants": ["identity"],
                "actions": [{"actor": "client_a", "operation": "observe", "description": "Validator synthetic input", "fixture_sha256": input_hash}],
                "expected": [{"name": "version", "value": "26.09.4"}], "observed": [{"name": "version", "value": "26.09.4"}],
                "status": "matched", "fixture_sha256": fixture_hash,
                "lane": {"kind": "reference", "client": self.client, "server": self.server}}
        self.manifest = {"format": "meguri-sync-server-v1", "baseline_release": "26.09.3", "document_kind": "evidence",
                         "source_commit": "0" * 40, "batch_base": "1" * 40, "source_sha256": zero, "lock_sha256": zero,
                         "mg_job": "sync-server-reference", "arguments": ["selftest"],
                         "expected_case_count": 1, "selected_case_count": 1, "completed_case_count": 1, "cases": [case]}
        self.contract = {"format": "meguri-sync-selection-v1", "lane_kind": "reference", "cases": [
            {key: copy.deepcopy(case[key]) for key in ("id", "family", "classification", "spec_ids", "variants", "expected")} | {"client_kind": "desktop"}]}
        self.invocation = {key: copy.deepcopy(self.manifest[key]) for key in ("source_commit", "batch_base", "source_sha256", "lock_sha256", "mg_job", "arguments")}
        self.invocation.update(run_id="validator-only", author_session="validator-author", selection=[case["id"]])
        self.rebind()

    def rebind(self):
        case = self.manifest["cases"][0]
        current = binding(self.manifest, case)
        self.runtime = {"format": "meguri-sync-runtime-v1", "client": self.client, "server": self.server, 'lock_sha256': self.manifest['lock_sha256']}
        runtime = self.store.put_json(self.runtime)
        observed = self.store.put_json({"format": "meguri-sync-neutral_observation-v1", "binding": current})
        receipt_value = {"format": "meguri-sync-mg_receipt-v1", "binding": current, "run_id": "validator-only",
                         "selection": self.invocation["selection"], "completed_ids": [case["id"]], "runtime_receipt_sha256": runtime}
        if case['status'] == 'sealed':
            result = {'job':'sync-server-reference', 'run_id':'validator-only', 'status':'pass', 'exit_code':0, 'dry_run':False,
                      'args':self.manifest['arguments'], 'commit':{'sha':self.manifest['source_commit'],'dirty':False},
                      'details':{'reference_invocation':self.invocation}}
            receipt_value['final_result_sha256'] = self.store.put_json(result)
        receipt = self.store.put_json(receipt_value)
        case["artifacts"] = [{"role": role, "sha256": digest} for role, digest in (
            ("synthetic_input", case["fixture_sha256"]), ("neutral_observation", observed), ("mg_receipt", receipt))]

    def check(self, credit=True):
        return validate(self.manifest, self.contract, self.schema, self.store, self.invocation, self.runtime, credit)

    def test_current_matching_evidence_is_accepted(self):
        self.assertTrue(self.check()["credit"])

    def test_duplicate_ids_fail_even_with_adjusted_declared_counts(self):
        self.manifest["cases"] *= 2
        self.manifest.update(expected_case_count=2, selected_case_count=2, completed_case_count=2)
        with self.assertRaises(ValueError): self.check()

    def test_missing_variant_cannot_reduce_expected_count(self):
        another = copy.deepcopy(self.contract["cases"][0])
        another["id"] = "P-T01:another-required-target"
        another["client_kind"] = "mobile"
        self.contract["cases"].append(another)
        with self.assertRaises(ValueError): self.check()

    def test_family_prefix_mismatch_fails(self):
        self.manifest["cases"][0]["family"] = "P-T02"
        with self.assertRaises(ValueError): self.check()

    def test_mismatch_cannot_claim_matched(self):
        self.manifest["cases"][0]["observed"][0]["value"] = "26.09.3"
        self.rebind()
        with self.assertRaises(ValueError): self.check()

    def test_changed_contract_expectation_is_rejected(self):
        self.manifest["cases"][0]["expected"][0]["value"] = "made-up"
        with self.assertRaises(ValueError): self.check()

    def test_identical_values_cannot_claim_different(self):
        self.manifest["cases"][0].update(status="different", reason="test")
        self.rebind()
        with self.assertRaises(ValueError): self.check(False)

    def test_tampered_retained_bytes_fail(self):
        case = self.manifest["cases"][0]
        path = pathlib.Path(self.temp.name) / case["artifacts"][1]["sha256"]
        path.write_bytes(b'{}')
        with self.assertRaises(ValueError): self.check()

    def test_missing_artifact_fails(self):
        case = self.manifest["cases"][0]
        (pathlib.Path(self.temp.name) / case["artifacts"][1]["sha256"]).unlink()
        with self.assertRaises(ValueError): self.check()

    def test_wrong_run_or_source_fails(self):
        self.invocation["run_id"] = "another-run"
        with self.assertRaises(ValueError): self.check()
        self.invocation["run_id"] = "validator-only"
        self.invocation["source_sha256"] = "2" * 64
        with self.assertRaises(ValueError): self.check()

    def test_unavailable_is_not_completed_or_credit(self):
        self.manifest["cases"][0].update(status="unavailable", reason="actual target absent")
        self.manifest["cases"][0].pop("observed")
        self.manifest["completed_case_count"] = 0
        self.assertFalse(self.check(False)["credit"])
        with self.assertRaises(ValueError): self.check()

    def test_stale_or_nonindependent_review_cannot_seal(self):
        case = self.manifest["cases"][0]
        case["status"] = "sealed"
        self.rebind()
        review = {"format": "meguri-sync-review-v1", "binding": binding(self.manifest, case),
                  "outcome": "accepted", "reviewer_session": "validator-author"}
        digest = self.store.put_json(review)
        case["artifacts"].append({"role": "independent_review", "sha256": digest})
        with self.assertRaises(ValueError): self.check()
        review["reviewer_session"] = "independent-validator"
        review["binding"]["source_sha256"] = "2" * 64
        case["artifacts"][-1]["sha256"] = self.store.put_json(review)
        with self.assertRaises(ValueError): self.check()

    def test_pinned_runtime_receipt_is_checked_independently(self):
        self.runtime = copy.deepcopy(self.runtime)
        self.runtime['client']['runtime_sha256'] = '9' * 64
        with self.assertRaises(ValueError): self.check()

    def test_seal_requires_review_and_clean_actual_mg_result(self):
        case = self.manifest['cases'][0]
        case['status'] = 'sealed'
        self.rebind()
        wanted = binding(self.manifest, case)
        batch = {'format':'meguri-sync-review-batch-v1','outcome':'accepted','role':'oracle-maintainer',
                 'reviewer_session':'independent-validator', 'bindings':[wanted]}
        review = {'format':'meguri-sync-review-v1', 'binding':wanted, 'outcome':'accepted',
                  'reviewer_session':'independent-validator','review_batch_sha256':self.store.put_json(batch)}
        case['artifacts'].append({'role':'independent_review','sha256':self.store.put_json(review)})
        self.assertTrue(self.check()['credit'])
        receipt = self.store.json(case['artifacts'][2]['sha256'])
        result = self.store.json(receipt['final_result_sha256'])
        result['commit']['dirty'] = True
        receipt['final_result_sha256'] = self.store.put_json(result)
        case['artifacts'][2]['sha256'] = self.store.put_json(receipt)
        with self.assertRaises(ValueError): self.check()
