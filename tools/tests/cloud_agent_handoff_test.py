#!/usr/bin/env python3
"""Generated boundary evidence only: no signed VM, SDK, or acceptance proof."""

import copy
import importlib.util
import io
import json
import os
import plistlib
import shlex
import signal
import struct
import threading
from types import SimpleNamespace
from pathlib import Path
import subprocess
import sys
import tempfile
import shutil
import unittest
from unittest.mock import patch

import cloud_agent_install_test as delivery
import cloud_agent_test as fixtures

REAL_RUN = subprocess.run

spec = importlib.util.spec_from_file_location("cloud_agent_handoff", Path(__file__).resolve().parents[1] /
                                              "scripts/cloud-agent-handoff.py")
h = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = h
spec.loader.exec_module(h)
h.i, h.v, h.need, h.strict = delivery.install, delivery.v, delivery.v.need, delivery.install.strict
v = h.v


class EvidenceDouble(delivery.TransportDouble):
    """Generated observations only; none represents actual OS execution."""
    def __init__(self, owner):
        super().__init__(owner.case)
        self.owner = owner
        self.before_copy = self.after_copy = None
        self.copies = 0
        self.container = str(owner.guest / "actual-simulator-data")
        Path(self.container).mkdir(mode=0o700)
        self.support = owner.guest / "observed-mac-support"
        self.support.mkdir(mode=0o700)
        self.observation_mutation = None
        self.setup_events = []

    def call(self, operation, *args, **kwargs):
        if operation == "simctl" and args[:2] == ("get_app_container", delivery.UDID):
            self.events.append((operation, args))
            self.owner.assertEqual(args[2], self.owner.fixture.info["CFBundleIdentifier"])
            self.owner.assertIn(args[3], ("app", "data"))
            return ((self.container if args[3] == "data" else self.installed) + "\n").encode()
        return super().call(operation, *args, **kwargs)

    def guest(self, action, *args):
        if action == "inventory-pins":
            self.events.append((action, args))
            entries, pins = v.snapshot(str(self.owner.root / "build"), str(
                self.owner.fixture.product if args[-1] == self.installed else self.owner.fixture.products))
            return json.loads(json.dumps({"path": args[-1], "entries": entries, "pins": pins}))
        return super().guest(action, *args)

    def setup(self, action, value):
        self.setup_events.append((action, copy.deepcopy(value)))
        if action == "support":
            return h.guest_support(**value)
        self.owner.assertEqual(action, "preflight")
        m = value
        products = m["products"]["path"]
        def bundle(role):
            source = m[role]
            path = products + "/" + source["path"]
            return {"root": path, "canonical_root": path, "bundle_id": source["bundle_id"],
                    "executable": path + "/" + source["executable"]}
        host, agent = bundle("app"), bundle("agent")
        paths = [host["executable"], agent["executable"], host["executable"] + ".debug.dylib"]
        result = {"protocol_version": 1, "scope": "PATH-PREPARATION-ONLY", "application_support": str(self.support),
                  "application_support_chain": h.guest_support(root=str(self.support))["chain"],
                  "host": host, "agent": agent, "app_build": m["app_build"], "os_build": self.owner.fixture.expected["os_build"],
                  "executable_path": paths[0], "anchor_image_path": paths[1], "main_image_path": paths[0],
                  "images": [{"path": path, "cpu": 0x100000c, "subtype": 0, "uuid": (bytes([n]) * 16).hex()}
                             for n, path in enumerate(paths, start=1)]}
        if self.observation_mutation:
            self.observation_mutation(result)
        return result

    def evidence(self, root, parent, label, header, files, copy, expected_root=None):
        self.owner.assertEqual(root, str(self.owner.run.parents[3]))
        originals = {"run.json": header, **files}
        if copy:
            self.copies += 1
            if self.before_copy:
                self.before_copy()
        for original in originals.values():
            original.check()
        metadata = {name: {k: original.reference[k] for k in ("sha256", "bytes")}
                    for name, original in originals.items()}
        raw = h.i.raw_json(metadata)
        if copy:
            raw += b"".join(originals[name].raw for name in ("run.json", *h.ROLES))
        result = h.guest_evidence(root, parent, label, io.BytesIO(raw), copy, expected_root)
        if copy and self.after_copy:
            self.after_copy()
        for original in originals.values():
            original.check()
        return json.loads(json.dumps(result))


class HandoffTests(unittest.TestCase):
    def setUp(self):
        self.case = delivery.DeliveryTests()
        self.case.setUp()
        self.addCleanup(self.case.doCleanups)
        self.fixture, self.root, self.directory = self.case.fixture, self.case.root, self.case.directory
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.guest = Path(self.temporary.name).resolve()
        self.run = self.guest / "observed-mac-support/Meguri/CloudKitSigned" / fixtures.RUN / "slot1"
        self.vm = EvidenceDouble(self)
        self.addCleanup(patch.stopall)
        # The generated run ids name no actual MG processes. Simulate reaping
        # only for this control-flow double; test exited() independently below.
        patch.object(h.os, "kill", side_effect=ProcessLookupError).start()

    def prepare(self, simulator=False, kind="iphone", preflight=True, selection=None):
        if simulator:
            self.case.simulator()
            if kind == "ipad":
                self.fixture.use_simulator(kind)
                self.case.manifest = json.loads(self.fixture.manifest_path.read_bytes())
                self.case.vm.delivered = copy.deepcopy(self.case.manifest["products"])
                self.case.device.update(name="Meguri-Pool-ipad-1", deviceTypeIdentifier=next(
                    d["device_type"] for d in self.case.pins["devices"] if d["kind"] == kind))
                self.case.lease["args"] = ["acquire", kind]
                self.case.lease["details"]["device"]["kind"] = kind
                self.case.lease["details"]["lease"]["kind"] = kind
                self.case.save_lease()
        # Minimal generated Mach-O images bind actual LC_UUID parsing, never
        # a success flag or an injected claim of actual OS execution.
        m = self.case.manifest
        products = self.fixture.products
        paths = [products / m["app"]["path"] / m["app"]["executable"],
                 products / m["agent"]["path"] / m["agent"]["executable"]]
        paths.append(Path(str(paths[0]) + ".debug.dylib"))
        for n, path in enumerate(paths, start=1):
            path.write_bytes(struct.pack("<8I", 0xfeedfacf, 0x100000c, 0, 2, 1, 24, 0, 0) +
                             struct.pack("<2I", 0x1b, 24) + bytes([n]) * 16)
        self.fixture.expected["executable_sha256"] = v.digest(paths[0].read_bytes())
        plan = plistlib.loads(self.fixture.xctestrun.read_bytes())
        plan["TestConfigurations"][0]["TestTargets"][0]["OnlyTestIdentifiers"] = (
            copy.deepcopy(selection) if selection is not None else
            ["CloudKitSoakTests"] + (["CloudKitPathPreflightTests"] if preflight else []))
        self.fixture.xctestrun.write_bytes(plistlib.dumps(plan))
        self.case.manifest = self.fixture.refresh_manifest()
        self.case.vm.delivered = copy.deepcopy(self.case.manifest["products"])
        self.run = (Path(self.vm.container) / "Library/Application Support" if simulator else self.vm.support) / "Meguri/CloudKitSigned" / fixtures.RUN / "slot1"
        # Exercise noncanonical original JSON bytes, including the header's
        # exact digest of a finalized build result with complete MG metadata.
        build_path = self.fixture.results / "build-app" / fixtures.BUILD_RUN / "result.json"
        build_path.write_bytes((json.dumps(self.fixture.build, indent=3) + "\n\n").encode())
        self.fixture.expected["build_receipt_sha256"] = v.digest(build_path.read_bytes())
        self.case.save_template()
        self.case.freeze()
        report = self.case.deliver()
        self.receipt = self.fixture.envelope("cloudkit-agent-install", fixtures.INSTALL_RUN)
        self.receipt["args"] = ["--parent-run", fixtures.RUN, "--header-template", str(self.case.template)]
        if simulator:
            self.receipt["args"] += ["--lease-run", delivery.LEASE_RUN]
        self.receipt["artifacts"] = [{"kind": "file" if ref["path"].endswith(".xctestrun") else "json",
                                      "path": ref["path"]} for ref in report["artifacts"]]
        self.receipt["details"] = {"agent_deployment": report["agent_deployment"],
                                   "outcome": "delivery_only", "scope": "DELIVERY-ONLY"}
        self.save_receipt()
        return report

    def save_receipt(self):
        (self.directory / "result.json").write_bytes((json.dumps(self.receipt, indent=2) + "\n").encode())

    def handoff(self, lease_check=lambda: None):
        return h.handoff_product_evidence(str(self.root), fixtures.RUN, fixtures.INSTALL_RUN, self.vm, lease_check)

    def assert_no_publication(self):
        self.assertFalse((self.run / "product-evidence").exists())
        self.assertEqual(self.vm.copies, 0)

    def test_exact_four_original_roles_and_header_with_complete_finalized_references(self):
        self.prepare()
        result = self.handoff()
        self.assertEqual(result, {"scope": "PRODUCT-EVIDENCE-ONLY", "outcome": "product_evidence_delivered",
                                  "signed_execution": False})
        originals = [self.fixture.results / "build-app" / fixtures.BUILD_RUN / "result.json",
                     self.fixture.manifest_path, self.directory / "result.json", self.directory / "agent-deployment.json"]
        self.assertEqual(set(p.name for p in (self.run / "product-evidence").iterdir()), set(h.ROLES))
        for role, original in zip(h.ROLES, originals):
            self.assertEqual((self.run / "product-evidence" / role).read_bytes(), original.read_bytes())
        self.assertEqual((self.run / "run.json").read_bytes(), (self.directory / "run.json").read_bytes())
        self.assertNotIn(self.vm.selection["ssh"], json.dumps(result))
        self.assertEqual(self.vm.copies, 1)

    def test_existing_admitted_header_is_preserved_and_second_copy_refuses(self):
        self.prepare()
        self.run.mkdir(parents=True, mode=0o700)
        path = self.run / "run.json"
        path.write_bytes((self.directory / "run.json").read_bytes())
        before = path.stat().st_ino, path.read_bytes()
        self.handoff()
        self.assertEqual((path.stat().st_ino, path.read_bytes()), before)
        with self.assertRaisesRegex(v.Refused, "run-not-prelaunch"):
            self.handoff()

    def test_result_must_be_complete_passing_clean_and_the_exact_child(self):
        self.prepare()
        original = copy.deepcopy(self.receipt)
        for mutation in (lambda r: r.pop("finished_at"), lambda r: r.update(status="warn"),
                         lambda r: r.update(dry_run=True), lambda r: r.update(exit_code=1),
                         lambda r: r["commit"].update(dirty=True),
                         lambda r: r["commit"].update(sha="b" * 40),
                         lambda r: r.update(run_id=fixtures.BUILD_RUN),
                         lambda r: r.update(artifacts=[]),
                         lambda r: r["args"].extend(["--parent-run", fixtures.BUILD_RUN]),
                         lambda r: r["details"]["agent_deployment"].update(sha256="0" * 64)):
            with self.subTest(mutation=mutation):
                self.receipt = copy.deepcopy(original)
                mutation(self.receipt)
                self.save_receipt()
                with self.assertRaises((v.Refused, v.Invalid)):
                    self.handoff()
                self.assert_no_publication()

    def test_marker_and_in_progress_results_never_supply_finalization(self):
        self.prepare()
        for name in (".mg", "result.json.tmp"):
            path = self.directory / name
            path.mkdir() if name == ".mg" else path.write_text("in progress")
            with self.assertRaisesRegex(v.Refused, "receipt-in-progress"):
                self.handoff()
            path.rmdir() if name == ".mg" else path.unlink()
        with patch.object(h.os, "kill", return_value=None):
            with self.assertRaisesRegex(v.Refused, "receipt-in-progress"):
                self.handoff()
        (self.directory / "result.json").unlink()
        (self.directory / "finalized").write_text("finished")
        with self.assertRaises((v.Refused, OSError)):
            self.handoff()
        self.assert_no_publication()

    def test_receipt_declaration_and_header_byte_changes_refuse(self):
        self.prepare()
        path = self.directory / "run.json"
        path.write_bytes(path.read_bytes() + b"\n")
        with self.assertRaisesRegex(v.Refused, "deployment-binding"):
            self.handoff()
        self.assert_no_publication()

    def test_build_hash_original_manifest_and_declaration_mismatches_refuse(self):
        self.prepare()
        path = self.fixture.manifest_path
        original = path.read_bytes()
        path.write_bytes(original + b"\n")
        with self.assertRaisesRegex(v.Refused, "artifact-bytes"):
            self.handoff()
        path.write_bytes(original)
        self.receipt["artifacts"].append(copy.deepcopy(self.receipt["artifacts"][0]))
        self.save_receipt()
        with self.assertRaisesRegex(v.Refused, "artifact-declaration"):
            self.handoff()
        self.assert_no_publication()

    def test_symlink_hardlink_and_repeated_roles_refuse(self):
        self.prepare()
        path = self.directory / "agent-deployment.json"
        original = path.read_bytes()
        saved = path.with_name("saved.json")
        path.rename(saved)
        path.symlink_to(saved)
        with self.assertRaises((v.Refused, OSError)):
            self.handoff()
        path.unlink()
        os.link(saved, path)
        with self.assertRaisesRegex(v.Refused, "file-authority"):
            self.handoff()
        path.unlink()
        path.write_bytes(original)
        self.receipt["details"]["agent_deployment"] = self.fixture.build["details"]["agent_build"]
        self.save_receipt()
        with self.assertRaisesRegex(v.Refused, "aliased-role"):
            self.handoff()
        self.assert_no_publication()

    def test_source_file_and_directory_replacements_and_permission_changes_refuse(self):
        self.prepare()
        path = self.fixture.manifest_path

        def replace_file():
            saved = path.with_name("replacement.json")
            saved.write_bytes(path.read_bytes())
            saved.replace(path)

        self.vm.before_copy = replace_file
        with self.assertRaisesRegex(v.Refused, "file-changed"):
            self.handoff()
        self.assertFalse(self.run.exists())
        self.vm.before_copy = lambda: path.chmod(0o666)
        with self.assertRaisesRegex(v.Refused, "file-changed"):
            self.handoff()
        path.chmod(0o644)

        def replace_directory():
            parent = path.parent
            parent.rename(parent.with_name("retained-build"))
            parent.mkdir()

        self.vm.before_copy = replace_directory
        with self.assertRaisesRegex(v.Refused, "directory-changed"):
            self.handoff()

    def test_frozen_vm_instance_and_current_vm_lease_are_required(self):
        self.prepare()
        self.vm.selection["name"] = "different-vm"
        with self.assertRaisesRegex(v.Refused, "destination-frozen-selected"):
            self.handoff()
        self.vm.selection["name"] = self.case.vm.selection["name"]
        changed = dict(self.case.observed, vm_instance=delivery.UDID)
        with patch.object(h.i, "machine", return_value=changed):
            with self.assertRaisesRegex(v.Refused, "destination-frozen-selected"):
                self.handoff()
        with self.assertRaisesRegex(v.Refused, "lease-lost"):
            self.handoff(lambda: v.need(False, "lease-lost"))
        self.assert_no_publication()

    def test_read_only_selection_does_not_fall_back_or_rewrite_map(self):
        self.prepare()
        path = self.case.support / "Meguri/CloudKitSignedPrivate" / fixtures.RUN / "destinations.json"
        before = path.stat().st_ino, path.read_bytes()
        selected = h.select(str(self.root), fixtures.RUN, fixtures.INSTALL_RUN,
                            [dict(self.vm.selection, name="other"), self.vm.selection])
        self.assertEqual(selected, self.vm.selection)
        with self.assertRaisesRegex(v.Refused, "configured-vm"):
            h.select(str(self.root), fixtures.RUN, fixtures.INSTALL_RUN, [dict(selected, name="other")])
        self.assertEqual((path.stat().st_ino, path.read_bytes()), before)
        path.unlink()
        with self.assertRaises(OSError):
            self.handoff()
        self.assertFalse(path.exists())

    def test_reserved_simulator_uses_actual_data_container_and_all_four_original_roles(self):
        self.prepare(simulator=True)
        result = self.handoff()
        self.assertEqual(result["outcome"], "product_evidence_delivered")
        self.assertEqual(set(p.name for p in (self.run / "product-evidence").iterdir()), set(h.ROLES))
        events = [args for operation, args in self.vm.events if operation == "simctl"]
        self.assertIn(("get_app_container", delivery.UDID, self.fixture.info["CFBundleIdentifier"], "data"), events)
        self.assertFalse(any(args[0] in ("install", "boot", "create", "acquire") for args in events))
        with h.inputs(str(self.root), fixtures.RUN, fixtures.INSTALL_RUN) as (_, header, files, _, _):
            self.assertEqual((self.run / "run.json").read_bytes(), header.raw)
            for name, original in files.items():
                self.assertEqual((self.run / "product-evidence" / name).read_bytes(), original.raw)
        self.assertFalse((self.guest / "build").exists())
        self.assertFalse(result["signed_execution"])

    def test_ipad_actual_data_subtree_uses_bound_reserved_udid(self):
        self.prepare(simulator=True, kind="ipad")
        self.assertEqual(self.handoff()["outcome"], "product_evidence_delivered")
        self.assertTrue((self.run / "product-evidence/agent-deployment.json").is_file())
        self.assertTrue(any(args == ("get_app_container", delivery.UDID, self.fixture.info["CFBundleIdentifier"], "data")
                            for operation, args in self.vm.events if operation == "simctl"))

    def test_mobile_checkout_mirror_is_never_a_delivery_or_cleanup_target(self):
        self.prepare(simulator=True)
        mirror = self.guest / "build/mg/cloudkit-signed" / fixtures.RUN / "slot1/product-evidence"
        mirror.mkdir(parents=True)
        sentinel = mirror / h.ROLES[0]
        sentinel.write_bytes(b"existing mirror is not app data")
        before = sentinel.stat().st_ino, sentinel.read_bytes()
        self.assertEqual(self.handoff()["outcome"], "product_evidence_delivered")
        self.assertEqual((sentinel.stat().st_ino, sentinel.read_bytes()), before)
        self.assertEqual(list(mirror.iterdir()), [sentinel])

    def test_finalized_children_do_not_require_the_parent_coordinator_to_exit(self):
        self.prepare()
        parent = self.fixture.results / "cloudkit-signed" / fixtures.RUN
        parent.mkdir(parents=True, exist_ok=True)
        (parent / ".mg").mkdir()
        result = self.handoff()
        self.assertEqual(result["outcome"], "product_evidence_delivered")

    def test_stale_simulator_lease_wrong_target_and_relabelled_receipt_refuse(self):
        self.prepare(simulator=True)
        original = copy.deepcopy(self.case.lease)
        for mutation in (lambda r: r["details"]["lease"].update(expires_epoch=1),
                         lambda r: r["details"]["lease"].update(acquired_epoch=1),
                         lambda r: r["details"].update(udid=delivery.IPAD_UDID),
                         lambda r: r["details"]["lease"].update(kind="ipad")):
            self.case.lease = copy.deepcopy(original)
            mutation(self.case.lease)
            self.case.save_lease()
            with self.assertRaises(v.Refused):
                self.handoff()
            self.assert_no_publication()
        self.case.lease = original
        self.case.save_lease()
        self.receipt["args"][-1] = "20261102T000000Z-105"
        self.save_receipt()
        with self.assertRaisesRegex(v.Refused, "destination-lease"):
            self.handoff()

    def test_destination_readback_detects_changed_bytes_and_same_byte_replacement(self):
        self.prepare()

        def replace():
            path = self.run / "product-evidence/build-result.json"
            saved = path.with_name("saved.json")
            saved.write_bytes(path.read_bytes())
            saved.replace(path)

        self.vm.after_copy = replace
        with self.assertRaisesRegex(v.Refused, "evidence-readback-changed"):
            self.handoff()
        # New invocation cannot repair or overwrite an already published set.
        self.vm.after_copy = None
        with self.assertRaisesRegex(v.Refused, "run-not-prelaunch"):
            self.handoff()

    def test_partial_existing_roles_and_live_inputs_are_never_overwritten(self):
        self.prepare()
        self.run.mkdir(parents=True)
        (self.run / "run.json").write_bytes((self.directory / "run.json").read_bytes())
        evidence = self.run / "product-evidence"
        evidence.mkdir()
        path = evidence / h.ROLES[0]
        path.write_bytes(b"partial")
        before = path.stat().st_ino, path.read_bytes()
        with self.assertRaisesRegex(v.Refused, "run-not-prelaunch"):
            self.handoff()
        self.assertEqual((path.stat().st_ino, path.read_bytes()), before)
        path.unlink()
        evidence.rmdir()
        (self.run / "steps.jsonl").write_bytes(b"live")
        with self.assertRaisesRegex(v.Refused, "run-not-prelaunch"):
            self.handoff()

    def test_changed_destination_bytes_and_modes_fail_readback(self):
        self.prepare()

        def change():
            (self.run / "product-evidence/build-result.json").write_bytes(b"{}")

        self.vm.after_copy = change
        with self.assertRaisesRegex(v.Refused, "evidence-readback"):
            self.handoff()
        self.assertEqual(self.vm.copies, 1)

    def test_guest_ancestor_replacement_refuses_before_publication_without_outside_writes(self):
        self.prepare()
        outside = self.guest / "outside"
        outside.mkdir()
        sentinel = outside / "sentinel"
        sentinel.write_bytes(b"untouched")
        with h.inputs(str(self.root), fixtures.RUN, fixtures.INSTALL_RUN) as (_, header, files, _, _):
            originals = {"run.json": header, **files}
            metadata = {name: {k: original.reference[k] for k in ("sha256", "bytes")}
                        for name, original in originals.items()}
            payload = h.i.raw_json(metadata) + b"".join(originals[name].raw for name in ("run.json", *h.ROLES))
            run_parent = self.run.parent

            class Swap(io.BytesIO):
                swapped = False

                def read(stream, count=-1):
                    raw = super().read(count)
                    if raw and not stream.swapped:
                        stream.swapped = True
                        run_parent.rename(run_parent.with_name("retained-parent"))
                        run_parent.symlink_to(outside, target_is_directory=True)
                    return raw

            with self.assertRaises((v.Refused, OSError)):
                h.guest_evidence(str(self.run.parents[3]), fixtures.RUN, "slot1", Swap(payload), True)
            self.assertEqual(list(outside.iterdir()), [sentinel])
            self.assertEqual(sentinel.read_bytes(), b"untouched")
            self.assertFalse((run_parent.with_name("retained-parent") / "slot1").exists())

    def test_private_map_replacement_and_source_change_are_rechecked_after_copy(self):
        self.prepare()
        path = self.case.support / "Meguri/CloudKitSignedPrivate" / fixtures.RUN / "destinations.json"

        def replace():
            saved = path.with_name("replacement.json")
            saved.write_bytes(path.read_bytes())
            saved.chmod(0o600)
            saved.replace(path)

        self.vm.after_copy = replace
        with self.assertRaisesRegex(v.Refused, "file-changed"):
            self.handoff()

    def test_exclusive_publication_refuses_racing_destination_and_retains_stage(self):
        self.prepare()
        publish = h.publish_exclusive

        def race(source, name, destination, leaf):
            os.mkdir(leaf, dir_fd=destination)
            publish(source, name, destination, leaf)

        with patch.object(h, "publish_exclusive", side_effect=race):
            with self.assertRaisesRegex(v.Refused, "publication-conflict"):
                self.handoff()
        self.assertEqual(list(self.run.iterdir()), [])
        stages = list(self.run.parent.glob(".meguri-evidence-*"))
        self.assertEqual(len(stages), 1)
        self.assertEqual(set(p.name for p in (stages[0] / "product-evidence").iterdir()), set(h.ROLES))

    def test_guest_source_executes_the_exact_transport_payload_and_rejects_truncation(self):
        self.prepare()
        with h.inputs(str(self.root), fixtures.RUN, fixtures.INSTALL_RUN) as (_, header, files, _, _):
            originals = {"run.json": header, **files}
            metadata = {name: {k: original.reference[k] for k in ("sha256", "bytes")}
                        for name, original in originals.items()}
            raw = h.i.raw_json(metadata) + b"".join(originals[name].raw for name in ("run.json", *h.ROLES))
            argv = [sys.executable, "-B", "-I", "-c", h.guest_source().decode(), "copy",
                    str(self.run.parents[3]), fixtures.RUN, "slot1"]
            truncated = REAL_RUN(argv, input=raw[:-1], capture_output=True)
            self.assertEqual(truncated.returncode, 1)
            self.assertEqual(truncated.stdout + truncated.stderr, b"")
            self.assertFalse(self.run.exists())
            delivered = REAL_RUN(argv, input=raw, capture_output=True)
            self.assertEqual(delivered.returncode, 0, delivered.stderr)
            self.assertEqual(json.loads(delivered.stdout)["files"], metadata)

    def test_mac_preflight_rejects_loaded_anchor_main_debug_and_bundle_mismatches(self):
        self.prepare()
        for mutation in (
                lambda o: o["host"].update(bundle_id="other.app"),
                lambda o: o["agent"].update(bundle_id="other.tests"),
                lambda o: o.update(anchor_image_path=o["executable_path"]),
                lambda o: o.update(main_image_path=o["anchor_image_path"]),
                lambda o: o["images"][0].update(uuid="0" * 32),
                lambda o: o["images"][1].update(uuid="f" * 32),
                lambda o: o["images"][2].update(subtype=9),
                lambda o: o["images"].pop(),
                lambda o: o["images"].append(dict(o["images"][0])),
                lambda o: o.update(app_build="other"),
                lambda o: o.update(scope="signed-pass"),
                lambda o: o["application_support_chain"][-1].__setitem__(1, 999999)):
            self.vm.observation_mutation = mutation
            with self.subTest(mutation=mutation), self.assertRaises(v.Refused):
                self.handoff()
            self.assert_no_publication()
        self.vm.observation_mutation = None
        self.assertEqual(self.handoff()["outcome"], "product_evidence_delivered")

    def test_both_required_selections_in_source_order_reach_preflight_and_handoff(self):
        self.prepare(selection=["CloudKitSoakTests", "CloudKitPathPreflightTests"])
        self.assertEqual(self.handoff()["outcome"], "product_evidence_delivered")
        actions = [action for action, _ in self.vm.setup_events]
        self.assertEqual(actions[0], "preflight")
        self.assertEqual(actions.count("preflight"), 1)
        self.assertTrue(actions[1:])
        self.assertEqual(set(actions[1:]), {"support"})

    def test_both_required_selections_in_native_order_reach_preflight_and_handoff(self):
        self.prepare(selection=["CloudKitPathPreflightTests", "CloudKitSoakTests"])
        self.assertEqual(self.handoff()["outcome"], "product_evidence_delivered")
        actions = [action for action, _ in self.vm.setup_events]
        self.assertEqual(actions[0], "preflight")
        self.assertEqual(actions.count("preflight"), 1)
        self.assertTrue(actions[1:])
        self.assertEqual(set(actions[1:]), {"support"})

    def test_extra_missing_duplicate_and_malformed_selection_never_reach_setup(self):
        for selection in (["CloudKitSoakTests", "CloudKitSoakTests"],
                          ["CloudKitPathPreflightTests"],
                          ["CloudKitSoakTests", "GeneratedOtherTests"],
                          ["CloudKitSoakTests", "CloudKitPathPreflightTests", "CloudKitSoakTests"],
                          "CloudKitSoakTests,CloudKitPathPreflightTests"):
            with self.subTest(selection=selection):
                with self.assertRaises(v.Refused):
                    self.prepare(selection=selection)
                self.assertEqual(self.vm.setup_events, [])
                self.assert_no_publication()

    def test_missing_mac_preflight_is_missing_code_without_mirror_or_setup_step(self):
        self.prepare(preflight=False)
        result = self.handoff()
        self.assertEqual(result, {"scope": "PRODUCT-EVIDENCE-ONLY", "outcome": "implementation_missing",
                                  "category": "mac-path-preflight-entrypoint", "signed_execution": False})
        self.assertEqual(self.vm.setup_events, [])
        self.assert_no_publication()

    def test_mac_preflight_uses_exact_verified_selection_and_private_attachment_only(self):
        self.assert_guest_preflight_selection(["CloudKitSoakTests", "CloudKitPathPreflightTests"])

    def test_actual_mac_guest_preflight_accepts_native_selection_order(self):
        self.assert_guest_preflight_selection(["CloudKitPathPreflightTests", "CloudKitSoakTests"])

    def assert_guest_preflight_selection(self, selection):
        self.prepare(selection=selection)
        manifest = self.case.manifest
        original = self.fixture.xctestrun.read_bytes()
        observation = self.vm.setup("preflight", manifest)
        calls = []
        def command(argv):
            calls.append(argv)
            if argv[0] == "/usr/bin/xcodebuild":
                self.assertEqual(argv[1], "test-without-building")
                selected = Path(argv[argv.index("-xctestrun") + 1])
                self.assertNotEqual(selected, self.fixture.xctestrun)
                self.assertEqual(selected.parent.stat().st_mode & 0o777, 0o700)
                self.addCleanup(lambda: shutil.rmtree(selected.parent))
                plan = plistlib.loads(selected.read_bytes())
                target = plan["TestConfigurations"][0]["TestTargets"][0]
                self.assertEqual(target["OnlyTestIdentifiers"], ["CloudKitPathPreflightTests/testObserveOSRootAndLoadedImages"])
                self.assertEqual(target["EnvironmentVariables"]["MEGURI_CLOUD_AGENT_PATH_PREFLIGHT"], "1")
                self.assertEqual(target["TestHostPath"], str(self.fixture.product))
                self.assertIn("-only-testing:MeguriSnapshotTests/CloudKitPathPreflightTests/testObserveOSRootAndLoadedImages", argv)
                self.assertNotIn("CloudKitSoakTests", " ".join(argv))
                return b""
            if "summary" in argv:
                return b'{"passedTests":1,"failedTests":0,"skippedTests":0}'
            self.assertIn("attachments", argv)
            output = Path(argv[argv.index("--output-path") + 1])
            output.mkdir(mode=0o700)
            name = "00000000-0000-0000-0000-000000000123.json"
            method = "CloudKitPathPreflightTests/testObserveOSRootAndLoadedImages()"
            (output / "manifest.json").write_text(json.dumps([{"testIdentifier": method, "attachments": [{
                "exportedFileName": name, "suggestedHumanReadableName": "cloud-agent-path-preflight_0_" + name,
                "isAssociatedWithFailure": False}]}]))
            (output / name).write_text(json.dumps(observation))
            return b""
        with patch.object(h, "setup_command", side_effect=command):
            self.assertEqual(h.guest_preflight(manifest), observation)
        self.assertEqual(len(calls), 3)
        self.assertEqual(self.fixture.xctestrun.read_bytes(), original)
        self.assert_no_publication()

    def test_actual_guest_preflight_refuses_incomplete_duplicate_extra_and_malformed_full_sets(self):
        self.prepare()
        original = self.fixture.xctestrun.read_bytes()
        for selection in (["CloudKitSoakTests"], ["CloudKitPathPreflightTests"],
                          ["CloudKitSoakTests", "CloudKitSoakTests"],
                          ["CloudKitPathPreflightTests", "CloudKitPathPreflightTests"],
                          ["CloudKitSoakTests", "GeneratedOtherTests"],
                          ["CloudKitSoakTests", "CloudKitPathPreflightTests", "GeneratedOtherTests"],
                          [], "CloudKitSoakTests", ["CloudKitSoakTests", {}]):
            with self.subTest(selection=selection):
                plan = plistlib.loads(original)
                plan["TestConfigurations"][0]["TestTargets"][0]["OnlyTestIdentifiers"] = selection
                raw = plistlib.dumps(plan)
                self.fixture.xctestrun.write_bytes(raw)
                manifest = copy.deepcopy(self.case.manifest)
                manifest["xctestrun"].update(sha256=v.digest(raw), bytes=len(raw))
                with patch.object(h, "setup_command") as command:
                    with self.assertRaisesRegex(v.Refused, "setup-selection"):
                        h.guest_preflight(manifest)
                    command.assert_not_called()
                self.assertEqual(self.fixture.xctestrun.read_bytes(), raw)
        self.assert_no_publication()

    def test_mobile_data_variant_cannot_be_the_app_bundle(self):
        self.prepare(simulator=True)
        self.vm.container = self.vm.installed
        with self.assertRaisesRegex(v.Refused, "data-container-observation"):
            self.handoff()
        self.assert_no_publication()

    def test_mobile_changed_container_result_and_recreated_support_refuse(self):
        self.prepare(simulator=True)
        original = self.vm.container
        other = self.guest / "replacement-data"
        other.mkdir(mode=0o700)
        self.vm.after_copy = lambda: setattr(self.vm, "container", str(other))
        with self.assertRaisesRegex(v.Refused, "container-changed"):
            self.handoff()
        self.assertFalse((other / "Library").exists())
        self.vm.container = original
        self.vm.after_copy = None
        # First admission has frozen the actual root. Even the same app at a
        # recreated OS root cannot freshen that existing run on another call.
        support = self.run.parents[3]
        support.rename(support.with_name("retained-support"))
        support.mkdir(mode=0o700)
        with self.assertRaisesRegex(v.Refused, "frozen-layout-changed"):
            self.handoff()
        self.assertFalse((support / "Meguri").exists())

    def test_support_replacement_between_resolution_and_copy_refuses_before_run_mkdir(self):
        self.prepare(simulator=True)
        support = self.run.parents[3]
        def replace():
            support.rename(support.with_name("retained-support"))
            support.mkdir(mode=0o700)
        self.vm.before_copy = replace
        with self.assertRaisesRegex(v.Refused, "application-support-changed"):
            self.handoff()
        self.assertFalse((support / "Meguri").exists())

    def test_installed_product_identity_changes_after_copy_refuse(self):
        self.prepare(simulator=True)
        guest = self.vm.guest
        changed = False
        def observe(action, *args):
            value = guest(action, *args)
            if changed and action == "inventory-pins" and args[-1] == self.vm.installed:
                value["pins"][""][1] += 1
            return value
        def replace():
            nonlocal changed
            changed = True
        self.vm.after_copy = replace
        with patch.object(self.vm, "guest", side_effect=observe):
            with self.assertRaisesRegex(v.Refused, "installed-product-changed"):
                self.handoff()

    def test_mobile_partial_publication_is_not_repaired(self):
        self.prepare(simulator=True)
        path = self.run / "product-evidence" / h.ROLES[0]
        path.parent.mkdir(parents=True)
        path.write_bytes(b"original partial")
        before = path.stat().st_ino, path.read_bytes()
        with self.assertRaisesRegex(v.Refused, "run-not-prelaunch"):
            self.handoff()
        self.assertEqual((path.stat().st_ino, path.read_bytes()), before)

    def test_duplicate_or_unexpected_wire_roles_are_rejected(self):
        self.prepare()
        for raw in (b'{"run.json":{},"run.json":{}}\n', b'{"elsewhere":{}}\n'):
            with self.assertRaises(v.Refused):
                h.guest_evidence(str(self.run.parents[3]), fixtures.RUN, "slot1", io.BytesIO(raw), True)
        self.assertFalse(self.run.exists())


class SetupLifecycleTests(unittest.TestCase):
    """Only generated children/transport; no VM or Apple command is executed."""

    def exercise(self, transport, mode, streamed=False):
        events = []
        handlers = {number: object() for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
        original_handlers = dict(handlers)
        output = b'{"generated":true}'
        payload = io.BytesIO(b"generated-private-stream")

        class Child(h.SetupChild):
            def __init__(self):
                self.pid = self._setup_pid = 123456
                self.returncode = None
                self.args = ['generated-original']
                self.alive, self.status = True, None
                self._setup_ready = True
                self._setup_joined = self._setup_reaped = self._setup_invalid = False
                self._setup_start_stamp = None
                self._setup_lock = threading.RLock()
                self._setup_kernel = SimpleNamespace(
                    terminal=lambda pid: None if self.alive else self.status,
                    quiet=lambda pid, status: not self.alive,
                    owned=lambda pid, status, start_stamp: (True, (1, 2)))

            def initial(self):
                events.append("original-launch")
                if mode == "timeout":
                    raise subprocess.TimeoutExpired("generated-original", 600 if transport else 180)
                if mode in ("cancel", "hangup"):
                    h.setup_cancel(signal.SIGTERM if mode == "cancel" else signal.SIGHUP, None)
                self.status = 255 if mode == "lost-ssh" else 0
                self.alive = False

            def communicate(self, input=None, timeout=None):
                events.append(("communicate", input, timeout))
                self.initial()
                h.SetupChild.wait(self, timeout=timeout)

            def wait(self, timeout=None):
                events.append(("wait", timeout))
                if timeout == 180:
                    self.initial()
                elif timeout == 10 and self.alive:
                    raise subprocess.TimeoutExpired("generated-original", timeout)
                return h.SetupChild.wait(self, timeout=timeout)


        self_test = self
        child = Child()

        def popen(argv, **kwargs):
            self.assertTrue(kwargs["start_new_session"])
            kwargs["stdout"].write(output)
            if transport:
                self.assertEqual(argv[:4], ["/bin/bash", "-c", h.i.BRIDGE, "cloud-agent-delivery"])
                self.assertEqual(argv[-2:], ["copy", "generated-private-command"])
                self.assertIs(kwargs["stdin"], payload if streamed else subprocess.PIPE)
            else:
                self.assertEqual(argv, ["generated-setup-command"])
                self.assertFalse(any(key.startswith(("TEST_RUNNER_", "MEGURI_CLOUD_AGENT_"))
                                     for key in kwargs["env"]))
            if mode == "cancel-during-launch":
                events.append("original-launch")
                # Exercise the installed deferral, before Popen returns its
                # original handle; cancellation must still join that child.
                handlers[signal.SIGTERM](signal.SIGTERM, None)
            return child

        def killpg(pid, number):
            self.assertEqual(pid, child.pid)  # No process discovery or unrelated kill.
            self.assertTrue(all(value == signal.SIG_IGN for value in handlers.values()))
            events.append(("kill-original-group", number))
            if number == signal.SIGKILL:
                child.alive = False
                child.status = -signal.SIGKILL

        def reap(pid, options):
            self.assertEqual((pid, options), (child.pid, 0))
            self.assertFalse(child.alive)
            events.append("original-joined")
            return pid, child.status << 8 if child.status >= 0 else -child.status

        def change_handler(number, handler):
            previous = handlers[number]
            handlers[number] = handler
            return previous

        with patch.object(h, "setup_process", side_effect=popen), \
             patch.object(h.os, "killpg", side_effect=killpg), \
             patch.object(h.os, "waitpid", side_effect=reap), \
             patch.object(h.signal, "signal", side_effect=change_handler), \
             patch.dict(h.os.environ, {"TEST_RUNNER_GENERATED": "remove", "MEGURI_CLOUD_AGENT_GENERATED": "remove"}):
            def invoke():
                if transport:
                    selection = {"name": "generated", "ssh": "/generated/ssh", "tart": "/generated/tart", "state": "running"}
                    return h.VM("/generated", selection, lambda: events.append("lease-check")).call(
                        "copy", "generated-private-command", **({"stream": payload} if streamed else {"data": b"generated-input"}))
                return h.setup_command(["generated-setup-command"])

            if mode == "normal":
                self.assertEqual(invoke(), output)
            elif transport and mode in ("timeout", "lost-ssh"):
                with self.assertRaises(h.i.Queued):
                    invoke()
            else:
                with self.assertRaisesRegex(v.Refused, "setup-timeout" if mode == "timeout" else "setup-cancelled"):
                    invoke()

        self.assertEqual(handlers, original_handlers)
        self.assertEqual(events.count("original-launch"), 1)
        self.assertEqual(events.count("original-joined"), 1)
        if mode in ("normal", "lost-ssh"):
            # Successful managed wait has already proved quiet and reaped;
            # cleanup must never signal that now-reusable group id.
            self.assertFalse(any(isinstance(event, tuple) and event[0] == "kill-original-group" for event in events))
            self.assertNotIn(("wait", 10), events)
        else:
            self.assertLess(events.index("original-launch"), events.index(("kill-original-group", signal.SIGTERM)))
            self.assertLess(events.index(("kill-original-group", signal.SIGKILL)), events.index("original-joined"))
            self.assertIn(("wait", 10), events)
        if transport:
            if mode == "cancel-during-launch":
                # Cancellation after handle assignment joins without further IO.
                self.assertEqual([event for event in events
                                  if isinstance(event, tuple) and event[0] == "communicate"], [])
                self.assertEqual(payload.tell(), 0)
            else:
                self.assertIn(("communicate", None if streamed else b"generated-input", 600), events)
            self.assertEqual(events.count("lease-check"), 2 if mode == "normal" else 1)
            if mode == "normal":
                self.assertLess(events.index("original-joined"), len(events) - 1)

    def test_guest_original_is_joined_on_success_cancellation_hangup_and_timeout(self):
        for mode in ("normal", "cancel", "hangup", "timeout", "cancel-during-launch"):
            with self.subTest(mode=mode):
                self.exercise(False, mode)

    def test_original_transport_is_joined_before_return_on_all_exit_paths(self):
        for mode in ("normal", "cancel", "hangup", "timeout", "lost-ssh", "cancel-during-launch"):
            with self.subTest(mode=mode):
                self.exercise(True, mode)

    def test_private_stream_uses_the_same_owned_transport_join(self):
        for mode in ("normal", "timeout", "cancel", "cancel-during-launch"):
            with self.subTest(mode=mode):
                self.exercise(True, mode, streamed=True)

    def test_existing_stop_matcher_covers_supervisor_before_original_xcodebuild_launch(self):
        # Obtain the real supervisor command, rather than inventing a second
        # command selection. The shell witness below uses mvl_stop_tests itself.
        selection = {"name": "generated", "ssh": "/generated/ssh", "tart": "/generated/tart", "state": "running"}
        vm = h.VM("/generated", selection, lambda: None)
        with patch.object(vm, "call", return_value=b"{}") as call:
            vm.setup("preflight", {})
            argv = shlex.split(call.call_args.args[1])
            self.assertEqual(argv[:4], ["/usr/bin/python3", "-B", "-I", "-c"])
            self.assertEqual(argv[4], h.guest_source().decode())
            self.assertEqual(argv[5:], ["preflight", "xcodebuild test-without-building"])
            self.assertRegex(shlex.join(argv), r"xcodebuild[ ]test-without-building")
            vm.setup("support", {})
            self.assertEqual(shlex.split(call.call_args.args[1])[5:], ["support"])


class HandoffLeaseLifecycleTests(unittest.TestCase):
    def witness(self, mode):
        # Model the remote supervisor and transport as files; only the actual
        # MG exit-action order and checked stop/refusal logic run in bash.
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / "bin").mkdir()
            (directory / "unrelated").write_text("unrelated job must survive")

            def program(path, body):
                path.write_text("#!" + sys.executable + "\n" + body)
                path.chmod(0o700)

            program(directory / "bin/python3", r'''
import json, os, signal, sys
from pathlib import Path
p = Path(os.environ["GENERATED_CASE"])
def event(value):
    with (p / "events").open("a") as f: f.write(value + "\n")
if "--select" in sys.argv:
    print(json.dumps({"name":"generated","tart":"/generated/tart","ssh":str(p / "ssh"),"state":"running"}))
else:
    assert (p / "lease").is_dir()
    assert os.environ["MG_MAC_VM_TESTING"] == "1"
    event("original-launch")
    (p / "original").write_text("python preflight 'xcodebuild test-without-building'")
    mode = os.environ["GENERATED_MODE"]
    if mode == "normal":
        (p / "original").unlink()
        event("original-complete")
    event("transport-joined")
    (p / "transport-joined").touch()
    print("generated-private-diagnostic", file=sys.stderr)
    if mode == "cancel":
        os.kill(os.getppid(), signal.SIGTERM)
        sys.exit(143)
    if mode in ("timeout", "lost-ssh"):
        sys.exit(75)
    print('{"scope":"PRODUCT-EVIDENCE-ONLY","outcome":"product_evidence_delivered","signed_execution":false}')
''')
            program(directory / "ssh", r'''
import os, re, shlex, sys
from pathlib import Path
p = Path(os.environ["GENERATED_CASE"])
def event(value):
    with (p / "events").open("a") as f: f.write(value + "\n")
assert (p / "lease").is_dir()
assert (p / "transport-joined").exists()
pattern = shlex.split(sys.argv[1].splitlines()[0])[0].removeprefix("p=")
assert not re.search(pattern, "unrelated-job")
assert (p / "unrelated").read_text() == "unrelated job must survive"
event("checked-drain")
if os.environ["GENERATED_MODE"] == "unavailable":
    sys.exit(255)
if (p / "original").exists():
    assert re.search(pattern, (p / "original").read_text())
    if os.environ["GENERATED_MODE"] == "unknown":
        print("mg-mac-vm: still running")
        sys.exit(0)
    event("stop-original")
    (p / "original").unlink()
    event("join-original")
    print("mg-mac-vm: stopping")
print("mg-mac-vm: none")
''')
            shell = r'''
mg_job() { :; }
MG_ROOT="$1" MG_TOOLS="$1/tools" MG_RUN_DIR="$GENERATED_CASE"
MG_RUN_ID=20261102T010100Z-101 MG_JOB=cloudkit-signed MG_COMMIT_DIRTY=false
. "$MG_TOOLS/mg.d/host.sh"
. "$MG_TOOLS/mg.d/mac-vm.sh"
. "$MG_TOOLS/mg.d/cloud-agent.sh"
original_take="$(declare -f mg_mac_vm_take)"
event() { printf '%s\n' "$1" >>"$GENERATED_CASE/events"; }
mg_detail() { :; }
mg_artifact() { :; }
mg_mac_vm_probe() { MG_MAC_VMS='[]'; }
mg_mac_vm_take() {
  MG_MAC_VM="$(jq -c '.[0]' <<<"$MG_MAC_VMS")"
  MG_LEASE_DIR="$GENERATED_CASE/lease" MG_LEASE_SLOT="$GENERATED_CASE/lease"
  mkdir "$MG_LEASE_DIR" || return 1
  event lease-taken
  mg_on_exit 'event lease-released; rmdir "$MG_LEASE_DIR"'
  mg_on_exit mg__mac_vm_stop_orphans
  export MG_MAC_VM_TESTING
}
mvl_lease_held() { test -d "$1"; }
mg_cloudkit_agent_handoff 20261102T010100Z-102
rc=$?
if [ "$GENERATED_MODE" = unknown ] || [ "$GENERATED_MODE" = unavailable ]; then
  # Next admission must retain the existing stop-before-work refusal even
  # after the previous job's lease was released. No real lease/VM probes.
  eval "$original_take"
  mg__mac_vm_lease() {
    MG_MAC_VM="$(cat "$GENERATED_CASE/selection")"
    MG_LEASE_DIR="$GENERATED_CASE/lease"
    mkdir "$MG_LEASE_DIR"
    event next-take
  }
  mvl_state() { printf 'running\n'; }
  mvl_wait_ready() { return 0; }
  mg__mac_vm_fail() { event next-refused; }
  mg__mac_vm_unreachable() { event next-unavailable; }
  mg__mac_vm_release_queued() { event next-queued; }
  mg__mac_vm_procs() { event forbidden-new-work; }
  mg_mac_vm_copy_in() { event forbidden-new-work; }
  mg_mac_vm_take --no-copy --running-only
  event "next-rc-$?"
fi
exit "$rc"
'''
            (directory / "selection").write_text(json.dumps({"name": "generated", "tart": "/generated/tart",
                "ssh": str(directory / "ssh"), "state": "running"}))
            # The generated guest runs no command: the VM's claim is tested in mac_vm.bats.
            env = dict(os.environ, GENERATED_CASE=str(directory), GENERATED_MODE=mode, TMPDIR=str(directory),
                       MEGURI_MAC_VM_SHARED_CLAIM="0",
                       PATH=str(directory / "bin") + os.pathsep + os.environ["PATH"])
            result = REAL_RUN(["/bin/bash", "-c", shell, "generated-lifecycle", str(Path(__file__).resolve().parents[2])],
                              env=env, capture_output=True, timeout=30)
            events = (directory / "events").read_text().splitlines()
            report = json.loads((directory / "product-evidence-report.json").read_bytes())
            self.assertNotIn(b"generated-private-diagnostic", result.stdout + result.stderr)
            self.assertEqual((directory / "unrelated").read_text(), "unrelated job must survive")
            self.assertEqual(events.count("original-launch"), 1)
            self.assertEqual(events.count("transport-joined"), 1)
            self.assertLess(events.index("lease-taken"), events.index("original-launch"))
            self.assertLess(events.index("transport-joined"), events.index("checked-drain"))
            self.assertLess(events.index("checked-drain"), events.index("lease-released"))
            self.assertNotIn("forbidden-new-work", events)
            if mode in ("normal", "cancel", "timeout", "lost-ssh"):
                self.assertFalse((directory / "original").exists())
                end = "original-complete" if mode == "normal" else "join-original"
                self.assertLess(events.index(end), events.index("lease-released"))
                if mode != "normal":
                    self.assertLess(events.index("stop-original"), events.index("join-original"))
            else:
                self.assertTrue((directory / "original").exists())
                self.assertEqual(events.count("checked-drain"), 2)  # One per admission; no stop retries.
                self.assertIn("next-rc-1" if mode == "unknown" else "next-rc-2", events)
                self.assertIn("next-refused" if mode == "unknown" else "next-unavailable", events)
            self.assertEqual(result.returncode, {"normal": 0, "cancel": 143, "timeout": 75,
                "lost-ssh": 75, "unknown": 1, "unavailable": 75}[mode])
            self.assertEqual(report["outcome"], "product_evidence_delivered" if mode == "normal" else
                             "queued" if mode in ("timeout", "lost-ssh", "unavailable") else "failed_evidence")
            self.assertFalse(report["signed_execution"])

    def test_confirmed_original_stop_and_join_precedes_lease_release(self):
        for mode in ("cancel", "timeout", "lost-ssh"):
            with self.subTest(mode=mode):
                self.witness(mode)

    def test_unconfirmed_stop_refuses_handoff_and_next_vm_run(self):
        for mode in ("unknown", "unavailable"):
            with self.subTest(mode=mode):
                self.witness(mode)

    def test_normal_success_keeps_lease_through_completion_and_final_drain(self):
        self.witness("normal")


if __name__ == "__main__":
    unittest.main()
