#!/usr/bin/env python3
"""Generated J2.2.1 checks only: synthetic identities, temporary OS roots.
No native/VM/SDK/build/setup entry or actual owner approval is executed.
"""
import ast
import contextlib
import importlib.util
import io
import json
import os
import copy
import plistlib
from pathlib import Path
import re
import stat
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("generated_cloud_account_setup", ROOT / "tools/scripts/cloud-account-setup.py")
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)
CONTAINER = "iCloud.generated.setup"
CANDIDATE = [41, 12, 255]  # Synthetic test data only.


def encoded(accounts):
    return p.i.raw_json({"protocol_version": 1, "container": CONTAINER, "environment": "development", "accounts": accounts})


class Terminal:
    def __init__(self, result=True, callback=lambda: None):
        self.result, self.callback, self.prompts = result, callback, []
    def approve(self, label, container, changed):
        self.prompts.append((label, container, changed))
        self.callback()
        return self.result


class Transport:
    def __init__(self, fresh=None, fail=None, status="available"):
        self.fresh = CANDIDATE if fresh is None else fresh
        self.fail, self.status, self.actions = fail, status, []
    def request(self, action):
        self.actions.append(action)
        if action == self.fail:
            raise OSError("GENERATED_PRIVATE_SDK_MARKER")
        if action in ("capture", "fresh"):
            return {"status": self.status, "identity": CANDIDATE if action == "capture" else self.fresh,
                    "observation": {"generated_loaded_product": True}}, b""
        if action == "hold":
            return {"observation": {"generated_loaded_product": True}}, b""
        if action == "finish":
            return {"joined": True}, b""
        raise AssertionError(action)


class OwnerSetupPlanSelectionTests(unittest.TestCase):
    """Only pure plan derivation; never owner CLI, account capture or SDK work."""
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.products = Path(self.temp.name).resolve()
        self.path = self.products / "original.xctestrun"

    def fixture(self, selection):
        target = {"BlueprintName": "MeguriSnapshotTests", "OnlyTestIdentifiers": copy.deepcopy(selection),
                  "TestHostPath": "__TESTROOT__/Meguri.app", "EnvironmentVariables": {}, "TestingEnvironmentVariables": {}}
        return self.store({"TestConfigurations": [{"TestTargets": [target]}]})

    def store(self, plan):
        raw = plistlib.dumps(plan); self.path.write_bytes(raw)
        return {"products": {"path": str(self.products)}, "xctestrun": {
            "path": self.path.name, "sha256": p.v.digest(raw), "bytes": len(raw)}}, raw

    def test_actual_owner_plan_accepts_both_full_orders_and_preserves_frozen_bytes(self):
        for selection in (["CloudKitSoakTests", "CloudKitPathPreflightTests"],
                          ["CloudKitPathPreflightTests", "CloudKitSoakTests"]):
            with self.subTest(selection=selection):
                manifest, original = self.fixture(selection)
                raw, private = p.setup_plan(manifest, "/generated/private-owner-run")
                self.assertEqual(raw, original)
                self.assertEqual(self.path.read_bytes(), original)
                target = plistlib.loads(private)["TestConfigurations"][0]["TestTargets"][0]
                self.assertEqual(target["OnlyTestIdentifiers"], ["CloudKitAccountSetupTests/testOwnerSetup"])
                self.assertEqual(target["EnvironmentVariables"]["MEGURI_CLOUD_ACCOUNT_SETUP"], "1")
                self.assertEqual(target["EnvironmentVariables"]["MEGURI_CLOUD_ACCOUNT_DIRECTORY"], "/generated/private-owner-run")
                self.assertEqual(target["TestHostPath"], str(self.products / "Meguri.app"))

    def test_actual_owner_plan_refuses_legacy_missing_extra_duplicate_and_malformed_sets(self):
        for selection in (["CloudKitSoakTests"], ["CloudKitPathPreflightTests"],
                          ["CloudKitSoakTests", "CloudKitSoakTests"],
                          ["CloudKitPathPreflightTests", "CloudKitPathPreflightTests"],
                          ["CloudKitSoakTests", "GeneratedOtherTests"],
                          ["CloudKitSoakTests", "CloudKitPathPreflightTests", "GeneratedOtherTests"],
                          [], "CloudKitSoakTests", ["CloudKitSoakTests", {}]):
            with self.subTest(selection=selection):
                manifest, original = self.fixture(selection)
                with self.assertRaisesRegex(p.v.Refused, "setup-selection"):
                    p.setup_plan(manifest, "/generated/private-owner-run")
                self.assertEqual(self.path.read_bytes(), original)

    def test_owner_plan_keeps_target_configuration_skip_environment_and_hash_guards(self):
        _, original = self.fixture(["CloudKitPathPreflightTests", "CloudKitSoakTests"])
        mutations = [lambda x: x["TestConfigurations"].append(copy.deepcopy(x["TestConfigurations"][0])),
                     lambda x: x["TestConfigurations"][0]["TestTargets"].append(copy.deepcopy(x["TestConfigurations"][0]["TestTargets"][0])),
                     lambda x: x["TestConfigurations"][0]["TestTargets"][0].update(BlueprintName="GeneratedOtherTests"),
                     lambda x: x["TestConfigurations"][0]["TestTargets"][0].update(SkipTestIdentifiers=["CloudKitSoakTests"])]
        for mutate in mutations:
            plan = plistlib.loads(original); mutate(plan); manifest, raw = self.store(plan)
            with self.assertRaisesRegex(p.v.Refused, "setup-selection"):
                p.setup_plan(manifest, "/generated/private-owner-run")
            self.assertEqual(self.path.read_bytes(), raw)
        for key in ("EnvironmentVariables", "TestingEnvironmentVariables"):
            plan = plistlib.loads(original)
            plan["TestConfigurations"][0]["TestTargets"][0][key]["MEGURI_CLOUD_ACCOUNT_GENERATED"] = "generated"
            manifest, raw = self.store(plan)
            with self.assertRaisesRegex(p.v.Refused, "setup-environment"):
                p.setup_plan(manifest, "/generated/private-owner-run")
            self.assertEqual(self.path.read_bytes(), raw)
        manifest, raw = self.store(plistlib.loads(original)); manifest["xctestrun"]["sha256"] = "0" * 64
        with self.assertRaisesRegex(p.v.Refused, "setup-plan"):
            p.setup_plan(manifest, "/generated/private-owner-run")
        self.assertEqual(self.path.read_bytes(), raw)


class SetupDiagnosticTests(unittest.TestCase):
    def test_only_closed_phase_code_kind_can_reach_private_diagnostic(self):
        for error in (p.v.Refused("setup-peer"), p.v.Refused("GENERATED_PRIVATE_MARKER"),
                      OSError("GENERATED_PRIVATE_MARKER"), TypeError("GENERATED_PRIVATE_MARKER")):
            diagnostic=p.SetupDiagnostic();diagnostic.enter("GENERATED_PRIVATE_MARKER")
            out=io.StringIO()
            with contextlib.redirect_stderr(out):diagnostic.emit(error)
            prefix="MG_OWNER_SETUP_DIAGNOSTIC "
            self.assertTrue(out.getvalue().startswith(prefix));value=json.loads(out.getvalue()[len(prefix):])
            self.assertEqual(set(value),{"phase","refusal","kind"});self.assertEqual(value["phase"],"unknown")
            self.assertNotIn("GENERATED_PRIVATE_MARKER",out.getvalue())
            self.assertEqual(value["refusal"],"setup-peer" if isinstance(error,p.v.Refused) and error.args==("setup-peer",) else "unknown")

    def test_main_preserves_public_failure_while_retaining_exact_closed_stage(self):
        def failed(root,run,label,context,diagnostic):
            diagnostic.enter("owner-launch");raise p.v.Refused("setup-peer")
        out,err=io.StringIO(),io.StringIO()
        with mock.patch.object(p.sys,"argv",["generated","--root","/generated","--run-id","20261102T000000Z-1","--label","acct1"]), \
             mock.patch.object(p.os,"dup",return_value=3), \
             mock.patch.object(p.os,"fdopen",return_value=contextlib.nullcontext(io.BytesIO(b"{}"))), \
             mock.patch.object(p.signal,"signal"),mock.patch.object(p,"run",side_effect=failed), \
             contextlib.redirect_stdout(out),contextlib.redirect_stderr(err):
            self.assertEqual(p.main(),1)
        self.assertEqual(json.loads(out.getvalue()),{"scope":"SETUP-ONLY","outcome":"failed_evidence","category":"setup-refused","label":"acct1","signed_execution":False})
        self.assertEqual(json.loads(err.getvalue().removeprefix("MG_OWNER_SETUP_DIAGNOSTIC ")),
                         {"phase":"owner-launch","refusal":"setup-peer","kind":"refused"})

    def test_diagnostic_output_failure_cannot_replace_original_outcome(self):
        diagnostic=p.SetupDiagnostic()
        with mock.patch.object(p.sys.stderr,"write",side_effect=OSError("GENERATED_PRIVATE_MARKER")):
            diagnostic.emit(p.v.Refused("setup-peer"))

    def test_perform_failure_keeps_original_exception_and_does_not_approve(self):
        diagnostic=p.SetupDiagnostic();terminal=Terminal();transport=Transport(fail="capture")
        source=mock.Mock()
        with self.assertRaisesRegex(OSError,"GENERATED_PRIVATE_SDK_MARKER"):
            p.perform("acct1",source,terminal,transport,lambda current=None:None,diagnostic)
        self.assertEqual(diagnostic.phase,"capture");self.assertEqual(terminal.prompts,[])
        source.publish.assert_not_called();self.assertEqual(transport.actions,["capture"])


class Producer(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        os.chmod(self.root, 0o700)
        self.path = self.root / "Meguri/CloudKitSignedPrivate/approved-accounts.json"
    def source(self, accounts=None):
        if accounts is not None:
            self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            os.chmod(self.path.parent.parent, 0o700)
            self.path.write_bytes(encoded(accounts))
            os.chmod(self.path, 0o600)
        source = p.Source(str(self.root), CONTAINER)
        self.addCleanup(source.close)
        return source
    def perform(self, source, label="acct1", terminal=None, transport=None, check=lambda current=None: None):
        terminal, transport = terminal or Terminal(), transport or Transport()
        p.perform(label, source, terminal, transport, check)
        return terminal, transport
    def unchanged(self, original):
        self.assertEqual(self.path.read_bytes() if self.path.exists() else None, original)
    def private_priors(self):
        return [path for path in self.path.parent.glob(".approved-*") if path.name != p.LOCK_NAME]
    def assert_private_prior(self, raw, inode):
        matching = [path for path in self.private_priors() if path.read_bytes() == raw]
        self.assertEqual(len(matching), 1)
        info = matching[0].stat()
        self.assertEqual(info.st_ino, inode)
        self.assertEqual(stat.S_IMODE(info.st_mode), 0o600)
        self.assertEqual(info.st_uid, os.getuid())
        self.assertEqual(info.st_nlink, 1)
        self.assertEqual(stat.S_IMODE(matching[0].parent.stat().st_mode), 0o700)

    def test_creation_idempotence_replacement_and_preservation(self):
        for accounts, label, changed in ((None, "acct1", False), ({"acct1": CANDIDATE}, "acct1", False),
                ({"acct1": [77], "acct2": [88]}, "acct1", True), ({"acct1": [77]}, "acct2", False)):
            with self.subTest(accounts=accounts):
                if self.path.exists():
                    self.path.unlink()
                source = self.source(accounts)
                terminal, transport = self.perform(source, label)
                self.assertEqual(terminal.prompts, [(label, CONTAINER, changed)])
                result = p.approved(self.path.read_bytes(), CONTAINER)["accounts"]
                self.assertEqual(result[label], CANDIDATE)
                for other in set(accounts or {}) - {label}:
                    self.assertEqual(result[other], accounts[other])
                self.assertEqual(transport.actions, ["capture", "fresh", "hold", "hold", "finish"])
                self.assertEqual(stat.S_IMODE(self.path.stat().st_mode), 0o600)
                self.assertEqual(self.path.stat().st_nlink, 1)
                source.close()
                # close is an idempotent context boundary in production too.
                self._cleanups.pop()

    def test_acct2_needs_acct1_and_equal_accounts_refuse_before_prompt(self):
        source, terminal, transport = self.source(), Terminal(), Transport()
        with self.assertRaises(p.s.UserStep):
            self.perform(source, "acct2", terminal, transport)
        self.assertEqual(terminal.prompts, [])
        self.assertFalse(self.path.exists())
        source2 = self.source({"acct1": CANDIDATE})
        with self.assertRaises(p.v.Refused):
            self.perform(source2, "acct2", terminal, Transport())
        self.assertEqual(terminal.prompts, [])
        self.unchanged(encoded({"acct1": CANDIDATE}))

    def test_failed_product_admission_never_looks_up(self):
        source, transport = self.source(), Transport()
        def failed(current=None):
            raise p.v.Refused("generated-product-refusal")
        with self.assertRaises(p.v.Refused):
            self.perform(source, transport=transport, check=failed)
        self.assertEqual(transport.actions, [])
        self.assertFalse(self.path.exists())

    def test_no_write_without_approval_and_fresh_identity(self):
        for terminal, transport in ((Terminal(False), Transport()), (Terminal(), Transport(fresh=[1])),
                (Terminal(), Transport(fail="fresh")), (Terminal(), Transport(status="unavailable"))):
            source = self.source()
            with self.assertRaises((p.v.Refused, OSError)):
                self.perform(source, terminal=terminal, transport=transport)
            self.assertNotIn("finish", transport.actions)
            self.assertFalse(self.path.exists())

    def test_changed_product_vm_lease_during_approval_refuses(self):
        for category in ("product", "vm", "lease", "loaded-image", "source-commit"):
            source, state, transport = self.source(), {"changed": False}, Transport()
            def check(current=None):
                if state["changed"]:
                    raise p.v.Refused("generated-" + category)
            terminal = Terminal(callback=lambda: state.update(changed=True))
            with self.assertRaises(p.v.Refused):
                self.perform(source, terminal=terminal, transport=transport, check=check)
            self.assertEqual(transport.actions, ["capture"])
            self.assertFalse(self.path.exists())

    def test_original_source_replacement_and_absent_source_creation_refuse(self):
        for existing in (False, True):
            if self.path.exists():
                self.path.unlink()
            source = self.source({"acct1": [77]} if existing else None)
            replacement = encoded({"acct1": [99]})
            def mutate():
                temp = self.path.parent / "generated-other"
                temp.write_bytes(replacement)
                os.chmod(temp, 0o600)
                os.replace(temp, self.path)
            with self.assertRaises(p.v.Refused):
                self.perform(source, terminal=Terminal(callback=mutate))
            self.unchanged(replacement)

    def test_invalid_source_types_duplicates_labels_container_and_equal_values(self):
        invalid = [b'{"protocol_version":1,"protocol_version":1}', encoded({"acct1": [True]}),
                   encoded({"acct1": []}), encoded({"acct1": [256]}), encoded({"acct1": [1.0]}),
                   encoded({"acct2": [1]}), encoded({"acct1": [1], "acct2": [1]}),
                   encoded({"acct1": [1], "unknown": [2]}), encoded({"acct1": [1]}).replace(b'development', b'production'),
                   encoded({"acct1": [1]}).replace(CONTAINER.encode(), b'iCloud.other'),
                   encoded({"acct1": [1]}).replace(b'"protocol_version": 1', b'"protocol_version": true')]
        for raw in invalid:
            with self.subTest(raw=raw), self.assertRaises((p.v.Refused, ValueError)):
                p.approved(raw, CONTAINER)

    def test_modes_links_owner_and_binding_refuse(self):
        source = self.source({"acct1": [77]})
        original = self.path.read_bytes()
        os.chmod(self.path, 0o644)
        with self.assertRaises(p.v.Refused):
            source.check()
        self.unchanged(original)
        os.chmod(self.path, 0o600)
        linked = self.path.parent / "generated-link"
        os.link(self.path, linked)
        with self.assertRaises(p.v.Refused):
            p.Source(str(self.root), CONTAINER)
        linked.unlink()
        self.path.unlink()
        self.path.symlink_to(linked)
        with self.assertRaises((p.v.Refused, OSError)):
            p.Source(str(self.root), CONTAINER)
        self.path.unlink()
        self.source({"acct1": [77]})
        os.chmod(self.path.parent, 0o755)
        with self.assertRaises(p.v.Refused):
            p.Source(str(self.root), CONTAINER)
        os.chmod(self.path.parent, 0o700)
        with mock.patch.object(p.os, "getuid", return_value=os.getuid() + 1), self.assertRaises(p.v.Refused):
            p.Source(str(self.root), CONTAINER)

    def test_canonicalize_trusted_root_before_private_suffix(self):
        alias = self.root / "generated-alias"
        physical = self.root / "generated-physical"
        physical.mkdir(mode=0o700)
        alias.symlink_to(physical, target_is_directory=True)
        source = p.Source(str(alias), CONTAINER)
        self.addCleanup(source.close)
        self.assertEqual(source.base.bindings[-1][1], "generated-physical")
        # Private aliases never receive the trusted-root canonicalization.
        source.close()
        private = physical / "Meguri/CloudKitSignedPrivate"
        (private / p.LOCK_NAME).unlink()  # Fixture teardown after authority closes.
        private.rmdir()
        private.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(OSError):
            p.Source(str(alias), CONTAINER)

    def test_cancel_before_publish_preserves_source(self):
        source = self.source({"acct1": [77]})
        original = self.path.read_bytes()
        def cancelled():
            raise p.v.Refused("setup-cancelled")
        with self.assertRaises(p.v.Refused):
            source.publish(source.proposed("acct1", CANDIDATE), cancelled)
        self.assertFalse(source.started)
        self.unchanged(original)

    def test_interrupted_atomic_publication_is_uncertain_once_and_never_rolled_back(self):
        source = self.source({"acct1": [77]})
        original, inode = self.path.read_bytes(), self.path.stat().st_ino
        exchange = p.exchange
        def lost(*args, **kwargs):
            exchange(*args, **kwargs)
            raise OSError("GENERATED_PRIVATE_COMPLETION_MARKER")
        with mock.patch.object(p, "exchange", side_effect=lost) as operation:
            with self.assertRaises(p.PublicationUncertain):
                source.publish(source.proposed("acct1", CANDIDATE), lambda: None)
        self.assertEqual(operation.call_count, 1)
        self.assertEqual(p.approved(self.path.read_bytes(), CONTAINER)["accounts"]["acct1"], CANDIDATE)
        self.assertTrue(source.started)
        self.assert_private_prior(original, inode)

    def test_existing_exchange_succeeds_and_retains_exact_private_prior(self):
        source = self.source({"acct1": [77], "acct2": [88]})
        original, inode = self.path.read_bytes(), self.path.stat().st_ino
        writer = self.path.parent / p.LOCK_NAME
        writer_inode = writer.stat().st_ino
        with mock.patch.object(p, "exchange", wraps=p.exchange) as operation:
            self.perform(source)
        self.assertEqual(operation.call_count, 1)
        self.assertEqual(p.approved(self.path.read_bytes(), CONTAINER)["accounts"],
                         {"acct1": CANDIDATE, "acct2": [88]})
        self.assert_private_prior(original, inode)
        self.assertEqual(writer.stat().st_ino, writer_inode)
        self.assertEqual(stat.S_IMODE(writer.stat().st_mode), 0o600)

    def test_competing_existing_source_at_exchange_is_private_uncertain_and_no_success(self):
        source = self.source({"acct1": [77], "acct2": [88]})
        transport, exchange = Transport(), p.exchange
        competing = encoded({"acct1": [77], "acct2": [99]})
        competing_inode, retained = [], []
        opened = os.open
        def record_open(name, flags, *args, **kwargs):
            fd = opened(name, flags, *args, **kwargs)
            if str(name).startswith(".approved-") and flags & os.O_NOFOLLOW and not flags & os.O_CREAT:
                retained.append(fd)
            return fd
        def conflict(*args):
            temp = self.path.parent / "generated-competing"
            temp.write_bytes(competing)
            os.chmod(temp, 0o600)
            competing_inode.append(temp.stat().st_ino)
            os.replace(temp, self.path)
            exchange(*args)  # Actual OS operation at the last-check race point.
        with mock.patch.object(p, "exchange", side_effect=conflict) as operation, \
             mock.patch.object(p.os, "open", side_effect=record_open):
            with self.assertRaises(p.PublicationUncertain) as raised:
                self.perform(source, transport=transport)
        self.assertEqual(str(raised.exception), "publication-uncertain")
        self.assertEqual(operation.call_count, 1)
        self.assertTrue(source.started)
        self.assertEqual(transport.actions, ["capture", "fresh", "hold"])
        self.assert_private_prior(competing, competing_inode[0])
        self.assertEqual(len(retained), 1)
        self.assertEqual(os.fstat(retained[0]).st_ino, competing_inode[0])
        self.assertEqual(os.pread(retained[0], p.a.LIMIT, 0), competing)
        # No rollback: the approved temporary is now published, and the other
        # label's competing value survives in the private prior inode.
        self.assertEqual(p.approved(self.path.read_bytes(), CONTAINER)["accounts"],
                         {"acct1": CANDIDATE, "acct2": [88]})

    def test_cooperative_writer_cannot_enter_during_publication(self):
        source = self.source({"acct1": [77], "acct2": [88]})
        other = self.source()
        attempts = []
        def concurrent():
            attempts.append(True)
            with self.assertRaises(BlockingIOError):
                other.publish(other.proposed("acct2", [99]), lambda: self.fail("contending writer entered"))
            self.assertFalse(other.started)
        source.publish(source.proposed("acct1", CANDIDATE), concurrent)
        self.assertEqual(len(attempts), 2)
        self.assertEqual(p.approved(self.path.read_bytes(), CONTAINER)["accounts"],
                         {"acct1": CANDIDATE, "acct2": [88]})
        self.assertEqual(len(self.private_priors()), 1)
        # Release never replaces the stable lock; a stale snapshot must still
        # fail its original-source check after acquiring that same authority.
        with self.assertRaises(p.v.Refused):
            other.publish(other.proposed("acct2", [99]), lambda: None)
        self.assertFalse(other.started)

    def test_stable_writer_authority_substitution_refuses(self):
        source = self.source({"acct1": [77]})
        original = self.path.read_bytes()
        writer = self.path.parent / p.LOCK_NAME
        replacement = self.path.parent / "generated-writer"
        replacement.touch(mode=0o600)
        os.replace(replacement, writer)
        with self.assertRaises(p.v.Refused):
            source.publish(source.proposed("acct1", CANDIDATE), lambda: None)
        self.assertFalse(source.started)
        self.unchanged(original)

    def test_in_place_source_edit_at_exchange_is_uncertain(self):
        source = self.source({"acct1": [77], "acct2": [88]})
        exchange, before = p.exchange, self.path.stat()
        edited = encoded({"acct1": [77], "acct2": [99]})
        inode = self.path.stat().st_ino
        def conflict(*args):
            self.path.write_bytes(edited)
            # Preserve all post-exchange metadata we can legitimately compare;
            # the prior-byte check must still reject the unrelated edit.
            os.utime(self.path, ns=(before.st_atime_ns, before.st_mtime_ns))
            exchange(*args)
        with mock.patch.object(p, "exchange", side_effect=conflict):
            with self.assertRaises(p.PublicationUncertain):
                self.perform(source)
        self.assert_private_prior(edited, inode)

    def test_unrelated_mtime_edit_at_exchange_is_uncertain_even_with_same_bytes(self):
        source = self.source({"acct1": [77]})
        exchange, before = p.exchange, self.path.stat()
        def conflict(*args):
            os.utime(self.path, ns=(before.st_atime_ns, before.st_mtime_ns + 2_000_000_000))
            exchange(*args)
        with mock.patch.object(p, "exchange", side_effect=conflict):
            with self.assertRaises(p.PublicationUncertain):
                self.perform(source)

    def test_cancellation_after_exchange_preserves_prior_without_rollback_or_finish(self):
        source = self.source({"acct1": [77], "acct2": [88]})
        original, inode = self.path.read_bytes(), self.path.stat().st_ino
        transport, exchange = Transport(), p.exchange
        def cancelled(*args):
            exchange(*args)
            raise KeyboardInterrupt()
        with mock.patch.object(p, "exchange", side_effect=cancelled) as operation:
            with self.assertRaises(p.PublicationUncertain):
                self.perform(source, transport=transport)
        self.assertEqual(operation.call_count, 1)
        self.assertTrue(source.started)
        self.assertNotIn("finish", transport.actions)
        self.assert_private_prior(original, inode)
        self.assertEqual(p.approved(self.path.read_bytes(), CONTAINER)["accounts"],
                         {"acct1": CANDIDATE, "acct2": [88]})

    def test_new_source_refuses_an_existing_target(self):
        source = self.source()
        exclusive = p.h.publish_exclusive
        competing = encoded({"acct1": [77]})
        def conflict(*args):
            self.path.write_bytes(competing)
            os.chmod(self.path, 0o600)
            exclusive(*args)
        with mock.patch.object(p.h, "publish_exclusive", side_effect=conflict) as operation:
            with self.assertRaises(p.PublicationUncertain):
                source.publish(source.proposed("acct1", CANDIDATE), lambda: None)
        self.assertEqual(operation.call_count, 1)
        self.unchanged(competing)

    def test_lost_original_app_or_transport_after_publication_is_uncertain(self):
        source = self.source()
        transport = Transport(fail="finish")
        with self.assertRaises(p.PublicationUncertain):
            self.perform(source, transport=transport)
        self.assertEqual(p.approved(self.path.read_bytes(), CONTAINER)["accounts"]["acct1"], CANDIDATE)
        self.assertEqual(transport.actions.count("capture"), 1)

    def test_original_transport_join_even_after_failure(self):
        transport = p.Transport.__new__(p.Transport)
        original = mock.Mock()
        transport.child, transport.closed = original, False
        transport.out, transport.err = io.BytesIO(), io.BytesIO()
        with mock.patch.object(p.h, "setup_join") as joined:
            transport.close()
            joined.assert_called_once_with(original)
            original.stdin.close.assert_called_once()
            original.stdout.close.assert_called_once()
            transport.close()
            self.assertEqual(joined.call_count, 1)


class TerminalChecks(unittest.TestCase):
    def test_piped_input_refuses_before_open_or_lookup(self):
        with mock.patch.object(p.os, "isatty", return_value=False), mock.patch.object(p.os, "open") as opened:
            with self.assertRaises(p.v.Refused):
                p.Terminal()
            opened.assert_not_called()

    def test_exact_phrases_eof_refusal_replace_and_no_private_prompt(self):
        # Generated descriptor/TTY only, never an actual owner approval.
        with tempfile.TemporaryFile() as descriptor:
            terminal = p.Terminal.__new__(p.Terminal)
            terminal.fd = descriptor.fileno()
            terminal.pin = p.h.directory_pin(os.fstat(terminal.fd))
            for changed, expected_input in ((False, b"approve acct1"), (True, b"replace acct1")):
                for wire, allowed in ((expected_input + b"\n", True), (expected_input + b"", False), (b"\n", False),
                                      (b"yes\n", False), (b"approve acct2\n", False), (b" replace acct1\n", False)):
                    chunks = [bytes([byte]) for byte in wire] + [b""]
                    output = []
                    with mock.patch.object(p.os, "tcgetpgrp", return_value=os.getpgrp()), \
                         mock.patch.object(p.os, "write", side_effect=lambda fd, raw: output.append(raw)), \
                         mock.patch.object(p.os, "read", side_effect=chunks):
                        self.assertEqual(terminal.approve("acct1", CONTAINER, changed), allowed)
                    self.assertNotIn(b"GENERATED_PRIVATE", b"".join(output))
                    self.assertNotIn(str(CANDIDATE).encode(), b"".join(output))
            with mock.patch.object(p.os, "tcgetpgrp", return_value=os.getpgrp() + 1), self.assertRaises(p.v.Refused):
                terminal.approve("acct1", CONTAINER, False)


class SourceBoundaries(unittest.TestCase):
    def assert_owner_setup_has_no_runtime_construction(self, entry):
        # CoreBridge defines the configuration enum; importing it creates no host.
        # Exempt only that import, keeping qualified uses and constructor aliases guarded.
        runtime = re.sub(r"(?m)^import CoreBridge\r?$", "", entry)
        self.assertNotRegex(runtime, r"\b(?:CoreBridge|Core|ReviewCollection|SyncCoordinator|CKSyncEngine|CKDatabase|CKRecord|CKAsset)\b")
        for forbidden in ("ReviewCollection(", "SyncCoordinator(", "CKSyncEngine(",
                          "privateCloudDatabase", "publicCloudDatabase", "sharedCloudDatabase",
                          "CKRecord(", "CKAsset(", "XCTAttachment", "Attachment.record", "localizedDescription"):
            self.assertNotIn(forbidden, entry)

    def test_setup_boundary_rejects_generated_runtime_calls(self):
        self.assert_owner_setup_has_no_runtime_construction("import CoreBridge\n")
        for name in ("Core", "ReviewCollection", "SyncCoordinator", "CKSyncEngine", "CKDatabase", "CKRecord", "CKAsset"):
            for expression in (f"{name}(generated: true)", f"{name} \n (generated: true)",
                               f"{name}.init(generated: true)", f"let GeneratedAlias = {name}.self"):
                with self.subTest(expression=expression), self.assertRaises(AssertionError):
                    self.assert_owner_setup_has_no_runtime_construction("import CoreBridge\n" + expression)
        for expression in ("CoreBridge.Core(generated: true)", "Core.open(generated: true)",
                           "container.privateCloudDatabase", "container.publicCloudDatabase", "container.sharedCloudDatabase"):
            with self.subTest(expression=expression), self.assertRaises(AssertionError):
                self.assert_owner_setup_has_no_runtime_construction("import CoreBridge\n" + expression)

    def test_private_wire_compiles_and_preserves_original_group_join(self):
        source = p.guest_source().decode()
        ast.parse(source)
        tree = ast.parse(source)
        entry = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "guest_setup")
        finalizers = [node.finalbody for node in ast.walk(entry) if isinstance(node, ast.Try) and node.finalbody]
        self.assertTrue(any("h.setup_join(child)" in "\n".join(ast.unparse(n) for n in body) for body in finalizers))
        self.assertIn('peer.getsockopt(0, 2)', source)
        self.assertNotIn('XCTAttachment', source)

    def test_startup_entry_excluded_normal_plans_and_no_core_database_calls(self):
        entry = (ROOT / "app/MeguriSnapshotTests/TestHooks/CloudKitAccountSetupTests.swift").read_text()
        self.assertTrue(entry.startswith("#if DEBUG && MEGURI_TEST_HOOKS && os(macOS)"))
        self.assert_owner_setup_has_no_runtime_construction(entry)
        snapshot = json.loads((ROOT / "app/TestPlans/Snapshot.xctestplan").read_text())
        self.assertIn("CloudKitAccountSetupTests", snapshot["testTargets"][0]["skippedTests"])
        plan = json.loads((ROOT / "app/TestPlans/CloudAgent.xctestplan").read_text())
        self.assertEqual(plan["testTargets"][0]["selectedTests"], ["CloudKitSoakTests", "CloudKitPathPreflightTests"])
        hooks = (ROOT / "app/Meguri/TestHooks/TestHooks.swift").read_text()
        self.assertLess(hooks.index("CloudAccountSetupStartup.requested()"), hooks.index("WebContentWarmup.shared.listen()"))

    def test_setup_keeps_normal_admission_separate(self):
        source = (ROOT / "tools/scripts/cloud-account-setup.py").read_text()
        self.assertNotIn('"job": "cloudkit-signed"', source)
        self.assertNotIn('AgentHeader', source)
        self.assertNotIn('approved_account_input(', source)
        self.assertIn('v.admit_product(', source)
        self.assertIn('h.admit_preflight(', source)
        wrapper = (ROOT / "tools/mg.d/cloud-account-setup.sh").read_text()
        self.assertNotIn('mg_detail lane', wrapper)
        self.assertIn('scope:\"SETUP-ONLY\"', wrapper)
        catchup = (ROOT / "tools/mg.d/catch-up.sh").read_text()
        self.assertIn('if [ "$job" = cloudkit-account-setup ]; then', catchup)
        tiers = (ROOT / "tools/gate/tiers.yaml").read_text()
        self.assertNotIn('job: cloudkit-account-setup', tiers.split('utilities:')[0])
        self.assertIn('job: cloudkit-account-setup', tiers.split('utilities:')[1])

    def test_public_errors_never_include_sdk_marker_candidate_or_digest(self):
        private = "GENERATED_PRIVATE_SDK_MARKER"
        for error, category in ((OSError(private + str(CANDIDATE)), "setup-refused"),
                                (p.PublicationUncertain(private + str(CANDIDATE)), "publication-uncertain")):
            out = io.StringIO()
            with mock.patch.object(p.sys, "argv", ["generated", "--root", "/generated", "--run-id", "20261102T000000Z-1", "--label", "acct1"]), \
                 mock.patch.object(p.os, "dup", return_value=3), \
                 mock.patch.object(p.os, "fdopen", return_value=contextlib.nullcontext(io.BytesIO(b"{}"))), \
                 mock.patch.object(p.signal, "signal"), \
                 mock.patch.object(p, "run", side_effect=error), \
                 contextlib.redirect_stdout(out):
                self.assertEqual(p.main(), 1)
            result = json.loads(out.getvalue())
            self.assertEqual(result, {"scope": "SETUP-ONLY", "outcome": "failed_evidence", "category": category,
                                      "label": "acct1", "signed_execution": False})
            self.assertNotIn("setup_completed", out.getvalue())
            self.assertNotIn(private, out.getvalue())
            self.assertNotIn(str(CANDIDATE), out.getvalue())
            self.assertNotIn(p.v.digest(bytes(CANDIDATE)), out.getvalue())


if __name__ == "__main__":
    unittest.main()
