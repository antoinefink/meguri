#!/usr/bin/env python3
"""Generated transport doubles: these tests provide NO real delivery evidence."""

import copy
import io
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest
from unittest.mock import patch

import cloud_agent_test as fixtures

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/cloud-agent-install.py"
spec = importlib.util.spec_from_file_location("cloud_agent_install", SCRIPT)
install = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = install
spec.loader.exec_module(install)
# Share the existing synthetic signed-product fixtures, without rerunning or
# duplicating their signature, loader-policy and product-verifier test suite.
install.v = fixtures.agent
install.need = fixtures.agent.need
v = install.v
sys.modules[v.__name__] = v
INSTANCE = "00000000-0000-0000-0000-000000000001"
UDID = "00000000-0000-0000-0000-000000000002"
PROVISIONING = "GENERATED-PROVISIONING-UDID-003"
IPAD_UDID = "00000000-0000-0000-0000-000000000004"
LEASE_RUN = "20261102T000000Z-104"


class TransportDouble:
    def __init__(self, test):
        self.test = test
        self.selection = {"name": "generated-vm", "ssh": "/generated-private-wrapper",
                          "tart": "/generated-private-tart", "state": "running"}
        self.events = []
        self.delivered = copy.deepcopy(test.manifest["products"])
        self.installed = "/generated-private-container/Meguri.app"
        self.bad_copy = self.bad_install = self.install_failure = False

    def guest(self, action, *args):
        self.events.append((action, args))
        if action == "inventory":
            if args[-1] == self.installed:
                entries = v.snapshot(str(self.test.root / "build"), str(self.test.fixture.product))[0]
                if self.bad_install:
                    entries.pop(next(iter(entries)))
                return {"path": self.installed, "entries": entries}
            result = copy.deepcopy(self.delivered)
            if self.bad_copy:
                result["entries"].pop(next(iter(result["entries"])))
            return result
        if action == "lease":
            return copy.deepcopy(self.test.lease["details"]["lease"])
        raise AssertionError("unexpected guest action")

    def copy(self, root, products, entries):
        self.test.assertEqual(root, str(self.test.root))
        self.test.assertEqual(entries, self.test.manifest["products"]["entries"])
        self.call("copy", products)

    def call(self, operation, *args, **kwargs):
        self.events.append((operation, args))
        if operation == "copy":
            self.test.assertEqual(args, (str(self.test.fixture.products),))
            return b""
        if operation != "simctl":
            raise AssertionError("unexpected transport")
        if args == ("list", "devices", "--json"):
            return json.dumps({"devices": {self.test.pins["runtime"]["identifier"]:
                                          [self.test.device]}}).encode()
        if args == ("list", "runtimes", "--json"):
            return json.dumps({"runtimes": [self.test.runtime]}).encode()
        if args == ("getenv", UDID, "SIMULATOR_RUNTIME_BUILD_VERSION"):
            return self.test.fixture.expected["os_build"].encode()
        if args[:2] == ("install", UDID):
            if self.install_failure:
                raise v.Refused("vm-simctl")
            return b""  # Successful real simctl normally emits no stdout.
        if args[:2] == ("get_app_container", UDID):
            return (self.installed + "\n").encode()
        raise AssertionError("unexpected simulator side effect")


class DeliveryTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.AdmissionTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        self.directory = self.fixture.results / "cloudkit-agent-install" / fixtures.INSTALL_RUN
        self.support = self.root / "private-support"
        self.support.mkdir(mode=0o700)
        self.fixture.profile["ProvisionedDevices"] = [PROVISIONING]
        self.fixture.expected["os_build"] = "24A434"
        self.fixture.save_json(self.fixture.header_path, self.fixture.expected)
        self.manifest = json.loads(self.fixture.manifest_path.read_bytes())
        self.observed = {"vm_instance": INSTANCE, "provisioning_udid": PROVISIONING,
                         "boot_epoch": int(time.time()) - 600,
                         "os_build": "24A434", "xcode": "Xcode 27.2\nBuild version generated"}
        self.vm = TransportDouble(self)
        self.template = self.fixture.header_path.with_name("template.json")
        self.save_template()
        self.lease_run = None
        self.patches = [patch.object(v, "source", return_value=fixtures.SHA),
                        patch.object(v.subprocess, "run", side_effect=self.fixture.fake_command),
                        patch.object(install, "machine", return_value=self.observed),
                        patch.object(install.os.path, "expanduser", return_value=str(self.support))]
        for item in self.patches:
            item.start()
            self.addCleanup(item.stop)

    def save_template(self):
        template = dict(self.fixture.expected)
        del template["deployment_receipt"]
        self.fixture.save_json(self.template, template)

    def freeze(self):
        return install.freeze(str(self.root), str(self.directory), str(self.template),
                              fixtures.RUN, fixtures.INSTALL_RUN)

    def deliver(self):
        return install.deliver(str(self.root), str(self.directory), fixtures.RUN,
                               fixtures.INSTALL_RUN, self.lease_run, self.vm)

    def simulator(self):
        self.fixture.use_simulator("iphone")
        self.manifest = json.loads(self.fixture.manifest_path.read_bytes())
        self.vm.delivered = copy.deepcopy(self.manifest["products"])
        self.save_template()
        self.lease_run = LEASE_RUN
        pins_path = Path(__file__).parents[1] / "sim-devices.json"
        self.pins = json.loads(pins_path.read_bytes())
        self.fixture.save_json(self.root / "tools/sim-devices.json", self.pins)
        self.device = {"udid": UDID, "state": "Booted", "isAvailable": True,
                       "name": "Meguri-Pool-iphone-1",
                       "deviceTypeIdentifier": self.pins["devices"][0]["device_type"]}
        self.runtime = {"identifier": self.pins["runtime"]["identifier"], "isAvailable": True,
                        "buildversion": self.pins["runtime"]["build"]}
        self.lease = self.fixture.envelope("sim", LEASE_RUN)
        self.lease.update(args=["acquire", "iphone"])
        self.lease["details"] = {"sim_runner": "vm", "udid": UDID, "device": {
            "udid": UDID, "kind": "iphone"}, "lease": {
                "udid": UDID, "kind": "iphone", "vm": self.vm.selection["name"],
                "worktree": str(self.root), "acquired_epoch": int(time.time()) - 100,
                "expires_epoch": int(time.time()) + 3600}}
        self.save_lease()

    def save_lease(self):
        self.fixture.save_json(self.fixture.results / "sim" / LEASE_RUN / "result.json", self.lease)

    def test_mac_transport_binds_exact_manifest_header_and_selected_xctestrun(self):
        self.freeze()
        result = self.deliver()
        self.assertEqual(result["outcome"], "delivery_only")
        self.assertFalse(result["signed_execution"])
        deployment_ref = result["agent_deployment"]
        self.assertEqual(deployment_ref, v.file_reference(str(self.root / "build"), deployment_ref["path"]))
        deployment = json.loads(Path(deployment_ref["path"]).read_bytes())
        self.assertEqual(set(deployment), {"protocol_version", "job", "run_id", "source_commit", "parent_run",
                         "device", "device_kind", "os_build", "header_sha256", "build_receipt",
                         "build_receipt_sha256", "build_manifest", "delivered_manifest", "xctestrun", "installation"})
        self.assertEqual(deployment["run_id"], fixtures.INSTALL_RUN)
        self.assertEqual(deployment["parent_run"], fixtures.RUN)
        self.assertEqual(deployment["source_commit"], fixtures.SHA)
        self.assertEqual(deployment["header_sha256"], v.digest((self.directory / "run.json").read_bytes()))
        self.assertEqual(deployment["build_receipt_sha256"], self.fixture.expected["build_receipt_sha256"])
        self.assertIsNone(deployment["installation"])
        self.assertEqual(deployment["xctestrun"]["path"], str(self.fixture.xctestrun))
        for ref in [deployment_ref, deployment["build_manifest"], deployment["delivered_manifest"], deployment["xctestrun"]]:
            self.assertIn(ref, result["artifacts"])
        self.assertEqual([a for a, _ in self.vm.events].count("copy"), 1)
        self.assertNotIn("simctl", [a for a, _ in self.vm.events])
        self.assertFalse((self.directory / "product-evidence").exists())
        self.assertEqual(result["product_evidence"], "implementation_missing-finalized-receipt-handoff")
        public = b"".join(p.read_bytes() for p in self.directory.glob("*.json") if p.name != "result.json")
        for private in (INSTANCE, UDID, PROVISIONING, self.vm.installed, self.vm.selection["ssh"], "destinations.json"):
            self.assertNotIn(private.encode(), public)

    def test_freeze_uses_allocated_child_and_rejects_preselected_deployment(self):
        value = json.loads(self.template.read_bytes())
        value["deployment_receipt"] = self.fixture.expected["deployment_receipt"]
        self.fixture.save_json(self.template, value)
        with self.assertRaisesRegex(v.Refused, "header-template"):
            self.freeze()
        self.save_template()
        self.freeze()
        self.assertEqual(json.loads((self.directory / "run.json").read_bytes())["deployment_receipt"],
                         {"job": "cloudkit-agent-install", "run_id": fixtures.INSTALL_RUN})

    def test_wrong_source_build_hash_and_receipt_reference_stop_before_copy(self):
        for key, value in (("source_commit", "b" * 40), ("build_receipt_sha256", "0" * 64),
                           ("build_receipt", {"job": "build-app", "run_id": fixtures.RUN})):
            with self.subTest(key=key):
                self.save_template()
                template = json.loads(self.template.read_bytes())
                template[key] = value
                self.fixture.save_json(self.template, template)
                with self.assertRaises((v.Refused, v.Invalid)):
                    self.freeze()
                (self.directory / "run.json").unlink()
        self.assertEqual(self.vm.events, [])

    def test_changed_host_product_refuses_without_copy(self):
        self.freeze()
        (self.fixture.product / "Contents/MacOS/Meguri.debug.dylib").write_bytes(b"changed")
        with self.assertRaisesRegex(v.Refused, "product-inventory"):
            self.deliver()
        self.assertNotIn("copy", [a for a, _ in self.vm.events])

    def test_substituted_header_deployment_source_and_build_binding_refuse(self):
        self.freeze()
        header_path = self.directory / "run.json"
        original = json.loads(header_path.read_bytes())
        for key, value in (("deployment_receipt", {"job": "cloudkit-agent-install", "run_id": fixtures.RUN}),
                           ("source_commit", "b" * 40),
                           ("build_receipt_sha256", "0" * 64)):
            with self.subTest(key=key):
                header_path.write_text(json.dumps(dict(original, **{key: value})))
                with self.assertRaises(v.Refused):
                    self.deliver()
        self.assertNotIn("copy", [a for a, _ in self.vm.events])

    def test_readback_mismatch_has_no_deployment_or_recopy(self):
        self.freeze()
        self.vm.bad_copy = True
        with self.assertRaisesRegex(v.Refused, "copy-readback"):
            self.deliver()
        self.assertEqual([a for a, _ in self.vm.events].count("copy"), 1)
        self.assertFalse((self.directory / "agent-deployment.json").exists())

    def test_vm_recreated_during_work_refuses(self):
        self.freeze()
        changed = dict(self.observed, vm_instance=UDID)
        with patch.object(install, "machine", side_effect=[self.observed, changed]):
            with self.assertRaisesRegex(v.Refused, "vm-instance-changed"):
                self.deliver()
        self.assertNotIn("copy", [a for a, _ in self.vm.events])

    def test_provisioning_permission_uses_separate_udid_and_rechecks_it(self):
        self.freeze()
        self.fixture.profile["ProvisionedDevices"] = [INSTANCE]
        with self.assertRaisesRegex(v.Refused, "profile-device"):
            self.deliver()
        self.assertNotIn("copy", [a for a, _ in self.vm.events])
        self.fixture.profile["ProvisionedDevices"] = [PROVISIONING]
        changed = dict(self.observed, provisioning_udid="GENERATED-OTHER-PROVISIONING")
        with patch.object(install, "machine", side_effect=[self.observed, changed]):
            with self.assertRaisesRegex(v.Refused, "vm-instance-changed"):
                self.deliver()

    def frozen_three_device_map(self):
        mac = {"device_kind": "mac", "vm": self.vm.selection["name"],
               "vm_instance": INSTANCE, "simulator": None}
        with install.destinations(fixtures.RUN, fixtures.SHA, "A", mac):
            pass
        path = self.support / "Meguri/CloudKitSignedPrivate" / fixtures.RUN / "destinations.json"
        value = json.loads(path.read_bytes())
        value["destinations"] = {"A": mac, "B": dict(mac, device_kind="iphone", simulator={
            "udid": UDID, "lease_receipt": {"job": "sim", "run_id": LEASE_RUN}}),
            "C": dict(mac, device_kind="ipad", simulator={"udid": IPAD_UDID,
            "lease_receipt": {"job": "sim", "run_id": "20261102T000000Z-105"}})}
        # Noncanonical whitespace/order must be preserved byte-for-byte.
        path.write_bytes((json.dumps(value, indent=3) + "\n\n").encode())
        return path, value

    def test_selected_full_map_is_frozen_before_children_and_rechecked(self):
        mac = {"device_kind": "mac", "vm": self.vm.selection["name"], "vm_instance": INSTANCE, "simulator": None}
        entries = {"mac1": mac, "phone1": dict(mac, device_kind="iphone", simulator={
            "udid": UDID, "lease_receipt": {"job": "sim", "run_id": LEASE_RUN}})}
        with install.freeze_destinations(fixtures.RUN, fixtures.SHA, entries) as check:
            path = self.support / "Meguri/CloudKitSignedPrivate" / fixtures.RUN / "destinations.json"
            original = path.read_bytes()
            self.assertEqual(json.loads(original)["destinations"], entries)
            for label, destination in entries.items():
                with install.destinations(fixtures.RUN, fixtures.SHA, label, destination):
                    check()
                self.assertEqual(path.read_bytes(), original)
        changed = copy.deepcopy(entries); changed["phone1"]["simulator"]["udid"] = IPAD_UDID
        with self.assertRaisesRegex(v.Refused, "destination-frozen-map"):
            with install.freeze_destinations(fixtures.RUN, fixtures.SHA, changed): pass
        with self.assertRaises(v.Refused):
            with install.freeze_destinations(fixtures.RUN, fixtures.SHA, entries) as check:
                value = json.loads(path.read_bytes())
                value["destinations"]["phone1"]["simulator"]["udid"] = IPAD_UDID
                path.write_bytes(install.raw_json(value)); check()

    def test_selected_full_map_never_expands_an_existing_single_map(self):
        mac = {"device_kind": "mac", "vm": self.vm.selection["name"], "vm_instance": INSTANCE, "simulator": None}
        with install.destinations(fixtures.RUN, fixtures.SHA, "mac1", mac): pass
        entries = {"mac1": mac, "phone1": dict(mac, device_kind="iphone", simulator={
            "udid": UDID, "lease_receipt": {"job": "sim", "run_id": LEASE_RUN}})}
        with self.assertRaisesRegex(v.Refused, "destination-frozen-map"):
            with install.freeze_destinations(fixtures.RUN, fixtures.SHA, entries): pass

    def test_prefrozen_valid_abc_map_accepts_every_selection_without_rewriting(self):
        path, value = self.frozen_three_device_map()
        original = path.read_bytes()
        inode = path.stat().st_ino
        for label, destination in value["destinations"].items():
            with install.destinations(fixtures.RUN, fixtures.SHA, label, destination):
                self.assertEqual(path.read_bytes(), original)
            self.assertEqual(path.read_bytes(), original)
            self.assertEqual(path.stat().st_ino, inode)

    def test_prefrozen_selected_substitution_and_unselected_invalid_entry_refuse(self):
        path, value = self.frozen_three_device_map()
        original = path.read_bytes()
        selected = value["destinations"]["A"]
        for label, destination in (("B", selected), ("A", value["destinations"]["B"]),
                                   ("D", selected), ("A", dict(selected, vm_instance=UDID))):
            with self.subTest(label=label, destination=destination):
                with self.assertRaisesRegex(v.Refused, "destination-frozen-selected"):
                    with install.destinations(fixtures.RUN, fixtures.SHA, label, destination):
                        pass
                self.assertEqual(path.read_bytes(), original)
        for mutation in (lambda m: m.update(protocol_version=True),
                         lambda m: m.update(run_id=fixtures.INSTALL_RUN),
                         lambda m: m.update(source_commit="b" * 40),
                         lambda m: m["destinations"]["C"].update(vm="/untrusted/route"),
                         lambda m: m["destinations"]["B"]["simulator"]["lease_receipt"].update(job="run")):
            invalid = copy.deepcopy(value)
            mutation(invalid)
            path.write_bytes(install.raw_json(invalid))
            with self.assertRaisesRegex(v.Refused, "destination-(envelope|entry)"):
                with install.destinations(fixtures.RUN, fixtures.SHA, "A", selected):
                    pass

    def test_existing_map_never_relabels_or_changes_vm_instance(self):
        destination = {"device_kind": "mac", "vm": self.vm.selection["name"],
                       "vm_instance": UDID, "simulator": None}
        with install.destinations(fixtures.RUN, fixtures.SHA, "slot1", destination):
            pass
        self.freeze()
        with self.assertRaisesRegex(v.Refused, "destination-frozen-selected"):
            self.deliver()
        with self.assertRaisesRegex(v.Refused, "destination-frozen-selected"):
            with install.destinations(fixtures.RUN, fixtures.SHA, "slot2", destination):
                pass

    def test_private_map_modes_duplicate_keys_and_nofollow_authority(self):
        destination = {"device_kind": "mac", "vm": "generated-vm", "vm_instance": INSTANCE, "simulator": None}
        with install.destinations(fixtures.RUN, fixtures.SHA, "slot1", destination):
            pass
        path = self.support / "Meguri/CloudKitSignedPrivate" / fixtures.RUN / "destinations.json"
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(path.parent.stat().st_mode & 0o777, 0o700)
        original = path.read_bytes()
        for raw in (b'{"job":"a","job":"b"}', b'{"x":NaN}'):
            path.write_bytes(raw)
            with self.assertRaises(v.Refused):
                with install.destinations(fixtures.RUN, fixtures.SHA, "slot1", destination):
                    pass
        path.write_bytes(original)
        path.chmod(0o644)
        with self.assertRaisesRegex(v.Refused, "private-authority"):
            with install.destinations(fixtures.RUN, fixtures.SHA, "slot1", destination):
                pass
        path.chmod(0o600)
        saved = path.with_name("saved.json")
        path.rename(saved)
        path.symlink_to(saved)
        with self.assertRaisesRegex(v.Refused, "product-path"):
            with install.destinations(fixtures.RUN, fixtures.SHA, "slot1", destination):
                pass

    def test_newer_sibling_never_selects_xctestrun(self):
        sibling = self.fixture.products / "Meguri_CloudAgent_macosx_NEW.xctestrun"
        sibling.write_bytes(self.fixture.xctestrun.read_bytes())
        self.manifest = self.fixture.refresh_manifest()
        self.fixture.save_receipts()
        self.save_template()
        self.vm.delivered = copy.deepcopy(self.manifest["products"])
        self.freeze()
        result = self.deliver()
        deployment = json.loads(Path(result["agent_deployment"]["path"]).read_bytes())
        self.assertEqual(deployment["xctestrun"]["path"], str(self.fixture.xctestrun))
        self.assertNotEqual(deployment["xctestrun"]["path"], str(sibling))
        self.assertFalse(any("test-without-building" in str(args) for _, args in self.vm.events))

    def test_simulator_live_checkpoints_observe_existing_guards_without_changing_them(self):
        self.simulator()
        expected = self.fixture.expected
        lease = install.lease_input(str(self.root), expected, self.lease_run)
        stages = []
        install.live_simulator(self.vm, str(self.root), expected, lease, self.observed, checkpoint=stages.append)
        self.assertEqual(stages, ["simulator-lease", "simulator-device", "simulator-runtime", "simulator-os", "simulator-receipt"])
        original_call = self.vm.call
        for output in (None, b"", b"generated-wrong-build"):
            stages = []
            def fail_environment(operation, *args, **kwargs):
                if args[:1] == ("getenv",):
                    if output is None: raise v.Refused("vm-simctl")
                    return output
                return original_call(operation, *args, **kwargs)
            with patch.object(self.vm, "call", side_effect=fail_environment):
                with self.subTest(command_failed=output is None), self.assertRaisesRegex(v.Refused, "vm-simctl|simulator-runtime"):
                    install.live_simulator(self.vm, str(self.root), expected, lease, self.observed, checkpoint=stages.append)
            self.assertEqual(stages[-1], "simulator-os")
            self.assertNotIn("simulator-receipt", stages)

    def test_simulator_real_install_control_flow_observation_bound_to_product(self):
        self.simulator()
        self.freeze()
        result = self.deliver()
        deployment = json.loads(Path(result["agent_deployment"]["path"]).read_bytes())
        proof = json.loads(Path(deployment["installation"]["path"]).read_bytes())
        self.assertEqual(proof["operation"], "mg-vm-simctl-install-and-nofollow-readback")
        self.assertEqual(proof["app"], self.manifest["app"])
        self.assertIn(deployment["installation"], result["artifacts"])
        commands = [args[0] for operation, args in self.vm.events if operation == "simctl"]
        self.assertEqual(commands.count("install"), 1)
        self.assertIn("get_app_container", commands)
        self.assertFalse(set(commands) & {"launch", "create", "delete", "erase", "shutdown", "boot", "privacy"})
        public = Path(deployment["installation"]["path"]).read_bytes()
        self.assertNotIn(UDID.encode(), public)
        self.assertNotIn(self.vm.installed.encode(), public)

    def test_failed_real_install_has_no_pass_or_deployment(self):
        self.simulator()
        self.freeze()
        self.vm.install_failure = True
        with self.assertRaisesRegex(v.Refused, "vm-simctl"):
            self.deliver()
        self.assertFalse((self.directory / "agent-deployment.json").exists())

    def test_missing_actual_container_observation_refuses(self):
        self.simulator()
        self.freeze()
        self.vm.installed = ""
        with self.assertRaisesRegex(v.Refused, "install-observation"):
            self.deliver()

    def test_changed_installed_app_refuses(self):
        self.simulator()
        self.freeze()
        self.vm.bad_install = True
        with self.assertRaisesRegex(v.Refused, "install-readback"):
            self.deliver()

    def test_lease_wrong_kind_owner_expired_and_old_boot_refuse(self):
        self.simulator()
        self.freeze()
        original = copy.deepcopy(self.lease)
        for key, value in (("kind", "ipad"), ("worktree", "/generated-other-checkout"),
                           ("expires_epoch", int(time.time()) - 1),
                           ("acquired_epoch", self.observed["boot_epoch"] - 1)):
            with self.subTest(key=key):
                self.lease = copy.deepcopy(original)
                self.lease["details"]["lease"][key] = value
                self.save_lease()
                with self.assertRaises(v.Refused):
                    self.deliver()
        self.assertNotIn("copy", [a for a, _ in self.vm.events])

    def test_lease_actual_bytes_and_receipt_target_must_match(self):
        self.simulator()
        self.freeze()
        actual = copy.deepcopy(self.lease["details"]["lease"])
        actual["expires_epoch"] += 1
        original = self.vm.guest
        def changed(action, *args):
            return actual if action == "lease" else original(action, *args)
        with patch.object(self.vm, "guest", side_effect=changed):
            with self.assertRaisesRegex(v.Refused, "lease-changed"):
                self.deliver()
        self.lease["details"]["device"]["udid"] = INSTANCE
        self.save_lease()
        with self.assertRaisesRegex(v.Refused, "lease-target"):
            self.deliver()

    def test_runtime_build_type_and_booted_state_refuse(self):
        self.simulator()
        self.freeze()
        original = copy.deepcopy(self.device)
        for key, value in (("state", "Shutdown"), ("deviceTypeIdentifier", "generated-wrong-type")):
            self.device = dict(original, **{key: value})
            with self.assertRaisesRegex(v.Refused, "simulator-kind-state"):
                self.deliver()
        self.device = original
        self.runtime["buildversion"] = "wrong"
        with self.assertRaisesRegex(v.Refused, "simulator-runtime"):
            self.deliver()
        self.assertNotIn("copy", [a for a, _ in self.vm.events])

    def test_host_mutation_after_copy_is_not_repaired(self):
        self.freeze()
        original = self.vm.call
        def mutate(operation, *args, **kwargs):
            result = original(operation, *args, **kwargs)
            if operation == "copy":
                (self.fixture.product / "Contents/MacOS/Meguri.debug.dylib").write_bytes(b"changed")
            return result
        with patch.object(self.vm, "call", side_effect=mutate):
            with self.assertRaisesRegex(v.Refused, "product-inventory"):
                self.deliver()
        self.assertEqual([a for a, _ in self.vm.events].count("copy"), 1)


class BoundaryTests(unittest.TestCase):
    def wire_payload(self, source):
        entries = v.snapshot(str(source.parent), str(source))[0]
        payload = io.BytesIO()
        payload.write(install.raw_json(entries))
        with v.authority(str(source)) as fd:
            install.send_products(fd, entries, payload)
        return entries, payload.getvalue()

    def test_guest_copy_executes_held_descriptor_publication_at_exact_path(self):
        with __import__("tempfile").TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            source, root = base / "host", base / "guest"
            source.mkdir()
            root.mkdir()
            (source / "Debug").mkdir()
            (source / "Debug/input").write_bytes(b"generated-product")
            (source / "CloudAgent.xctestrun").write_bytes(b"original-generated-routing")
            (source / "current").symlink_to("Debug/input")
            entries, payload = self.wire_payload(source)
            destination = root / "build/DerivedData/Build/Products"
            for _ in range(2):
                result = subprocess.run([sys.executable, "-I", "-c", install.guest_source().decode(),
                                         "copy", str(root), str(destination)],
                                        input=payload, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout), {"copied": True})
                self.assertEqual(v.snapshot(str(root), str(destination))[0], entries)
                self.assertEqual((destination / "CloudAgent.xctestrun").read_bytes(),
                                 b"original-generated-routing")
            retained = list(destination.parent.glob(".meguri-delivery-*/previous"))
            self.assertEqual(len(retained), 1)
            self.assertEqual(v.snapshot(str(root), str(retained[0]))[0], entries)

    def test_guest_ancestor_swap_refuses_before_publication_without_outside_writes(self):
        with __import__("tempfile").TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            source, root, outside = base / "host", base / "guest", base / "outside"
            source.mkdir()
            (source / "input").write_bytes(b"new-generated-product")
            root.mkdir()
            outside.mkdir()
            parent = root / "build/derived"
            destination = parent / "Products"
            destination.mkdir(parents=True)
            (destination / "input").write_bytes(b"old-original-product")
            (outside / "Products").mkdir()
            (outside / "Products/input").write_bytes(b"outside-must-survive")
            (outside / "sentinel").write_bytes(b"untouched")
            _, payload = self.wire_payload(source)
            saved = parent.with_name("held-original")
            before = v.snapshot(str(base), str(outside))

            class SwapDuringRead(io.BytesIO):
                swapped = False

                def read(self, count=-1):
                    raw = super().read(count)
                    if raw and not self.swapped:
                        self.swapped = True
                        parent.rename(saved)
                        parent.symlink_to(outside, target_is_directory=True)
                    return raw

            with self.assertRaisesRegex(v.Refused, "copy-authority"):
                install.receive_products(str(root), str(destination), SwapDuringRead(payload))
            self.assertEqual(v.snapshot(str(base), str(outside)), before)
            self.assertEqual((saved / "Products/input").read_bytes(), b"old-original-product")
            self.assertFalse(list(outside.glob(".meguri-delivery-*")))
            self.assertFalse(list(saved.glob(".meguri-delivery-*/previous")))

    def test_host_ancestor_swap_during_reads_refuses_before_transport(self):
        with __import__("tempfile").TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            parent, outside = base / "host", base / "outside"
            source = parent / "Products"
            source.mkdir(parents=True)
            (source / "input").write_bytes(b"generated")
            (outside / "Products").mkdir(parents=True)
            (outside / "Products/input").write_bytes(b"outside-not-read")
            entries = v.snapshot(str(base), str(source))[0]
            before = v.snapshot(str(base), str(outside))
            vm = install.VM(str(base), {"name": "generated", "state": "running",
                                       "tart": "unused", "ssh": "unused"})
            original_read = os.read
            swapped = False

            def swap(fd, count):
                nonlocal swapped
                raw = original_read(fd, count)
                if raw and not swapped:
                    swapped = True
                    parent.rename(base / "held-host")
                    parent.symlink_to(outside, target_is_directory=True)
                return raw

            with patch.object(install.os, "read", side_effect=swap), patch.object(vm, "call") as transport:
                with self.assertRaisesRegex(v.Refused, "product-changed"):
                    vm.copy(str(base), str(source), entries)
            transport.assert_not_called()
            self.assertTrue(swapped)
            self.assertEqual(v.snapshot(str(base), str(outside)), before)

    def test_guest_machine_uses_provisioning_udid_and_only_missing_value_falls_back(self):
        # Execute the real guest payload, replacing only its fixed native probes.
        import plistlib
        for hardware, expected, code in (({"provisioning_UDID": PROVISIONING, "platform_UUID": UDID},
                                           PROVISIONING, 0),
                                          ({"platform_UUID": UDID}, UDID, 0),
                                          ({"provisioning_UDID": "", "platform_UUID": UDID}, None, 1)):
            native = {
                "/usr/sbin/ioreg": plistlib.dumps([{"IOPlatformUUID": INSTANCE}]),
                "/usr/sbin/sysctl": b"{ sec = 1, usec = 0 }",
                "/usr/bin/xcodebuild": b"Xcode 27.2\nBuild version generated\n",
                "/usr/bin/sw_vers": b"24A434\n",
                "/usr/sbin/system_profiler": json.dumps({"SPHardwareDataType": [hardware]}).encode()}
            probe = ("\n_native=" + repr(native) + "\n"
                     "subprocess.run=lambda words,**kw: SimpleNamespace(returncode=0,"
                     "stdout=_native[words[0]],stderr=b'')\n")
            source = install.guest_source().decode().replace("\naction = sys.argv[1]", probe + "\naction = sys.argv[1]")
            result = subprocess.run([sys.executable, "-I", "-c", source, "machine"], capture_output=True)
            self.assertEqual(result.returncode, code, result.stderr)
            if code == 0:
                observed = json.loads(result.stdout)
                self.assertEqual(observed["vm_instance"], INSTANCE)
                self.assertEqual(observed["provisioning_udid"], expected)
                self.assertNotEqual(observed["vm_instance"], observed["provisioning_udid"])

    def test_actual_machine_probe_toolchain_mismatch_queues(self):
        observed = {"vm_instance": INSTANCE, "provisioning_udid": PROVISIONING,
                    "boot_epoch": 1, "os_build": "24A434",
                    "xcode": "Xcode 27.1\nBuild version generated"}
        vm = unittest.mock.Mock()
        vm.guest.return_value = observed
        with patch.object(v, "command", return_value=subprocess.CompletedProcess(
                [], 0, b"Xcode 27.2\nBuild version generated\n", b"")):
            with self.assertRaisesRegex(install.Queued, "xcode-mismatch"):
                install.machine(vm)
        with patch.object(v, "command", side_effect=v.Refused("host-xcode")):
            with self.assertRaisesRegex(install.Queued, "xcode-unavailable"):
                install.machine(vm)

    def test_bridge_uses_existing_ssh_copy_and_simctl_helpers_with_private_selection(self):
        with __import__("tempfile").TemporaryDirectory() as temporary:
            root = Path(temporary)
            module = root / "tools/mg.d/mac-vm.sh"
            module.parent.mkdir(parents=True)
            module.write_text('''MG_MAC_VM=""; MG_SIM_RUNNER=""
mg_mac_vm_ssh_in() { printf 'copy:%s:' "$1"; cat; }
mg_simctl() { printf 'simctl:%s:%s' "$MG_SIM_RUNNER" "$*"; }
''')
            selection = {"name": "generated", "state": "running", "tart": "unused", "ssh": "unused"}
            vm = install.VM(str(root), selection)
            # Both fixture libraries share subprocess; stop outer platform mocks
            # here: the stub shell invokes no native or network commands.
            self.assertEqual(vm.call("simctl", "list", "runtimes", "--json"), b"simctl:vm:list runtimes --json")
            self.assertEqual(vm.call("copy", "generated-command", data=b"generated-input"),
                             b"copy:generated-command:generated-input")

    def test_guest_inventory_executes_reused_nofollow_inspector(self):
        with __import__("tempfile").TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            products = base / "products"
            products.mkdir()
            (products / "input").write_bytes(b"generated")
            result = subprocess.run([sys.executable, "-I", "-", "inventory", str(base), str(products)],
                                    input=install.guest_source(), capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout), {"path": str(products),
                                                        "entries": v.snapshot(str(base), str(products))[0]})


if __name__ == "__main__":
    unittest.main()
