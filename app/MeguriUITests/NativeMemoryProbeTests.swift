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
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("NativeMemoryProbe-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
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
        keep(Data("Instrumented Debug diagnostic; synthetic spike fixtures; MallocStackLogging=lite; no performance receipt or acceptance. PID=\(pid)".utf8), name: "diagnostic.txt")
        for phase in ["baseline", "end"] {
            if phase == "end" {
                try validate()
                DistributedNotificationCenter.default().postNotificationName(
                    Notification.Name("\(build.expectedBundleIdentifier).testhooks.cardSwapBurst"),
                    object: nil, userInfo: nil, deliverImmediately: true
                )
            }
            let report = try waitForReport(app, timeout: phase == "baseline" ? 30 : 600, phase: phase)
            if phase == "end" {
                XCTAssertEqual(report.burst?.pages, 2)
                XCTAssertEqual(report.burst?.swaps, 1000)
            }
            for tool in phase == "baseline" ? [Tool.mallocHistory, .vmmap] : [.mallocHistory, .vmmap, .leaks] {
                try validate()
                try capture(tool, pid: pid, phase: phase, directory: directory)
            }
        }
        #else
        throw XCTSkip("native memory probe is macOS only")
        #endif
    }

    #if os(macOS)
    private enum Failure: Error { case invalid(String) }
    private enum Tool: String { case mallocHistory = "malloc_history", vmmap, leaks }

    private func identify(_ app: XCUIApplication, build: BuildUnderTest) throws -> NSRunningApplication {
        guard app.state == .runningForeground || app.state == .runningBackground else { throw Failure.invalid("app is not running") }
        let matches = NSRunningApplication.runningApplications(withBundleIdentifier: build.expectedBundleIdentifier)
        guard matches.count == 1, let running = matches.first, !running.isTerminated, running.processIdentifier > 0,
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

    private func capture(_ tool: Tool, pid: pid_t, phase: String, directory: URL) throws {
        let prefix = "\(phase)-\(tool.rawValue)"
        let stdout = directory.appendingPathComponent("\(prefix).stdout.txt")
        let stderr = directory.appendingPathComponent("\(prefix).stderr.txt")
        let graph = directory.appendingPathComponent("end.memgraph")
        try Data().write(to: stdout, options: .withoutOverwriting)
        try Data().write(to: stderr, options: .withoutOverwriting)
        let output = try FileHandle(forWritingTo: stdout)
        defer { try? output.close() }
        let errors = try FileHandle(forWritingTo: stderr)
        defer { try? errors.close() }
        let child = Process()
        child.executableURL = URL(filePath: "/usr/bin/\(tool.rawValue)")
        switch tool {
        case .mallocHistory: child.arguments = [String(pid), "-allBySize"]
        case .vmmap: child.arguments = ["-summary", String(pid)]
        case .leaks: child.arguments = [String(pid), "-noContent", "-outputGraph", graph.path]
        }
        child.standardOutput = output
        child.standardError = errors
        child.standardInput = FileHandle.nullDevice
        let finished = DispatchSemaphore(value: 0)
        child.terminationHandler = { _ in finished.signal() }
        var metadata = "\(child.executableURL!.path) \(child.arguments!.joined(separator: " "))\nlaunch incomplete"
        defer {
            keep(Data(metadata.utf8), name: "\(prefix)-exit.txt")
            attach(stdout)
            attach(stderr)
            if tool == .leaks, FileManager.default.fileExists(atPath: graph.path) { attach(graph, graph: true) }
        }
        do { try child.run() } catch {
            metadata += "\nlaunch error: \(error)"
            throw error
        }
        let timedOut = finished.wait(timeout: .now() + 30) == .timedOut
        if timedOut {
            if child.isRunning { child.terminate() }
            if finished.wait(timeout: .now() + 2) == .timedOut, child.isRunning {
                // Only this Process's still-running child, never the app being inspected.
                _ = Darwin.kill(child.processIdentifier, SIGKILL)
            }
        }
        child.waitUntilExit()
        metadata = "\(child.executableURL!.path) \(child.arguments!.joined(separator: " "))\nstatus=\(child.terminationStatus) reason=\(child.terminationReason.rawValue) timeout=\(timedOut)"
        guard !timedOut else { throw Failure.invalid("\(prefix) exceeded 30s; diagnostic child terminated") }
        guard child.terminationReason == .exit, child.terminationStatus == 0 || (tool == .leaks && child.terminationStatus == 1) else {
            throw Failure.invalid("\(prefix) failed: \(metadata)")
        }
        if tool == .leaks, !FileManager.default.fileExists(atPath: graph.path) { throw Failure.invalid("leaks produced no memgraph") }
    }

    private func attach(_ url: URL, graph: Bool = false) {
        do {
            let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
            if graph {
                guard size <= 128 * 1024 * 1024 else { throw Failure.invalid("memgraph exceeds 128 MiB (\(size) bytes)") }
                let attachment = XCTAttachment(contentsOfFile: url)
                attachment.name = "instrumented-Debug-diagnostic-\(url.lastPathComponent)"
                attachment.lifetime = .keepAlways
                add(attachment)
            } else {
                let file = try FileHandle(forReadingFrom: url)
                defer { try? file.close() }
                let data = try file.read(upToCount: 8 * 1024 * 1024) ?? Data()
                keep(data, name: url.lastPathComponent + (size > data.count ? "-truncated-from-\(size)-bytes" : ""))
            }
        } catch { XCTFail("diagnostic attachment \(url.lastPathComponent): \(error)") }
    }

    private func keep(_ data: Data, name: String) {
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.plain-text")
        attachment.name = "instrumented-Debug-diagnostic-\(name)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
    #endif
}
