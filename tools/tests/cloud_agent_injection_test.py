#!/usr/bin/env python3
"""Generated file/command regressions only; never evidence of Apple signing."""
import contextlib
import importlib.util
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
NATIVE_ARGS = ["build-for-testing", "-project", "app/Meguri.xcodeproj", "-scheme", "Meguri",
               "-testPlan", "CloudAgent", "-configuration", "Debug", "-destination", "platform=macOS,arch=arm64"]
spec = importlib.util.spec_from_file_location("cloud_agent_injection", ROOT / "tools/scripts/cloud-agent-injection.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class Preparation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.bundle = self.root / "Selected.app"
        self.source = self.bundle / m.SOURCE
        self.source.parent.mkdir(parents=True)
        self.source.write_bytes(b"generated injection bytes")
        self.run = self.root / "run"
        self.run.mkdir(mode=0o700)
        self.events = []
        self.facts = patch.object(m.stage, "facts", return_value={"version": "27.2", "build": "GENERATED"}).start()
        self.addCleanup(patch.stopall)

    @contextlib.contextmanager
    def selected(self, private):
        # The separate vm-xcode-stage tests own real trust-policy argv checks.
        # This double supplies only a generated, descriptor-bound selection.
        with m.stage.Directory(str(self.bundle)) as bundle, \
                m.stage.Directory(str(self.bundle / "Contents/Developer")) as developer:
            authority = m.stage.InstalledSource(bundle)
            self.events.append("selected-verified")
            yield developer, bundle, authority
            self.events.append("selected-rechecked")

    def prepare(self):
        return m.prepare(str(self.run))

    def test_exact_exclusive_copy_keeps_original_and_rechecks_selection(self):
        before = m.stage.file_pin(self.source.stat())
        with patch.object(m.stage, "selected_source", self.selected), self.prepare() as prepared:
            copied = Path(prepared) / m.NAME
            self.assertEqual(copied.read_bytes(), self.source.read_bytes())
            self.assertEqual(copied.stat().st_nlink, 1)
            self.assertNotEqual(copied.stat().st_ino, self.source.stat().st_ino)
            self.assertEqual(copied.parent.stat().st_mode & 0o777, 0o700)
        self.assertEqual(m.stage.file_pin(self.source.stat()), before)
        self.assertEqual(self.events, ["selected-verified", "selected-rechecked"])
        self.facts.assert_called_once()

    def test_simulator_uses_only_its_selected_xcode_library_and_rejects_other_platforms(self):
        simulator_source = self.bundle / m.SOURCES["ios-sim"]
        simulator_source.parent.mkdir(parents=True)
        simulator_source.write_bytes(b"generated simulator injection bytes")
        with patch.object(m.stage, "selected_source", self.selected):
            with m.prepare(str(self.run), platform="ios-sim") as prepared:
                copied = Path(prepared) / m.NAME
                self.assertEqual(copied.read_bytes(), simulator_source.read_bytes())
                self.assertNotEqual(copied.read_bytes(), self.source.read_bytes())
            for platform in ("ios-device", "unknown", "iphonesimulator"):
                with self.subTest(platform=platform), self.assertRaises(m.stage.Refused):
                    with m.prepare(str(self.run), platform=platform): pass
            saved = simulator_source.with_name("saved.dylib")
            simulator_source.rename(saved)
            simulator_source.symlink_to(saved)
            with self.assertRaises((m.stage.Refused, OSError)):
                with m.prepare(str(self.run), platform="ios-sim"): pass

    def test_source_symlink_hardlink_oversize_and_parent_substitution_refuse(self):
        for mutation_case in ("source-link", "source-alias", "source-size", "parent-link"):
            with self.subTest(mutation_case), tempfile.TemporaryDirectory() as scratch_path:
                baseline_bytes = self.source.read_bytes()
                if mutation_case == "source-link":
                    payload_file = Path(scratch_path) / "fixture-payload"
                    payload_file.write_bytes(baseline_bytes)
                    self.source.unlink()
                    self.source.symlink_to(payload_file)
                elif mutation_case == "source-alias":
                    os.link(self.source, Path(scratch_path) / "fixture-hardlink")
                elif mutation_case == "source-size":
                    with self.source.open("wb") as writable:
                        writable.truncate(m.LIMIT + 1)
                else:
                    saved_parent = self.source.parent.with_name("fixture-parent")
                    self.source.parent.rename(saved_parent)
                    self.source.parent.symlink_to(saved_parent, target_is_directory=True)
                try:
                    with patch.object(m.stage, "selected_source", self.selected):
                        with self.assertRaises((m.stage.Refused, OSError)):
                            with self.prepare():
                                self.fail("preparation accepted the substituted fixture")
                finally:
                    if mutation_case == "parent-link":
                        self.source.parent.unlink()
                        saved_parent.rename(self.source.parent)
                    else:
                        self.source.unlink()
                        self.source.write_bytes(baseline_bytes)

    def test_source_and_destination_changes_during_build_refuse(self):
        for mutation_case in ("input-data", "input-entry", "copy-data", "copy-entry", "stage-entry"):
            with self.subTest(mutation_case), patch.object(m.stage, "selected_source", self.selected):
                baseline_bytes = self.source.read_bytes()
                with self.assertRaises((m.stage.Refused, OSError)):
                    with self.prepare() as staging_path:
                        copied_file = Path(staging_path) / m.NAME
                        if mutation_case == "input-data":
                            self.source.write_bytes(b"fixture replacement")
                        elif mutation_case == "input-entry":
                            self.source.unlink()
                            self.source.write_bytes(baseline_bytes)
                        elif mutation_case == "copy-data":
                            copied_file.chmod(0o600)
                            copied_file.write_bytes(b"fixture replacement")
                        elif mutation_case == "copy-entry":
                            copied_file.unlink()
                            copied_file.symlink_to(self.source)
                        else:
                            Path(staging_path).rename(str(staging_path) + "-fixture-moved")
                            Path(staging_path).mkdir(mode=0o700)
                self.source.write_bytes(baseline_bytes)

    def test_stale_exclusion_unlinks_only_exact_regular_library_and_refuses_links(self):
        frameworks = self.root / "Meguri.app/Contents/Frameworks"
        frameworks.mkdir(parents=True)
        stale = frameworks / m.NAME
        other = frameworks / "keep.dylib"
        other.write_bytes(b"preserve")
        stale.write_bytes(b"stale generated product")
        m.remove(str(frameworks))
        self.assertFalse(stale.exists())
        self.assertEqual(other.read_bytes(), b"preserve")
        stale.symlink_to(other)
        with self.assertRaises((m.stage.Refused, OSError)):
            m.remove(str(frameworks))
        self.assertEqual(other.read_bytes(), b"preserve")
        stale.unlink()
        frameworks.rename(frameworks.with_name("Original"))
        frameworks.symlink_to(frameworks.with_name("Original"), target_is_directory=True)
        with self.assertRaises((m.stage.Refused, OSError)):
            m.remove(str(frameworks))

    def exercise_child_lifetime(self, child_case, exit_status=0):
        """Exercise subprocess ownership using a Python fixture and open inputs."""
        with tempfile.TemporaryDirectory(dir=self.root) as scratch_path:
            scratch_dir = Path(scratch_path)
            child_script = scratch_dir / "child-fixture.py"
            child_script.write_text('''import os, signal, sys, time
from pathlib import Path
scratch_dir, child_case, owner_pid, exit_status = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
def handle_term(*unused):
    (scratch_dir / "term-entered").touch()
    time.sleep(0.15)
    (scratch_dir / "term-finished").touch()
    sys.exit(0)
signal.signal(signal.SIGTERM, handle_term)
(scratch_dir / "child-ready").touch()
if child_case == "complete":
    os.write(1, b"fixture output stream\\n")
    os.write(2, b"fixture error stream\\n")
    if exit_status < 0:
        signal.signal(-exit_status, signal.SIG_DFL)
        os.kill(os.getpid(), -exit_status)
    sys.exit(exit_status)
expires_at = time.monotonic() + 5
if child_case == "cancel-at-wait":
    while not (scratch_dir / "wait-entered").exists() and time.monotonic() < expires_at:
        time.sleep(0.01)
    os.kill(owner_pid, signal.SIGTERM)
while time.monotonic() < expires_at:
    time.sleep(0.01)
sys.exit(97)
''')
            spawn_original, join_group, close_fd, bind_original = subprocess.Popen, m.stage.join_original, os.close, m.bound_file
            children, input_fds, trace = [], set(), []

            def require_joined_child():
                self.assertEqual(len(children), 1)
                self.assertIsNotNone(children[0].returncode)
                self.assertIn("group-finished", trace)

            @contextlib.contextmanager
            def retain_selection(selection_arg):
                with self.selected(selection_arg) as selected_dirs:
                    try:
                        yield selected_dirs
                    finally:
                        require_joined_child()
                        # Both directory handles must remain usable when the
                        # selected-source context reaches its finalizer.
                        for selected_dir in selected_dirs[:2]:
                            os.fstat(selected_dir.fd)
                        trace.append("selection-close")

            def record_input(input_dir, basename, input_fd, initial_pin, content):
                bind_original(input_dir, basename, input_fd, initial_pin, content)
                input_fds.add(input_fd)

            def release_input(input_fd):
                try:
                    if input_fd in input_fds:
                        input_fds.remove(input_fd)
                        require_joined_child()
                        trace.append("input-close")
                finally:
                    close_fd(input_fd)

            def observe_group_join(process):
                self.assertIs(process, children[0])
                self.assertEqual(len(input_fds), 2)
                for input_fd in input_fds:
                    os.fstat(input_fd)
                trace.append("group-join-entered")
                join_group(process)
                if child_case.startswith("cancel-"):
                    self.assertTrue((scratch_dir / "term-entered").exists())
                    self.assertTrue((scratch_dir / "term-finished").exists())
                trace.append("group-finished")

            def launch_fixture(command_line, **spawn_options):
                self.assertEqual(command_line[0], "/usr/bin/xcodebuild")
                self.assertTrue(spawn_options["start_new_session"])
                for stream_key in ("stdin", "stdout", "stderr"):
                    self.assertIsNone(spawn_options.get(stream_key))
                process = spawn_original([sys.executable, "-B", str(child_script), str(scratch_dir), child_case,
                                          str(os.getpid()), str(exit_status)], **spawn_options)
                children.append(process)
                wait_original = process.wait

                def observe_wait(*wait_args, **wait_options):
                    if child_case == "cancel-at-wait":
                        (scratch_dir / "wait-entered").touch()
                    trace.append("wait-called")
                    wait_result = wait_original(*wait_args, **wait_options)
                    trace.append("wait-finished")
                    return wait_result

                process.wait = observe_wait
                if child_case == "cancel-at-start":
                    expires_at = time.monotonic() + 5
                    while not (scratch_dir / "child-ready").exists():
                        self.assertLess(time.monotonic(), expires_at)
                        time.sleep(0.01)
                    os.kill(os.getpid(), signal.SIGTERM)
                    # build() must receive the subprocess handle before its
                    # pending cancellation can begin joining that process.
                    trace.append("spawn-returned")
                return process

            def reject_unhandled_signal(*unused):
                # Expose absent signal handling as an assertion in this runner.
                raise AssertionError("cancellation reached the fixture fallback handler")

            prior_handlers = {signal_id: signal.signal(signal_id, reject_unhandled_signal)
                              for signal_id in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)}
            # Keep a separate process group alive through each join sequence.
            bystander = spawn_original([sys.executable, "-B", "-c", "import time; time.sleep(30)"],
                                       start_new_session=True)
            try:
                with tempfile.TemporaryFile() as output_capture, tempfile.TemporaryFile() as error_capture:
                    saved_output, saved_error = os.dup(1), os.dup(2)
                    try:
                        os.dup2(output_capture.fileno(), 1)
                        os.dup2(error_capture.fileno(), 2)
                        with patch.object(m.stage, "selected_source", retain_selection), \
                                patch.object(m, "bound_file", side_effect=record_input), \
                                patch.object(os, "close", side_effect=release_input), \
                                patch.object(m.stage, "join_original", side_effect=observe_group_join), \
                                patch.object(subprocess, "Popen", side_effect=launch_fixture) as spawn_mock:
                            command_args = ["--build", str(self.run), str(scratch_dir / "Frameworks"), "--", *NATIVE_ARGS]
                            if child_case.startswith("cancel-"):
                                with self.assertRaises(m.stage.Cancelled):
                                    m.main(command_args)
                            else:
                                self.assertEqual(m.main(command_args), exit_status)
                            self.assertEqual(spawn_mock.call_count, 1)
                    finally:
                        os.dup2(saved_output, 1)
                        os.dup2(saved_error, 2)
                        close_fd(saved_output)
                        close_fd(saved_error)
                    output_capture.seek(0)
                    error_capture.seek(0)
                    self.assertEqual(output_capture.read(), b"fixture output stream\n" if child_case == "complete" else b"")
                    self.assertEqual(error_capture.read(), b"fixture error stream\n" if child_case == "complete" else b"")
                self.assertIsNone(bystander.poll())
                self.assertEqual(input_fds, set())
                self.assertEqual(trace.count("input-close"), 2)
                self.assertLess(trace.index("wait-finished"), trace.index("group-finished"))
                self.assertLess(trace.index("group-finished"), trace.index("input-close"))
                self.assertLess(trace.index("input-close"), trace.index("selection-close"))
                if child_case == "cancel-at-start":
                    self.assertLess(trace.index("spawn-returned"), trace.index("group-join-entered"))
            finally:
                for signal_id, prior_handler in prior_handlers.items():
                    signal.signal(signal_id, prior_handler)
                for process in [*children, bystander]:
                    if process.poll() is None:
                        join_group(process)

    def test_generated_delayed_termination_cancellation_joins_before_input_and_authority_release(self):
        for child_case in ("cancel-at-wait", "cancel-at-start"):
            with self.subTest(mode=child_case):
                self.exercise_child_lifetime(child_case)

    def test_generated_success_failure_and_signal_exit_preserve_inherited_streams_and_status(self):
        for exit_status in (0, 23, -signal.SIGTERM):
            with self.subTest(returncode=exit_status):
                self.exercise_child_lifetime("complete", exit_status)


class NativeContract(unittest.TestCase):
    def test_preparation_holds_inputs_until_native_build_joins_and_propagates_failure(self):
        events = []

        @contextlib.contextmanager
        def prepared(run, *, platform):
            self.assertEqual(platform, "mac")
            self.assertEqual(run, "/generated/run")
            events.append("prepare")
            yield "/generated/exclusive-input"
            events.append("source-and-destination-rechecked")

        def native(argv, **kwargs):
            self.assertEqual(events, ["prepare", "remove-stale"])
            self.assertEqual(argv, ["/usr/bin/xcodebuild", *NATIVE_ARGS,
                                    "MEGURI_SIGNING=team", "MEGURI_CLOUD_AGENT_INJECTION=YES",
                                    "MEGURI_CLOUD_AGENT_INJECTION_DIR=/generated/exclusive-input"])
            self.assertFalse({"DEVELOPER_DIR", "SDKROOT", "TOOLCHAINS"} & set(kwargs["env"]))
            self.assertNotIn("check", kwargs)
            self.assertTrue(kwargs["start_new_session"])
            self.assertIsNone(kwargs.get("stdout"))
            self.assertIsNone(kwargs.get("stderr"))

            class Child:
                def wait(self):
                    events.append("native-joined")
                    return 23

            return Child()

        with patch.object(m, "prepare", prepared), \
                patch.object(m, "remove", side_effect=lambda path: events.append("remove-stale")), \
                patch.object(m.subprocess, "Popen", side_effect=native), \
                patch.object(m.stage, "join_original"), \
                patch.dict(os.environ, {"DEVELOPER_DIR": "/generated/override", "SDKROOT": "override", "TOOLCHAINS": "override"}):
            self.assertEqual(m.build("/generated/run", "/generated/product/Frameworks",
                                     [*NATIVE_ARGS, "MEGURI_SIGNING=team"]), 23)
        self.assertEqual(events, ["prepare", "remove-stale", "native-joined", "source-and-destination-rechecked"])

    def test_simulator_native_build_retains_exact_platform_source_until_join(self):
        args = list(NATIVE_ARGS)
        args[args.index("-destination") + 1] = "generic/platform=iOS Simulator"
        events = []
        @contextlib.contextmanager
        def prepared(run, *, platform):
            self.assertEqual(platform, "ios-sim")
            self.assertEqual(run, "/generated/run")
            events.append("simulator-source-held")
            yield "/generated/simulator-input"
            events.append("simulator-source-rechecked")
        class Child:
            def wait(self): events.append("native-joined"); return 0
        with patch.object(m, "prepare", prepared), patch.object(m, "remove") as remove, \
             patch.object(m.subprocess, "Popen", return_value=Child()) as launch, patch.object(m.stage, "join_original"):
            self.assertEqual(m.build("/generated/run", "/generated/product/Frameworks", args), 0)
        self.assertEqual(events, ["simulator-source-held", "native-joined", "simulator-source-rechecked"])
        remove.assert_called_once_with("/generated/product/Frameworks")
        self.assertEqual(launch.call_args.args[0], ["/usr/bin/xcodebuild", *args,
            "MEGURI_CLOUD_AGENT_INJECTION=YES", "MEGURI_CLOUD_AGENT_INJECTION_DIR=/generated/simulator-input"])
        for invalid in ([*args, "-destination", "platform=macOS,arch=arm64"],
                        [*args, "MEGURI_CLOUD_AGENT_EMBED=YES"]):
            with patch.object(m, "prepare") as prepare, self.assertRaises(m.stage.Refused):
                m.build("/generated/run", "/generated/product/Frameworks", invalid)
            prepare.assert_not_called()

    def test_cleanup_matrix_and_conflicting_native_settings_refuse(self):
        for flag in ("NO", "YES"):
            for platform in ("macosx", "iphonesimulator"):
                for config in ("Debug", "Profile"):
                    active = flag == "YES" and platform in ("macosx", "iphonesimulator") and config == "Debug"
                    env = {"MEGURI_CLOUD_AGENT_INJECTION": flag, "MEGURI_CLOUD_AGENT_EMBED": "YES" if active else "NO",
                           "PLATFORM_NAME": platform, "CONFIGURATION": config,
                           "TARGET_BUILD_DIR": "/generated/Debug", "FRAMEWORKS_FOLDER_PATH": "Meguri.app/Contents/Frameworks"}
                    with patch.object(m, "remove") as remove:
                        m.cleanup(env)
                        self.assertEqual(remove.call_count, int(not active))
                    env["MEGURI_CLOUD_AGENT_EMBED"] = "NO" if active else "YES"
                    with self.assertRaises(m.stage.Refused):
                        m.cleanup(env)

    def test_native_override_and_preparation_failure_never_start_build(self):
        with patch.object(m.subprocess, "Popen") as command, \
                patch.object(m, "prepare", side_effect=m.stage.Refused):
            for override in ([], ["archive"], list(NATIVE_ARGS),
                             [*NATIVE_ARGS, "MEGURI_CLOUD_AGENT_INJECTION_DIR=/unverified"],
                             [*NATIVE_ARGS, "EXCLUDED_SOURCE_FILE_NAMES="],
                             [*NATIVE_ARGS, "COPY_PHASE_STRIP=YES"]):
                with self.subTest(args=override), self.assertRaises(m.stage.Refused):
                    m.build("/generated/run", "/generated/Frameworks", override)
            command.assert_not_called()

    def test_non_agent_physical_and_non_debug_builds_never_prepare(self):
        for flag, value in (("-testPlan", "UI"), ("-configuration", "Profile"),
                            ("-destination", "generic/platform=iOS"), ("-scheme", "Meguri-Profile")):
            args = list(NATIVE_ARGS)
            args[args.index(flag) + 1] = value
            with self.subTest(flag=flag), patch.object(m, "prepare") as prepare, \
                    self.assertRaises(m.stage.Refused):
                m.build("/generated/run", "/generated/Frameworks", args)
            prepare.assert_not_called()

    def test_enabled_disabled_platform_configuration_matrix(self):
        for injection_setting in ("NO", "YES"):
            for target_platform in ("macosx", "iphonesimulator", "iphoneos"):
                for build_configuration in ("Debug", "Profile", "Release", "AppStore"):
                    build_environment = {"MEGURI_CLOUD_AGENT_INJECTION": injection_setting, "PLATFORM_NAME": target_platform,
                                         "CONFIGURATION": build_configuration}
                    self.assertEqual(m.enabled(build_environment), injection_setting == "YES" and target_platform in ("macosx", "iphonesimulator") and build_configuration == "Debug")
        with self.assertRaises(m.stage.Refused):
            m.enabled({"MEGURI_CLOUD_AGENT_INJECTION": "fixture-invalid-setting"})

    def test_native_copy_sign_order_and_final_freeze_remain_explicit(self):
        project = (ROOT / "app/Meguri.xcodeproj/project.pbxproj").read_text()
        app = (ROOT / "tools/mg.d/app.sh").read_text()
        config = (ROOT / "app/Config/Meguri.xcconfig").read_text()
        self.assertIn("libXCTestBundleInject.dylib in Embed CloudAgent injection", project)
        self.assertIn("ATTRIBUTES = (CodeSignOnCopy, );", project)
        phase = project.split("/* Embed CloudAgent injection */ = {", 1)[1].split("};", 1)[0]
        self.assertIn("isa = PBXCopyFilesBuildPhase;", phase)
        self.assertIn("dstSubfolderSpec = 10;", phase)
        target_phases = project.split("buildPhases = (", 1)[1].split(");", 1)[0]
        self.assertLess(target_phases.index("Exclude stale CloudAgent injection"), target_phases.index("Embed CloudAgent injection"))
        self.assertIn("MEGURI_CLOUD_AGENT_EMBED[config=Debug][sdk=macosx*] = $(MEGURI_CLOUD_AGENT_INJECTION)", config)
        self.assertIn("EXCLUDED_SOURCE_FILE_NAMES = $(inherited) $(MEGURI_CLOUD_AGENT_EXCLUDE_$(MEGURI_CLOUD_AGENT_EMBED))", config)
        self.assertIn("COPY_PHASE_STRIP = NO", (ROOT / "app/Config/Base.xcconfig").read_text())
        self.assertIn('"$config/$plan/$scheme" = Debug/CloudAgent/Meguri', app)
        self.assertIn('case "$platform" in mac|ios-sim) build_command=(mg_app_cloud_agent_build)', app)
        self.assertIn("MEGURI_CLOUD_AGENT_EMBED[config=Debug][sdk=iphonesimulator*] = $(MEGURI_CLOUD_AGENT_INJECTION)", config)
        self.assertIn("platformFilters = (ios, macos, );", project)
        jobs = app.split("mg_build_app() {", 1)[1]
        self.assertLess(jobs.index('mg_app_build "$platform"'), jobs.index('mg_app_agent_freeze "$platform"'))
        # The verifier's existing inventory must freeze final nested signatures.
        verifier = (ROOT / "tools/scripts/cloud-agent-verify.py").read_text()
        self.assertIn("libXCTestBundleInject.dylib", verifier)


if __name__ == "__main__":
    unittest.main()
