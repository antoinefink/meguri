import Foundation
import XCTest
#if os(macOS)
import AppKit
import Darwin
#endif

/// Temporary, instrumented Debug diagnosis on synthetic spike fixtures; no performance receipt or acceptance.
@MainActor
final class NativeMemoryProbeTests: XCTestCase {
    func testCaptureStacks() throws {
        guard ProcessInfo.processInfo.environment["MEGURI_NATIVE_MEMORY_PROBE"] == "1" else {
            throw XCTSkip("native memory probe requires explicit opt-in")
        }
        #if os(macOS)
        var virtual: Int32 = 0
        var size = MemoryLayout<Int32>.size
        guard sysctlbyname("kern.hv_vmm_present", &virtual, &size, nil, 0) == 0, virtual == 1 else {
            throw XCTSkip("native memory probe runs only in the test VM")
        }
        let build = try BuildUnderTest.current()
        guard build.configuration == .debug else { throw XCTSkip("native memory probe requires Debug") }
        guard try build.bundleIdentifier == build.expectedBundleIdentifier else { throw Failure.invalid("build bundle ID") }
        guard NSRunningApplication.runningApplications(withBundleIdentifier: build.expectedBundleIdentifier).isEmpty else {
            throw Failure.invalid("refusing to replace an existing app")
        }
        let app = XCUIApplication()
        app.launchArguments = MeguriLaunch(animations: false, route: "spike/card-swap").arguments
        app.launchEnvironment["MallocStackLogging"] = "lite"
        defer { app.terminate() }
        app.launch()
        let running = try identify(app, build: build)
        let pid = running.processIdentifier
        func validate() throws {
            let current = try identify(app, build: build)
            guard !running.isTerminated, current.processIdentifier == pid, current.launchDate == running.launchDate else {
                throw Failure.invalid("launched app identity changed")
            }
        }
        func checkpoint(_ phase: String) throws {
            try validate()
            let proceed = XCTNSNotificationExpectation(
                name: Notification.Name("com.antoinefink.meguri.dev.testhooks.nativeMemoryProbeContinue"),
                object: nil, notificationCenter: DistributedNotificationCenter.default()
            )
            print("MEGURI_NATIVE_MEMORY_CHECKPOINT phase=\(phase) pid=\(pid)")
            guard XCTWaiter.wait(for: [proceed], timeout: 180) == .completed else {
                throw Failure.invalid("\(phase) checkpoint did not receive continuation within 180s")
            }
            try validate()
        }
        keep(Data("Instrumented Debug diagnostic; synthetic spike fixtures; MallocStackLogging=lite; no performance receipt or acceptance. PID=\(pid)".utf8), name: "diagnostic.txt")
        _ = try waitForReport(app, timeout: 30, phase: "baseline")
        try checkpoint("baseline")
        try validate()
        DistributedNotificationCenter.default().postNotificationName(
            Notification.Name("\(build.expectedBundleIdentifier).testhooks.cardSwapBurst"),
            object: nil, userInfo: nil, deliverImmediately: true
        )
        let report = try waitForReport(app, timeout: 600, phase: "end")
        XCTAssertEqual(report.burst?.pages, 2)
        XCTAssertEqual(report.burst?.swaps, 1000)
        try checkpoint("end")
        #else
        throw XCTSkip("native memory probe is macOS only")
        #endif
    }

    #if os(macOS)
    private enum Failure: Error { case invalid(String) }

    private func identify(_ app: XCUIApplication, build: BuildUnderTest) throws -> NSRunningApplication {
        guard app.state == .runningForeground || app.state == .runningBackground else { throw Failure.invalid("app is not running") }
        let matches = NSRunningApplication.runningApplications(withBundleIdentifier: build.expectedBundleIdentifier)
        guard matches.count == 1, let running = matches.first, !running.isTerminated, running.processIdentifier > 0,
              running.launchDate != nil,
              running.bundleIdentifier == build.expectedBundleIdentifier,
              running.bundleURL?.resolvingSymlinksInPath() == build.appBundle.resolvingSymlinksInPath(),
              let executable = Bundle(url: build.appBundle)?.executableURL,
              running.executableURL?.resolvingSymlinksInPath() == executable.resolvingSymlinksInPath() else {
            throw Failure.invalid("no unique launched app with the exact build bundle and executable")
        }
        return running
    }

    private func waitForReport(_ app: XCUIApplication, timeout: TimeInterval, phase: String) throws -> CardSwapTimingSpikeTests.Report {
        let element = app.staticTexts["spike.cardSwap.report"]
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            guard app.state == .runningForeground || app.state == .runningBackground else { throw Failure.invalid("app exited awaiting report") }
            if element.exists {
                let raw = element.label.isEmpty ? (element.value as? String ?? "") : element.label
                guard raw.utf8.count <= 64 * 1024 else { throw Failure.invalid("unexpectedly large spike report") }
                if let report = try? JSONDecoder().decode(CardSwapTimingSpikeTests.Report.self, from: Data(raw.utf8)),
                   phase == "baseline" ? report.ready : report.burst?.swaps == 1000 {
                    keep(Data(raw.utf8), name: "\(phase)-report.json")
                    return report
                }
            }
            Thread.sleep(forTimeInterval: 0.3)
        } while Date() < deadline
        throw Failure.invalid("\(phase) report readiness timed out after \(timeout)s")
    }

    private func keep(_ data: Data, name: String) {
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.plain-text")
        attachment.name = "instrumented-Debug-diagnostic-\(name)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
    #endif
}
