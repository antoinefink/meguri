"""Synthetic only: no VNC connection, native execution or workstation GUI.

Run through tools/mg image-tools-selftest -k posture (Pillow image-tools image).
Transport/inspection regressions also remain in tools/tests/vnc_session.bats.
"""
import base64
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("posture", ROOT / "tools/vm/sim-posture.py")
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)
NAME = "Meguri-Pool-foldable-1"
UDID = "11111111-2222-3333-4444-555555555555"


def window():
    return dict(id=23, pid=123, title=NAME, bundle="/Selected/DeviceHub.app",
                executable="/Selected/DeviceHub.app/Contents/MacOS/DevicesTrampoline",
                rect=[100, 50, 400, 700], screen=[0, 0, 800, 900], pointer=[0, 0])


def png(color):
    output = io.BytesIO()
    Image.new("RGB", (100, 200), color).save(output, format="PNG")
    return base64.b64encode(output.getvalue()).decode()


class EvidenceTests(unittest.TestCase):
    def test_posture_calibration_only_reads_bounded_authenticated_terminal_data(self):
        answers = [b"COMPACT " + NAME.encode() + b"\n", b"170 650\n", b"230 650\n"]
        with mock.patch.object(p.os, "open", return_value=99) as opened, \
                mock.patch.object(p.os, "close") as closed, \
                mock.patch.object(p.os, "tcgetpgrp", return_value=7), \
                mock.patch.object(p.os, "getpgrp", return_value=7), \
                mock.patch.object(p.termios, "tcgetattr", return_value=[0, 0, 0, p.termios.ICANON]), \
                mock.patch.object(p.select, "select", return_value=([99], [], [])), \
                mock.patch.object(p.os, "write", side_effect=lambda fd, data: len(data)), \
                mock.patch.object(p.os, "read", side_effect=answers) as read:
            self.assertEqual(p.operator_controls(Path("/private/capture.png"), NAME, dict(scale=2)),
                             dict(closed=[170, 650], open=[230, 650]))
            opened.assert_called_once_with("/dev/tty", os.O_RDWR | os.O_NOCTTY)
            self.assertEqual(read.call_args_list, [mock.call(99, 256), mock.call(99, 128), mock.call(99, 128)])
            closed.assert_called_once_with(99)
        with mock.patch.object(p.os, "open", return_value=99), mock.patch.object(p.os, "close"), \
                mock.patch.object(p.os, "tcgetpgrp", return_value=-1), \
                mock.patch.object(p.os, "read") as read:
            with self.assertRaises(RuntimeError):
                p.operator_controls(Path("/private/capture.png"), NAME, dict(scale=1))
            read.assert_not_called()

    def test_posture_geometry_handles_scale_and_movement_and_refuses_mismatch(self):
        w = window()
        geometry = p.window_binding(w, (800, 900), NAME)
        w["rect"][:2] = [250, 75]
        self.assertEqual(p.window_binding(w, (800, 900), NAME), geometry)
        self.assertEqual(p.window_binding(w, (1600, 1800), NAME)["scale"], 2)
        for dimensions in ((800, 1800), (1200, 1350)):
            with self.assertRaises(RuntimeError):
                p.window_binding(w, dimensions, NAME)
        for field, value in (("screen", [-800, 0, 800, 900]), ("rect", [500, 0, 400, 700]),
                             ("title", "Library")):
            other = dict(w, **{field: value})
            with self.assertRaises(RuntimeError):
                p.window_binding(other, (800, 900), NAME)

    def test_posture_title_requires_exact_high_confidence_titlebar_match(self):
        good = [(NAME, (130, 60, 250, 18))]
        p.title_evidence(good, window(), 1, NAME)
        for boxes in ([], [(NAME, (130, 200, 250, 18))], good * 2,
                      [(NAME + " other", (130, 60, 250, 18))]):
            with self.assertRaises(RuntimeError):
                p.title_evidence(boxes, window(), 1, NAME)

    def test_posture_tooltip_requires_new_exact_localized_control_at_candidate(self):
        for scale in (1, 2):
            point = (300 * scale, 700 * scale)
            box = (285 * scale, 720 * scale, 30 * scale, 14 * scale)
            good = [("Open", box)]
            p.tooltip_evidence([], good, point, scale, "Open")
            for before, after in ((good, good), ([], []), ([], good * 2),
                                  ([], [("Partially Open", box)]), ([], [("Closed", box)]),
                                  ([], [("Partially", box), ("Open", box)]),
                                  ([], [("Ouvrir", box)]), ([], [("Open", (5, 5, 30, 14))])):
                with self.assertRaises(RuntimeError):
                    p.tooltip_evidence(before, after, point, scale, "Open")

    def test_posture_ocr_rejects_low_confidence_and_bounds_data(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "image.png"
            Image.new("RGB", (800, 900)).save(path)
            guest = mock.Mock()
            guest.call.return_value = [dict(text=NAME, confidence=.89, box=[.1, .1, .1, .1])]
            self.assertEqual(p.text_boxes(guest, path), [])
            guest.call.return_value[0]["confidence"] = .99
            self.assertEqual(p.text_boxes(guest, path)[0][1], (80, 90, 80, 90))
            guest.call.return_value[0]["box"][0] = float("nan")
            with self.assertRaises(RuntimeError):
                p.text_boxes(guest, path)

    def test_posture_calibration_is_exact_bounded_private_data(self):
        geometry = p.window_binding(window(), (800, 900), NAME)
        record = dict(schema=1, compact=True, binding=dict(build="24B1", vm="guest", udid=UDID),
                      geometry=geometry, controls=dict(closed=[170, 650], open=[230, 650]))
        p.validate_calibration(record, record["binding"], geometry)
        for field, value in (("compact", False), ("binding", dict(build="different")),
                             ("geometry", dict(geometry, scale=2)), ("controls", dict(open=[170, 650], closed=[230, 650])),
                             ("controls", dict(open=[float("nan"), 650], closed=[170, 650]))):
            with self.assertRaises(RuntimeError):
                p.validate_calibration(dict(record, **{field: value}), record["binding"], geometry)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "record.json"
            path.write_text(json.dumps(record))
            path.chmod(0o600)
            self.assertEqual(p.read_calibration(path), record)
            link = path.with_name("link")
            link.symlink_to(path)
            with self.assertRaises(OSError):
                p.read_calibration(link)
            path.chmod(0o644)
            with self.assertRaises(RuntimeError):
                p.read_calibration(path)
            path.chmod(0o600)
            path.write_text(" " * 8193)
            with self.assertRaises(RuntimeError):
                p.read_calibration(path)

    def test_posture_display_proof_fails_sleeping_dim_both_lit_and_partial_information(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "display.png"
            evidence = {}
            for name, color in (("black", (0, 0, 0)), ("lit", (0, 0, 255)), ("ambiguous", (12, 12, 12))):
                Image.new("RGB", (100, 200), color).save(path)
                evidence[name] = p.display_evidence(path)
                self.assertEqual(evidence[name]["state"], name)
                self.assertEqual(set(evidence[name]), {"state", "width", "height", "black_percent", "lit_percent"})
            self.assertEqual(p.display_state({"1": evidence["lit"], "3": evidence["black"]}), "outer")
            self.assertEqual(p.display_state({"1": evidence["black"], "3": evidence["lit"]}), "inner")
            for a, b in (("lit", "lit"), ("black", "black"), ("ambiguous", "lit")):
                with self.assertRaises(RuntimeError):
                    p.display_state({"1": evidence[a], "3": evidence[b]})
            with self.assertRaises(KeyError):
                p.display_state({"3": evidence["lit"]})  # Inner alone never proves Open.

    def test_posture_foreground_and_pointer_are_rechecked(self):
        original = window()
        guest = mock.Mock()
        guest.call.return_value = dict(original, pointer=[300, 700])
        p.stable_window(guest, original, [300, 700])
        for changed in (dict(original, rect=[101, 50, 400, 700]), dict(original, pid=124),
                        dict(original, id=24), dict(original, pointer=[400, 700])):
            guest.call.return_value = changed
            with self.assertRaises(RuntimeError):
                p.stable_window(guest, original, [300, 700])


class HolderTests(unittest.TestCase):
    def test_posture_guest_protocol_bounds_writes_and_handles_partial_publication(self):
        guest = p.Guest.__new__(p.Guest)
        guest.sequence = 0
        guest.child = mock.Mock()
        guest.child.stdin.fileno.return_value = 99
        guest.read = mock.Mock(return_value=dict(id=1, ok=True, result="joined"))
        writes = []
        def partial(fd, data):
            self.assertEqual(fd, 99)
            count = min(7, len(data))
            writes.append(bytes(data[:count]))
            return count
        with mock.patch.object(p.select, "select", return_value=([], [guest.child.stdin], [])), \
                mock.patch.object(p.os, "write", side_effect=partial):
            self.assertEqual(guest.call("window"), "joined")
        self.assertEqual(json.loads(b"".join(writes)), dict(id=1, action="window"))
        with mock.patch.object(p.select, "select", return_value=([], [], [])), \
                mock.patch.object(p.os, "write") as write:
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                guest.call("window")
            write.assert_not_called()

    def test_posture_guest_rejects_wrong_extra_boolean_ack_ids_and_bad_frames(self):
        guest = p.Guest.__new__(p.Guest)
        guest.sequence = 0
        guest.child = mock.Mock()
        for reply in (dict(id=True, ok=True, result=True), dict(id=2, ok=True, result=True),
                      dict(id=1, ok=True, result=True, extra=True), dict(ok=True, result=True),
                      dict(id=1, ok=1, result=True), [True]):
            guest.sequence = 0
            guest.failed = False
            with mock.patch.object(p.select, "select", return_value=([], [guest.child.stdin], [])), \
                    mock.patch.object(p.os, "write", side_effect=lambda fd, data: len(data)), \
                    mock.patch.object(guest, "read", return_value=reply):
                with self.assertRaises(RuntimeError):
                    guest.call("arm")
        for data in (b'{}\n{}\n', b'{}\ntrailing', b'[]\n', b'', b'{"error":"window"}\n', b'{"id":1,"id":1}\n', b'{"value":NaN}\n'):
            with self.subTest(data=data), mock.patch.object(p.select, "select", return_value=([guest.child.stdout], [], [])), \
                    mock.patch.object(p.os, "read", return_value=data):
                with self.assertRaises((RuntimeError, ValueError)):
                    guest.read(1)

    def fixture(self, folder):
        state = Path(folder)
        os.mkfifo(state / "commands", 0o600)
        (state / "holder.log").write_text("connected\nposture-click-v1\n")
        (state / "holder.log").chmod(0o600)
        keepalive = os.open(state / "commands", os.O_RDWR | os.O_NONBLOCK)
        self.addCleanup(os.close, keepalive)
        guest = mock.Mock()
        guest.request = dict(guard="synthetic-guard")
        guest.call.side_effect = lambda action: (
            dict(armed=True, remaining=600) if action == "arm" else
            dict(published=True, remaining=600) if action == "publish" else True)
        holder = p.Holder(state, lambda: True, guest, state, state)
        self.addCleanup(holder.log.close)
        self.addCleanup(Path(str(state) + ".mutex").unlink, missing_ok=True)
        return state, guest, holder

    def test_posture_readonly_shot_error_and_fifo_open_failure_never_arm(self):
        for failure in ("shot", "open", "lost"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as folder:
                state, guest, holder = self.fixture(folder)
                def publish(fd, data):
                    with (state / "holder.log").open("a") as log:
                        log.write("error shot private detail\n")
                    return len(data)
                if failure == "open":
                    (state / "commands").unlink()
                elif failure == "lost":
                    holder.connected = lambda: False
                with mock.patch.object(p.os, "write", side_effect=publish):
                    with self.assertRaises((OSError, RuntimeError)):
                        holder.batch()
                self.assertFalse(holder.pending)
                self.assertFalse((state / "sim-posture-pending").exists())
                guest.call.assert_not_called()

    def test_posture_click_terminal_error_and_success_join_without_a_screenshot(self):
        for error in (None, "input", "expired"):
            with self.subTest(error=error), tempfile.TemporaryDirectory() as folder:
                state, guest, holder = self.fixture(folder)
                publications = []
                def publish(fd, data):
                    publications.append(bytes(data))
                    command, deadline, path = data.decode().strip().split(" ", 2)
                    self.assertEqual(command, "posture-click")
                    self.assertGreater(float(deadline), time.monotonic())
                    result = dict(joined=True, executed=error != "expired")
                    if error:
                        result["error"] = error
                    Path(path).write_text(json.dumps(result))
                    return len(data)
                with mock.patch.object(p.os, "write", side_effect=publish):
                    if error:
                        with self.assertRaisesRegex(RuntimeError, "refused or failed"):
                            holder.click()
                    else:
                        holder.click()
                self.assertFalse(holder.pending)
                self.assertFalse((state / "sim-posture-pending").exists())
                self.assertEqual(len(publications), 1)
                self.assertEqual(guest.call.call_args_list, [mock.call("arm"), mock.call("publish"), mock.call("disarm")])

    def test_posture_short_lease_and_lost_ssh_before_publication_never_click(self):
        for failure in ("short", "arm-ssh", "publish-ssh", "fifo", "old"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as folder:
                state, guest, holder = self.fixture(folder)
                if failure == "short":
                    guest.call.side_effect = lambda action: dict(armed=True, remaining=1) if action == "arm" else True
                elif failure in ("arm-ssh", "publish-ssh"):
                    original = guest.call.side_effect
                    def call(action):
                        if action == ("arm" if failure == "arm-ssh" else "publish"):
                            raise RuntimeError("guest disconnected")
                        return original(action)
                    guest.call.side_effect = call
                elif failure == "fifo":
                    (state / "commands").unlink()
                else:
                    holder.capable = False
                with mock.patch.object(p.os, "write") as write:
                    with self.assertRaises((RuntimeError, OSError)):
                        holder.click()
                    write.assert_not_called()
                self.assertFalse(holder.pending)
                self.assertFalse((state / "sim-posture-pending").exists())

    def test_posture_atomic_fifo_error_is_nonexecution_but_partial_publication_is_uncertain(self):
        for outcome in ("error", "zero", "partial"):
            with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as folder:
                state, guest, holder = self.fixture(folder)
                def publish(fd, payload):
                    if outcome == "error":
                        raise BlockingIOError("pipe full")
                    return 0 if outcome == "zero" else 1
                with mock.patch.object(p.os, "write", side_effect=publish):
                    with self.assertRaises((RuntimeError, OSError)):
                        holder.click()
                self.assertEqual(holder.pending, outcome == "partial")
                self.assertEqual((state / "sim-posture-pending").exists(), outcome == "partial")
                if outcome != "partial":
                    self.assertIn(mock.call("disarm"), guest.call.call_args_list)

    def test_posture_unjoined_actual_click_retains_evidence_after_disconnect(self):
        with tempfile.TemporaryDirectory() as folder:
            state, guest, holder = self.fixture(folder)
            ticks = iter(range(1000))
            # A submitted callback has no terminal receipt. Never infer a join
            # from caller/SSH/VNC termination, timeout, or elapsed input deadline.
            with mock.patch.object(p.time, "monotonic", side_effect=lambda: next(ticks)), \
                    mock.patch.object(p.time, "sleep"), \
                    mock.patch.object(p.os, "write", side_effect=lambda fd, data: len(data)):
                with self.assertRaisesRegex(RuntimeError, "completion unproven"):
                    holder.click()
            self.assertTrue(holder.pending)
            self.assertTrue((state / "sim-posture-pending").exists())
            self.assertNotIn(mock.call("disarm"), guest.call.call_args_list)

    def test_posture_lost_transport_and_unjoined_guard_retain_only_actual_fences(self):
        guest = mock.Mock(admitted=True)
        guest.child.wait.side_effect = [subprocess.TimeoutExpired("synthetic", 1), -15]
        self.assertEqual(p.close_guest(guest, True), (False, False))
        guest.call.assert_not_called()
        guest.terminal.assert_not_called()
        for code in (0, 1, 255, -15):
            guest = mock.Mock(admitted=True)
            guest.child.wait.return_value = code
            self.assertEqual(p.close_guest(guest, False), (True, code == 0))
        guest = mock.Mock(admitted=True)
        guest.call.side_effect = RuntimeError("SSH lost before button input")
        guest.child.wait.return_value = 255
        self.assertEqual(p.close_guest(guest, False), (True, False))
        guest.terminal.assert_called_once()

    def test_posture_native_holder_callback_refuses_expired_and_joins_terminal_errors(self):
        # Load only against synthetic vncdotool boundary classes; no dependency
        # installation, VNC connection, credentials or native acceptance implied.
        import types
        client_module = types.ModuleType("vncdotool.client")
        client_module.VNCDoToolClient = type("Client", (), {})
        client_module.VNCDoToolFactory = type("Factory", (), {})
        package = types.ModuleType("vncdotool")
        package.api = mock.Mock()
        with mock.patch.dict(sys.modules, {"vncdotool": package, "vncdotool.client": client_module}):
            spec = importlib.util.spec_from_file_location("native_holder", ROOT / "tools/vm/vnc-holder.py")
            native = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(native)
        for scenario in ("expired", "ok", "error"):
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as folder:
                client = native.ResizingVNCClient()
                client.mouseDown = mock.Mock(side_effect=RuntimeError("private") if scenario == "error" else None)
                client.mouseUp = mock.Mock()
                receipt = Path(folder) / "receipt.json"
                with mock.patch.object(native.time, "monotonic", return_value=10):
                    client.postureClick(9 if scenario == "expired" else 11, str(receipt))
                proof = json.loads(receipt.read_bytes())
                self.assertTrue(proof["joined"])
                self.assertEqual(proof["executed"], scenario != "expired")
                if scenario == "expired":
                    client.mouseDown.assert_not_called()
                    client.mouseUp.assert_not_called()
                else:
                    client.mouseDown.assert_called_once_with(1)
                    client.mouseUp.assert_called_once_with(1)

    def test_posture_calibration_terminal_refusal_precedes_all_guest_work(self):
        with mock.patch.object(p, "operator_terminal", side_effect=RuntimeError("operator terminal")), \
                mock.patch.object(p, "Guest") as guest, mock.patch.object(p.session, "status") as status:
            with self.assertRaisesRegex(RuntimeError, "operator terminal"):
                p.run(Path("/unused"), "ssh", "guest", str(ROOT), UDID, "tart", "calibrate", Path("/lease"))
            guest.assert_not_called()
            status.assert_not_called()


class ProductionFlowTests(unittest.TestCase):
    def test_posture_one_click_after_tooltip_and_two_readonly_display_proofs(self):
        for pose in ("open", "closed"):
            for failure in (None, "delayed", "transient", "stale", "partial", "sleep", "proof", "calibration", "moved"):
                with self.subTest(pose=pose, failure=failure), tempfile.TemporaryDirectory() as folder:
                    folder = Path(folder)
                    lease = folder / "lease"
                    lease.mkdir()
                    w = window()
                    state = dict(clicks=0, frames=0, pointed=False, displays=0)
                    binding = dict(name=NAME, vm="guest", udid=UDID, build="selected")
                    record = dict(schema=1, compact=True, binding=binding,
                                  geometry=p.window_binding(w, (800, 900), NAME),
                                  controls=dict(closed=[170, 650], open=[230, 650]))
                    if failure == "calibration":
                        record["binding"] = dict(binding, build="foreign")

                    class FakeGuest:
                        def __init__(self, *args):
                            self.ready = dict(binding=binding)
                            self.child = mock.Mock()
                        def call(self, action, **kwargs):
                            if action == "window":
                                result = copy.deepcopy(w)
                                if failure == "moved" and state["frames"] > 1:
                                    result["rect"][0] += 1
                                return result
                            if action == "displays":
                                state["displays"] += 1
                                if failure == "sleep":
                                    return {"1": png("black"), "3": png("black")}
                                if failure == "proof" and state["clicks"]:
                                    return {"1": png("white"), "3": png("white")}
                                if failure == "transient" and state["clicks"] and 2 < state["displays"] < 6:
                                    color = {3: "black", 4: "#0c0c0c", 5: "white"}[state["displays"]]
                                    return {"1": png(color), "3": png(color)}
                                inner = pose == "closed" if not state["clicks"] else pose == "open"
                                return {"1": png("black" if inner else "white"),
                                        "3": png("white" if inner else "black")}
                            raise AssertionError(action)

                    class FakeHolder:
                        def __init__(self, *args):
                            self.pending = False
                            self.log = io.BytesIO()
                            self.folder = args[3]
                        def click(self):
                            state["clicks"] += 1
                        def batch(self, command=None):
                            if command:
                                _, x, y = command.split()
                                w["pointer"] = [int(x), int(y)]
                                state["pointed"] = int(y) == 700
                            state["frames"] += 1
                            path = self.folder / (str(state["frames"]) + ".png")
                            image = Image.new("RGB", (800, 900))
                            if state["pointed"] and failure != "stale":
                                ImageDraw.Draw(image).rectangle([200, 715, 400, 735], fill="white")
                            image.save(path)
                            return path

                    def boxes(guest, path):
                        result = [(NAME, (130, 60, 250, 18))]
                        if state["pointed"] and (failure != "delayed" or state["frames"] > 7):
                            label = "Partially Open" if failure == "partial" else pose.title()
                            x = w["pointer"][0]
                            result.append((label, (x - 15, 720, 30, 14)))
                        return result

                    ticks = iter(i / 20 for i in range(100000))
                    with mock.patch.object(p.time, "monotonic", side_effect=lambda: next(ticks)), \
                            mock.patch.object(p.session, "status", return_value=dict(connected=True)), \
                            mock.patch.object(p, "Guest", FakeGuest), mock.patch.object(p, "Holder", FakeHolder), \
                            mock.patch.object(p, "text_boxes", side_effect=boxes), \
                            mock.patch.object(p, "read_calibration", return_value=record), \
                            mock.patch.object(p, "calibration_path", return_value=folder / "record"), \
                            mock.patch.object(p, "close_guest", return_value=(True, True)), mock.patch.object(p.time, "sleep"):
                        if failure not in (None, "delayed", "transient"):
                            with self.assertRaises(RuntimeError):
                                p.run(folder, "ssh", "guest", str(ROOT), UDID, "tart", pose, lease)
                        else:
                            receipt = p.run(folder, "ssh", "guest", str(ROOT), UDID, "tart", pose, lease)
                            self.assertEqual(receipt["click_count"], 1)
                            self.assertIn(pose.title(), receipt["control_evidence"])
                            self.assertEqual(state["displays"], 7 if failure == "transient" else 3)
                    self.assertEqual(state["clicks"], 1 if failure in (None, "delayed", "transient", "proof") else 0)
                    self.assertFalse((lease / "sim-posture-pending").exists())


class GuardTests(unittest.TestCase):
    def test_posture_guest_mutex_holds_exact_lease_through_input_and_proof(self):
        self.check_guard_scenarios(("read-only", "armed-only", "short", "error", "joined", "pending", "timeout", "bad-frame"))

    def test_posture_terminal_cancellation_orders_with_paused_real_publish(self):
        self.check_guard_scenarios(("cancel-before-lock", "cancel-at-marker", "foreign-at-marker"))

    def check_guard_scenarios(self, scenarios):
        for scenario in scenarios:
            end_pending = scenario in ("pending", "foreign-at-marker")
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                developer = root / "Selected Xcode.app/Contents/Developer"
                developer.mkdir(parents=True)
                hub = developer.parent / "Applications/DeviceHub.app"
                (hub / "Contents/MacOS").mkdir(parents=True)
                (hub / "Contents/MacOS/DevicesTrampoline").write_bytes(b"synthetic")
                (hub / "Contents/Info.plist").write_bytes(plistlib.dumps(dict(
                    CFBundleIdentifier="com.apple.dt.Devices", CFBundleExecutable="DevicesTrampoline")))
                (developer.parent / "Info.plist").write_bytes(plistlib.dumps(dict(CFBundleShortVersionString="27.2")))
                (developer.parent / "version.plist").write_bytes(plistlib.dumps(dict(ProductBuildVersion="selected")))
                pool = root / "pool"
                lease = pool / "leases" / UDID / "lease.json"
                lease.parent.mkdir(parents=True)
                lease.write_text(json.dumps(dict(udid=UDID, vm="guest", kind="foldable", worktree="/task",
                                                 expires_epoch=time.time() + (1 if scenario == "short" else 600))))
                original = lease.read_bytes()
                pin = json.loads((ROOT / "tools/sim-devices.json").read_text())["foldable"]["runtime"]
                request = dict(vm="guest", checkout="/task", udid=UDID, pool=str(pool), pin=pin, visual="fixed source", guard="test-guard")
                actions = {
                    "read-only": ["window", "finish"], "armed-only": ["arm", "finish"],
                    "short": ["arm"], "error": ["arm", "disarm", "finish"],
                    "joined": ["arm", "publish", "displays", "disarm", "finish"],
                    "pending": ["arm", "publish"], "timeout": [], "bad-frame": [],
                    "cancel-before-lock": ["arm", "publish"],
                    "cancel-at-marker": ["arm", "publish"], "foreign-at-marker": ["arm", "publish"],
                }[scenario]
                wire = b"".join((json.dumps(dict(id=i, action=a)) + "\n").encode() for i, a in enumerate(actions, 1))
                if scenario == "bad-frame":
                    wire = b'{"id":true,"action":"arm"}\n'
                source = io.TextIOWrapper(io.BytesIO(wire))
                if scenario == "timeout":
                    source = mock.Mock()
                    source.buffer.readline.side_effect = TimeoutError("read-only guard alarm")
                calls = []
                def output(args, **kwargs):
                    calls.append(args)
                    self.assertTrue((pool / "pool.lock/pid").exists())
                    self.assertEqual(lease.read_bytes(), original)
                    if args[0] == "/usr/bin/xcode-select":
                        return str(developer).encode()
                    if args[0] in ("/usr/bin/codesign", "/usr/bin/open"):
                        return b""
                    if args[0].endswith("/visual"):
                        return json.dumps(dict(window(), bundle=str(hub),
                            executable=str(hub / "Contents/MacOS/DevicesTrampoline"))).encode()
                    self.assertEqual(args[0], str(developer / "usr/bin/simctl"))
                    if args[1] == "io":
                        self.assertEqual(args[2:4], (UDID, "screenshot"))
                        self.assertTrue((pool / "pool.lock/sim-posture-pending").is_dir())
                        Path(args[-1]).write_bytes(b"synthetic PNG transport")
                        return b""
                    inventories = dict(devices={pin["identifier"]: [dict(udid=UDID, name=NAME,
                                       state="Booted", isAvailable=True, deviceTypeIdentifier="duo")]},
                                       runtimes=[dict(identifier=pin["identifier"], version=pin["version"],
                                                      buildversion=pin["build"], isAvailable=True)],
                                       devicetypes=[dict(identifier="duo", name="iPhone Duo", productFamily="iPhone")])
                    return json.dumps({args[2]: inventories[args[2]]}).encode()
                original_stat = Path.stat
                def path_stat(path, *args, **kwargs):
                    return type("Console", (), dict(st_uid=501))() if str(path) == "/dev/console" else original_stat(path, *args, **kwargs)
                # Run the real terminal script against the real publish path.
                # Exercise both orders: cancellation wins before serialization,
                # or it arrives just before mkdir and must wait for publication.
                racing = scenario in ("cancel-before-lock", "cancel-at-marker", "foreign-at-marker")
                terminal_requested, terminal_done = threading.Event(), threading.Event()
                terminal_errors = []
                def terminal():
                    try:
                        terminal_request = dict(request, guard="foreign" if scenario == "foreign-at-marker" else request["guard"])
                        with mock.patch.object(sys, "argv", ["terminal", json.dumps(terminal_request)]):
                            exec(p.TERMINAL, {})
                    except BaseException as error:
                        terminal_errors.append(error)
                    finally:
                        terminal_done.set()
                worker = threading.Thread(target=terminal, daemon=True)
                main_thread = threading.current_thread()
                real_flock, real_mkdir = p.fcntl.flock, os.mkdir
                first_lock = True
                def flock(fd, operation):
                    nonlocal first_lock
                    if operation == p.fcntl.LOCK_EX:
                        if threading.current_thread() is worker:
                            terminal_requested.set()
                        elif first_lock:
                            first_lock = False
                            if scenario == "cancel-before-lock":
                                worker.start()
                                self.assertTrue(terminal_done.wait(5))
                    return real_flock(fd, operation)
                def mkdir(path, *args, **kwargs):
                    if racing and scenario != "cancel-before-lock" and path == "sim-posture-pending":
                        self.assertIs(threading.current_thread(), main_thread)
                        worker.start()
                        self.assertTrue(terminal_requested.wait(5))
                        self.assertFalse(terminal_done.is_set())  # Publish still holds the inode lock.
                    return real_mkdir(path, *args, **kwargs)
                def reply(value, **kwargs):
                    if racing and '"published": true' in value:
                        self.assertTrue(terminal_done.wait(5))
                with mock.patch.object(sys, "argv", ["guard", json.dumps(request)]), \
                        mock.patch.object(sys, "stdin", source), mock.patch.object(os, "getuid", return_value=501), \
                        mock.patch.object(Path, "stat", path_stat), \
                        mock.patch.object(subprocess, "check_output", side_effect=output), \
                        mock.patch.object(subprocess, "run"), mock.patch.object(p.signal, "signal"), \
                        mock.patch.object(p.signal, "alarm"), mock.patch("builtins.print", side_effect=reply), \
                        mock.patch.object(p.fcntl, "flock", side_effect=flock), mock.patch.object(os, "mkdir", side_effect=mkdir):
                    if scenario in ("pending", "short", "timeout", "bad-frame") or racing:
                        with self.assertRaises(SystemExit) as stopped:
                            exec(p.GUEST, {})
                        self.assertEqual(stopped.exception.code, 1)
                    else:
                        exec(p.GUEST, {})
                if racing:
                    worker.join(5)
                    self.assertFalse(worker.is_alive())
                    self.assertEqual(terminal_errors, [])
                self.assertEqual((pool / "pool.lock/sim-posture-pending").exists(), end_pending)
                self.assertEqual((pool / "pool.lock").exists(), end_pending)
                self.assertEqual(lease.read_bytes(), original)
                self.assertFalse(any("boot" in c or "reset" in c for c in calls))
                if scenario.startswith("cancel-"):
                    # No residual token files or fences can block the next owner.
                    (pool / "pool.lock").mkdir()
                    (pool / "pool.lock").rmdir()
                    self.assertEqual(list(pool.iterdir()), [pool / "leases"])

    def test_posture_terminal_disarm_removes_only_the_exact_owned_guard(self):
        for token, replaced in (("owned", False), ("foreign", False), ("owned", True)):
            with self.subTest(token=token, replaced=replaced), tempfile.TemporaryDirectory() as folder:
                pool = Path(folder)
                lock = pool / "pool.lock"
                lock.mkdir()
                (lock / "posture-owner").write_text(token + "\nP")
                (lock / "sim-posture-pending").mkdir()
                request = dict(pool=str(pool), guard="owned")
                original_flock = p.fcntl.flock
                def flock(fd, operation):
                    if replaced and operation == p.fcntl.LOCK_EX:
                        # Delay cleanup after it has opened its exact owner. A
                        # replacement must survive even when the old token matches.
                        lock.rename(pool / "retired")
                        lock.mkdir()
                        (lock / "posture-owner").write_text("replacement\nP")
                        (lock / "sim-posture-pending").mkdir()
                    return original_flock(fd, operation)
                with mock.patch.object(sys, "argv", ["terminal", json.dumps(request)]), mock.patch("builtins.print"), \
                        mock.patch.object(p.fcntl, "flock", side_effect=flock):
                    exec(p.TERMINAL, {})
                original = pool / "retired" if replaced else lock
                self.assertEqual((original / "sim-posture-pending").exists(), token != "owned")
                self.assertEqual((original / "posture-owner").read_text(), token + ("\nT" if token == "owned" else "\nP"))
                if replaced:
                    self.assertTrue((lock / "sim-posture-pending").exists())
                    self.assertEqual((lock / "posture-owner").read_text(), "replacement\nP")
                self.assertTrue(lock.exists())  # No foreign/live lock reclamation.


if __name__ == "__main__":
    unittest.main()
