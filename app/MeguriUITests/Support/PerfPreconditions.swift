import Darwin
import Foundation
import XCTest

/// R1866's preconditions for performance tests (U241). `PerfTestCase`
/// checks them at setup, so a run that breaks one measures nothing:
///
/// - the bundle was built in Profile, U7's reading of R1866's "Release
///   configuration" (Release optimization plus `MEGURI_TEST_HOOKS`); Debug,
///   Release and AppStore bundles are refused;
/// - no debugger is attached to the test runner;
/// - neither the app under test nor the runner carries code coverage or a
///   sanitizer runtime;
/// - tests do not run in parallel (a parallel run tests on clones of the
///   simulator);
/// - the app reports a warmed web content process before measuring.
///
/// The Performance test plan and `tools/mg perf-ui` fix the same options
/// (no coverage, sanitizers, screenshots or parallel testing), `tools/mg
/// xcode-check` checks the plan, and `tools/mg perf-gate` refuses perf
/// results not built in Profile or whose tests kept attachments.
enum PerfPreconditions {
    struct Failure: Error, CustomStringConvertible {
        let description: String
    }

    /// What code coverage leaves in a Mach-O: the profile counters' section
    /// and the profile runtime's symbol.
    static let coverageMarkers = ["__llvm_prf_cnts", "___llvm_profile_runtime"]

    /// What a sanitizer leaves in a Mach-O or in `DYLD_INSERT_LIBRARIES`:
    /// its runtime's install name.
    static let sanitizerMarkers = ["libclang_rt.asan", "libclang_rt.tsan", "libclang_rt.ubsan"]

    /// Every precondition the runner can check by itself, on the app under
    /// test (its executable and dylibs) and this process.
    static func check(_ build: BuildUnderTest, environment: [String: String] = ProcessInfo.processInfo.environment) throws {
        try requireProfile(build.configuration)
        try requireNoDebugger(traced: isTraced())
        try requireUninstrumented(try binaries(build))
        try requireUninstrumented(environment: environment)
        try requireSerialTesting(environment)
    }

    /// The app's executable and dylibs, as (file name, contents). Coverage and
    /// sanitizers, when a plan turns them on, instrument the app as well as
    /// the runner; this test bundle is not scanned, because its own strings
    /// hold the markers.
    static func binaries(_ build: BuildUnderTest) throws -> [(String, Data)] {
        let files = try build.machOFiles
        guard !files.isEmpty else {
            throw Failure(description: "no executable in \(build.appBundle.path(percentEncoded: false)) to check (R1866)")
        }
        return try files.map { ($0.lastPathComponent, try Data(contentsOf: $0, options: .alwaysMapped)) }
    }

    static func requireProfile(_ configuration: BuildUnderTest.Configuration) throws {
        guard configuration == .profile else {
            throw Failure(
                description: "performance tests require Profile (R1866); this bundle was built in \(configuration.rawValue)")
        }
    }

    static func requireNoDebugger(traced: Bool) throws {
        if traced {
            throw Failure(description: "performance tests run without a debugger (R1866)")
        }
    }

    /// Whether a debugger traces this process.
    static func isTraced() -> Bool {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&mib, u_int(mib.count), &info, &size, nil, 0) == 0 else { return false }
        return info.kp_proc.p_flag & P_TRACED != 0
    }

    /// The app's binaries, as (file name, contents), carry no coverage
    /// counters and link no sanitizer runtime.
    static func requireUninstrumented(_ binaries: [(String, Data)]) throws {
        for (name, data) in binaries {
            if let marker = coverageMarkers.first(where: { data.range(of: Data($0.utf8)) != nil }) {
                throw Failure(description: "\(name) is built with code coverage (\(marker)); performance tests run without it (R1866)")
            }
            if let marker = sanitizerMarkers.first(where: { data.range(of: Data($0.utf8)) != nil }) {
                throw Failure(description: "\(name) links a sanitizer runtime (\(marker)); performance tests run without sanitizers (R1866)")
            }
        }
    }

    /// The runner's own environment: no injected sanitizer runtime. (Xcode
    /// sets `LLVM_PROFILE_FILE` for every test run, coverage or not, so code
    /// coverage shows only in the binaries.)
    static func requireUninstrumented(environment: [String: String]) throws {
        let injected = environment["DYLD_INSERT_LIBRARIES"] ?? ""
        if let marker = sanitizerMarkers.first(where: { injected.contains($0) }) {
            throw Failure(description: "the test runner has a sanitizer runtime injected (\(marker)); performance tests run without sanitizers (R1866)")
        }
    }

    /// Not a parallel run: parallel testing runs on clones of the simulator
    /// ("Clone 1 of iPhone 17 Pro").
    static func requireSerialTesting(_ environment: [String: String]) throws {
        if let device = environment["SIMULATOR_DEVICE_NAME"], device.hasPrefix("Clone ") {
            throw Failure(description: "tests run in parallel on \(device); performance tests run with parallel testing off (R1866)")
        }
    }

    /// Asks the running app for a warmed web content process and waits until
    /// it reports one ready: the notification
    /// `<bundle id>.testhooks.warmWebContent`, answered by
    /// `<bundle id>.testhooks.webContentReady` (`TestHooks.WebContentWarmup`).
    /// `post` delivers the request (Darwin on iOS, distributed on the Mac).
    @MainActor
    static func requireWarmWebContent(
        bundleIdentifier id: String, timeout: TimeInterval, post: @MainActor (String) -> Void
    ) throws {
        let ready = "\(id).testhooks.webContentReady"
        #if os(macOS)
        let answer = XCTNSNotificationExpectation(
            name: Notification.Name(ready), object: nil, notificationCenter: DistributedNotificationCenter.default())
        #else
        let answer = XCTDarwinNotificationExpectation(notificationName: ready)
        #endif
        post("\(id).testhooks.warmWebContent")
        guard XCTWaiter().wait(for: [answer], timeout: timeout) == .completed else {
            throw Failure(description: "the app reported no warmed web content process within \(timeout) s (R1866)")
        }
    }
}
