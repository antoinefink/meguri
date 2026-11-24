import Foundation
import XCTest
#if os(macOS)
import AppKit
#endif

/// The inline transport is private and uses the existing parser and driver.
/// Never surface DriveScriptError: its descriptions may contain caller input.
enum DriveScriptInput {
    enum Failure: String, Error, CustomStringConvertible {
        case missing = "drive input missing"
        case conflicting = "drive input sources conflict"
        case encoding = "drive input encoding invalid"
        case oversized = "drive input too large"
        case json = "drive input JSON invalid"
        case policy = "drive input physical policy refused"
        case grammar = "drive input grammar invalid"
        var description: String { rawValue }
    }

    case path(URL)
    case inline(DriveScript)

    static var physicalDevice: Bool {
        #if os(iOS) && !targetEnvironment(simulator)
        true
        #else
        false
        #endif
    }

    static func load(_ environment: [String: String], physical: Bool = physicalDevice) throws -> Self? {
        if let keep = environment["MEGURI_DRIVE_KEEP_APP_RUNNING"] {
            guard keep == "0" || (keep == "1" && physical && environment["MEGURI_DRIVE_SCRIPT_BASE64"] != nil) else {
                throw Failure.policy
            }
        }
        let path = environment["MEGURI_DRIVE_SCRIPT"]
        let encoded = environment["MEGURI_DRIVE_SCRIPT_BASE64"]
        if path != nil, encoded != nil { throw Failure.conflicting }
        if let encoded {
            guard !encoded.isEmpty else { throw Failure.missing }
            guard encoded.utf8.count <= 21_848 else { throw Failure.oversized }
            guard let data = Data(base64Encoded: encoded), data.base64EncodedString() == encoded else {
                throw Failure.encoding
            }
            guard data.count <= 16_384 else { throw Failure.oversized }
            guard String(data: data, encoding: .utf8) != nil else { throw Failure.encoding }
            let object: Any
            do { object = try JSONSerialization.jsonObject(with: data) }
            catch { throw Failure.json }
            guard let root = object as? [String: Any], let steps = root["steps"] as? [[String: Any]] else {
                throw Failure.grammar
            }
            // Inspect raw objects before DTO defaults can hide supplied keys.
            for step in steps {
                guard step["resize"] == nil, step["transition"] == nil else { throw Failure.policy }
                if let launch = step["launch"] {
                    guard let fields = launch as? [String: Any], fields.isEmpty else { throw Failure.policy }
                }
            }
            guard environment["MEGURI_DRIVE_FRAMES"] != "1" else { throw Failure.policy }
            do { return .inline(try DriveScript(json: data)) }
            catch { throw Failure.grammar }
        }
        if let path, !path.isEmpty {
            guard !physical else { throw Failure.policy }
            return .path(URL(filePath: path))
        }
        if physical || environment["MEGURI_DRIVE_REQUIRED"] == "1" { throw Failure.missing }
        return nil
    }
}

/// Harness policy only. Activation never falls back to launch, including when
/// XCTest cannot determine the target's state. Public process evidence belongs
/// to MG; a successful activate() alone cannot set activated_existing.
enum DriveAppLifecycle {
    static func preserve(_ environment: [String: String], physical: Bool,
                         configuration: BuildUnderTest.Configuration, bundleIdentifier: String) throws -> Bool {
        guard environment["MEGURI_DRIVE_KEEP_APP_RUNNING"] == "1" else { return false }
        guard physical, configuration == .debug, bundleIdentifier == "com.antoinefink.meguri.dev" else {
            throw DriveScriptInput.Failure.policy
        }
        return true
    }

    static func start(preserve: Bool, activate: () -> Void, launch: () -> Void) {
        if preserve { activate() } else { launch() }
    }

    static func finish(preserve: Bool, terminate: () -> Void) {
        if !preserve { terminate() }
    }

    static func isRunningInBackground(_ state: XCUIApplication.State) -> Bool {
        #if os(iOS)
        state == .runningBackground || state == .runningBackgroundSuspended
        #else
        state == .runningBackground
        #endif
    }

    /// No launch fallback: a failed Home transition or stopped process must
    /// leave activation untouched. The driver supplies bounded XCTest waits.
    @MainActor
    static func foregroundCycle(state: () -> XCUIApplication.State, home: () -> Void,
                                waitForBackground: () -> Bool, activate: () -> Void,
                                waitForForeground: () -> Bool) throws {
        guard state() == .runningForeground else {
            throw AgentDriver.StepFailure(message: "foregroundCycle requires the running foreground app")
        }
        home()
        guard waitForBackground() else {
            throw AgentDriver.StepFailure(message: "the app did not reach the background within 10 s")
        }
        let background = state()
        guard isRunningInBackground(background) else {
            throw AgentDriver.StepFailure(message: "the original app is no longer running in the background")
        }
        activate()
        guard waitForForeground(), state() == .runningForeground else {
            throw AgentDriver.StepFailure(message: "the app did not return to the foreground within 10 s")
        }
    }
}

/// Runs one agent drive script (U8, R1961, R1965). `tools/mg drive` passes the
/// path or private inline bytes through the test-runner environment
/// (`TEST_RUNNER_` prefix); without a requested input the test skips, so the UI
/// plan stays runnable on its own.
///
/// Before and after every step the driver attaches the hierarchy
/// (`HierarchyExport`) and a screenshot; a launch step has only an after
/// capture and a snapshot step a single one under its name. Taps go to the
/// hit point the hierarchy gives for the identifier, never to a guessed
/// position. At the end it attaches `drive-log.json`, which names every
/// capture's attachments and each step's timing; `tools/mg drive` exports
/// them from the result bundle. With `MEGURI_DRIVE_FRAMES=1` (`tools/mg drive
/// --platform mac --record` without Screen Recording) a step marked with a
/// transition also gets fixed-rate frames (`FrameSampler`), listed under the
/// step's `frames`.
@MainActor
final class AgentDriverTests: XCTestCase {
    func testDriveScript() throws {
        let input: DriveScriptInput?
        do { input = try DriveScriptInput.load(ProcessInfo.processInfo.environment) }
        catch {
            let category = (error as? DriveScriptInput.Failure)?.rawValue ?? "drive input refused"
            let log = AgentDriver.Log(script: "physical-drive", path: "", platform: AgentDriver.platform,
                                      kept_running: ProcessInfo.processInfo.environment["MEGURI_DRIVE_KEEP_APP_RUNNING"] == "1",
                                      error: category)
            let attachment = XCTAttachment(data: try JSONEncoder().encode(log), uniformTypeIdentifier: "public.json")
            attachment.name = "drive-log.json"
            attachment.lifetime = .keepAlways
            add(attachment)
            return
        }
        guard let input else {
            throw XCTSkip("no drive script: tools/mg drive passes it as MEGURI_DRIVE_SCRIPT")
        }
        // Step failures live in the drive log so captures survive a failure.
        continueAfterFailure = true
        switch input {
        case let .path(url):
            AgentDriver(build: try BuildUnderTest.current(), test: self).run(scriptAt: url)
        case let .inline(script):
            let build = try BuildUnderTest.current()
            let preserve = try DriveAppLifecycle.preserve(ProcessInfo.processInfo.environment,
                physical: DriveScriptInput.physicalDevice, configuration: build.configuration,
                bundleIdentifier: try build.bundleIdentifier)
            AgentDriver(build: build, test: self, privateInput: true, preserve: preserve).run(script)
        }
    }

    /// Mac: a resize step resizes the window of the app that is running. The
    /// app is not relaunched, so it keeps its state: the same process, the
    /// sheet its route presented. The sizes are the launch sizes (R1962):
    /// 800 points wide at `minimum` (800x560, taller only when the content
    /// needs it), the visible frame of the window's display at `large`, and
    /// 1180x780 at `default`. Runs only through `tools/mg mac-ui`.
    func testResizeKeepsTheRunningAppAndItsState() throws {
        #if os(macOS)
        let build = try BuildUnderTest.current()
        guard build.hasHooks else { throw XCTSkip("resize needs a hook build (Debug or Profile)") }
        let id = try build.bundleIdentifier
        let driver = AgentDriver(build: build, test: self)
        var options = DriveScript.Launch()
        options.route = "sample/sheet"
        options.animations = false
        options.windowSize = "default"
        driver.run(DriveScript(name: "launch", steps: [DriveScript.Step(action: .launch(options))]))
        XCTAssertEqual(driver.log.steps.map(\.status), ["ok"])
        let app = XCUIApplication()
        XCTAssert(app.staticTexts["sample.sheet.title"].waitForExistence(timeout: 15), "the sample sheet is not on screen")
        let pid = try XCTUnwrap(Self.processIdentifier(id))
        let window = app.windows.firstMatch
        Self.assertSize(window.frame, CGSize(width: 1180, height: 780), "after the launch")

        func resize(_ size: String) {
            driver.run(DriveScript(name: "resize-\(size)", steps: [DriveScript.Step(action: .resize(size: size))]))
            XCTAssertEqual(driver.log.steps.map(\.status), ["ok"], "resize \(size): \(driver.log.steps.first?.message ?? "")")
            XCTAssertEqual(Self.processIdentifier(id), pid, "resize \(size) relaunched the app")
            XCTAssert(app.staticTexts["sample.sheet.title"].exists, "the sheet is gone after resize \(size)")
        }

        resize("minimum")
        XCTAssertEqual(window.frame.width, 800, accuracy: 0.5)
        XCTAssert((560..<780).contains(window.frame.height), "minimum: height \(window.frame.height)")

        resize("large")
        let visible = try XCTUnwrap(Self.visibleFrame(around: window.frame), "no display holds the window")
        XCTAssertEqual(window.frame.minX, visible.minX, accuracy: 0.5)
        XCTAssertEqual(window.frame.minY, visible.minY, accuracy: 0.5)
        Self.assertSize(window.frame, visible.size, "large")

        resize("default")
        Self.assertSize(window.frame, CGSize(width: 1180, height: 780), "default")
        #else
        throw XCTSkip("resizing a window is Mac only; on simulators the step is reported unavailable")
        #endif
    }

    #if os(macOS)
    private static func processIdentifier(_ bundleIdentifier: String) -> pid_t? {
        NSRunningApplication.runningApplications(withBundleIdentifier: bundleIdentifier)
            .first { !$0.isTerminated }?.processIdentifier
    }

    /// The visible frame of the display that holds most of `frame`, both in
    /// XCUITest's screen points (top-left origin on the main display).
    private static func visibleFrame(around frame: CGRect) -> CGRect? {
        guard let height = NSScreen.screens.first?.frame.height else { return nil }
        func flipped(_ rect: CGRect) -> CGRect {
            CGRect(x: rect.minX, y: height - rect.maxY, width: rect.width, height: rect.height)
        }
        func area(_ rect: CGRect) -> CGFloat { rect.isNull ? 0 : rect.width * rect.height }
        return NSScreen.screens
            .max { area(flipped($0.frame).intersection(frame)) < area(flipped($1.frame).intersection(frame)) }
            .map { flipped($0.visibleFrame) }
    }

    private static func assertSize(_ frame: CGRect, _ size: CGSize, _ when: String, line: UInt = #line) {
        XCTAssertEqual(frame.width, size.width, accuracy: 0.5, "\(when): width", line: line)
        XCTAssertEqual(frame.height, size.height, accuracy: 0.5, "\(when): height", line: line)
    }
    #endif
}

@MainActor
final class AgentDriver {
    struct Capture: Encodable {
        let phase: String
        let hierarchy: String
        let screenshot: String
    }

    struct FrameRecord: Encodable {
        let file: String
        let time: Double
    }

    struct StepRecord: Encodable {
        let index: Int
        let kind: String
        let transition: String?
        var started: Double
        var ended: Double = 0
        var status = "ok"
        var message: String?
        var captures: [Capture] = []
        /// Sampled frames (`FrameSampler`), oldest first.
        var frames: [FrameRecord]?
        /// Set when the step outlasted `FrameSampler.maxFrames`.
        var framesTruncated: Bool?
    }

    struct Log: Encodable {
        var script: String
        var path: String
        var platform: String
        var kept_running = false
        var activated_existing = false
        // XCTest activation completed; MG combines this with physical
        // process/installation brackets to attest activated_existing.
        var activation_completed = false
        var steps: [StepRecord] = []
        var error: String?
    }

    /// A step that cannot run here (not a failure).
    struct Unavailable: Error {
        let reason: String
    }

    struct StepFailure: Error {
        let message: String
    }

    private let build: BuildUnderTest
    private let privateInput: Bool
    private let preserve: Bool
    private unowned let test: XCTestCase
    /// Fixed-rate frames around marked steps (`FrameSampler`).
    private let sampleFrames = ProcessInfo.processInfo.environment["MEGURI_DRIVE_FRAMES"] == "1"
    private var app: XCUIApplication?
    private var tree: HierarchyExport.Tree?
    private(set) var log: Log

    static var platform: String {
        #if os(macOS)
        "mac"
        #else
        "ios"
        #endif
    }

    init(build: BuildUnderTest, test: XCTestCase, privateInput: Bool = false, preserve: Bool = false) {
        self.privateInput = privateInput
        self.preserve = preserve
        self.build = build
        self.test = test
        log = Log(script: "", path: "", platform: Self.platform)
    }

    func run(scriptAt url: URL) {
        let path = url.path(percentEncoded: false)
        let script: DriveScript
        do {
            script = try DriveScript(contentsOf: url)
        } catch {
            log = Log(script: "", path: path, platform: Self.platform, error: error.message)
            attachLog()
            return
        }
        run(script, path: path)
    }

    /// Runs the steps with a fresh log. The app the driver launched stays
    /// with it from one run to the next.
    func run(_ script: DriveScript, path: String = "") {
        log = Log(script: privateInput ? "physical-drive" : script.name, path: path, platform: Self.platform,
                  kept_running: preserve)
        defer { attachLog() }
        for (offset, step) in script.steps.enumerated() {
            guard perform(step, index: offset + 1) else { return }
        }
    }

    // MARK: Steps

    private func perform(_ step: DriveScript.Step, index: Int) -> Bool {
        let kind = step.action.kind
        var record = StepRecord(index: index, kind: kind, transition: step.transition, started: Date().timeIntervalSince1970)
        let failuresBefore = test.testRun?.totalFailureCount ?? 0
        let foregroundCycle = step.action == .foregroundCycle
        // Keep attachment names within the physical export's lowercase guard.
        let base = String(format: "drive-%02d-%@", index, foregroundCycle ? "foreground-cycle" : kind)
        var failed = false
        var sampler: FrameSampler?
        do {
            switch step.action {
            case .launch, .snapshot:
                break
            case .foregroundCycle:
                #if os(iOS)
                record.captures.append(try capture(base, phase: "before", foregroundOnly: true))
                #else
                throw Unavailable(reason: "foregroundCycle is iOS only")
                #endif
            default:
                try prepareInput(step.action)
                record.captures.append(try capture(base, phase: "before"))
            }
            sampler = try startSampler(for: step)
            record.started = Date().timeIntervalSince1970
            try execute(step.action)
            record.ended = Date().timeIntervalSince1970
        } catch let unavailable as Unavailable {
            record.ended = Date().timeIntervalSince1970
            record.status = "unavailable"
            record.message = unavailable.reason
        } catch {
            record.ended = Date().timeIntervalSince1970
            record.status = "failed"
            record.message = (error as? StepFailure)?.message ?? "\(error)"
            failed = true
        }
        if let sampler {
            sampler.finish()
            record.frames = attachFrames(sampler, base: base)
            record.framesTruncated = sampler.truncated ? true : nil
        }
        // A failed Home cycle may leave the app in the background or dead.
        // Do not query its hierarchy or capture the screen in that state.
        if app != nil, !foregroundCycle || (Self.platform == "ios" && app?.state == .runningForeground) {
            do {
                if case let .snapshot(name) = step.action {
                    record.captures.append(try capture("\(base)-\(name)", phase: name))
                } else {
                    record.captures.append(try capture(base, phase: "after", foregroundOnly: foregroundCycle))
                }
            } catch {
                if !failed {
                    record.status = "failed"
                    record.message = "capture failed: \(error)"
                    failed = true
                }
            }
        }
        // A failure XCTest recorded itself (an action it could not perform).
        if !failed, (test.testRun?.totalFailureCount ?? 0) > failuresBefore {
            record.status = "failed"
            record.message = "XCTest recorded a failure during the step; see drive.log"
            failed = true
        }
        if privateInput, record.status != "ok" { record.message = "drive step failed" }
        log.steps.append(record)
        if failed {
            // Not XCTFail: on a test failure XCTest attaches the app's whole
            // UI hierarchy, which on the Mac lists the user's recent files.
            print("drive: step \(index) (\(kind)) failed: \(record.message ?? "")")
            return false
        }
        return true
    }

    /// Mac: another app is usually in front, and input must reach a key
    /// window (`bringToFront`). The app comes to the front before the step's
    /// 'before' capture, so that capture shows the window the action reaches.
    private func prepareInput(_ action: DriveScript.Action) throws {
        #if os(macOS)
        switch action {
        case .tap, .type, .swipe, .scroll, .custom:
            try running().bringToFront(bundleIdentifier: try build.bundleIdentifier)
        default:
            break
        }
        #endif
    }

    private func execute(_ action: DriveScript.Action) throws {
        switch action {
        case let .launch(options):
            try launch(options)
        case let .tap(id):
            #if os(macOS)
            if let native = try currentTree().nativeWeb, native.binding.generatedIDs.contains(id) {
                _ = try onlyElement(id)
                try native.press(id, in: running())
                return
            }
            #endif
            try coordinate(at: try hitPoint(id)).clickOrTap()
        case let .type(id, text):
            try coordinate(at: try hitPoint(id)).clickOrTap()
            try running().typeText(text)
        case let .swipe(id, direction):
            try drag(id, direction, scroll: false)
        case let .scroll(id, direction):
            try drag(id, direction, scroll: true)
        case let .wait(id, timeout, gone):
            #if os(macOS)
            if let binding = try NativeWebCapture.binding(in: running()), binding.generatedIDs.contains(id) {
                try NativeWebCapture.wait(id, in: running(), binding: binding, timeout: timeout, gone: gone)
                return
            }
            #endif
            let element = try running().descendants(matching: .any).matching(identifier: id).firstMatch
            if gone {
                guard element.waitForNonExistence(timeout: timeout) else {
                    throw StepFailure(message: "'\(id)' is still there after \(timeout) s")
                }
            } else {
                guard element.waitForExistence(timeout: timeout) else {
                    throw StepFailure(message: "no element '\(id)' after \(timeout) s")
                }
            }
        case .snapshot:
            _ = try running()
        case let .resize(size):
            try resize(size)
        case .foregroundCycle:
            try foregroundCycle()
        case let .custom(command):
            try custom(command)
        }
    }

    /// Screen-library capture steps that need no unique identifier.
    private func custom(_ command: String) throws {
        let parts = command.split(separator: ":", maxSplits: 1).map(String.init)
        guard parts.count == 2 else { throw StepFailure(message: "do takes <op>:<argument>") }
        let app = try running(), argument = parts[1]
        func labelled(_ label: String) throws -> XCUIElement {
            let pieces = label.components(separatedBy: "#")
            let typed = pieces[0].components(separatedBy: "@")
            let types: [String: XCUIElement.ElementType] = ["textView": .textView, "button": .button, "switch": .switch, "staticText": .staticText, "textField": .textField, "menuBarItem": .menuBarItem, "menuItem": .menuItem, "searchField": .searchField]
            let query = app.descendants(matching: typed.count > 1 ? types[typed[1]] ?? .any : .any).matching(NSPredicate(format: "label == %@ OR title == %@ OR placeholderValue == %@", typed[0], typed[0], typed[0]))
            let element = pieces.count > 1 ? query.element(boundBy: Int(pieces[1]) ?? 0) : query.firstMatch
            guard element.waitForExistence(timeout: 5) else { throw StepFailure(message: "no element labelled '\(label)'") }
            return element
        }
        switch parts[0] {
        case "label":
            try labelled(argument).coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).clickOrTap()
        case "press":
            let element = app.descendants(matching: .any).matching(identifier: argument).firstMatch
            guard element.waitForExistence(timeout: 5) else { throw StepFailure(message: "no element '\(argument)'") }
            #if os(macOS)
            element.rightClick()
            #else
            element.press(forDuration: 1.2)
            #endif
        case "pressLabel":
            #if os(macOS)
            try labelled(argument).rightClick()
            #else
            try labelled(argument).press(forDuration: 1.2)
            #endif
        case "keys":
            app.typeText(argument)
        case "point":
            let values = argument.split(separator: ",").compactMap { Double($0) }
            guard values.count == 2 else { throw StepFailure(message: "point takes x,y in 0...1") }
            app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: values[0], dy: values[1])).clickOrTap()
        case "sleep":
            Thread.sleep(forTimeInterval: Double(argument) ?? 1)
        #if os(iOS)
        case "orientation":
            let orientations: [String: UIDeviceOrientation] = ["portrait": .portrait, "landscapeLeft": .landscapeLeft, "landscapeRight": .landscapeRight]
            guard let orientation = orientations[argument] else { throw StepFailure(message: "unknown orientation") }
            XCUIDevice.shared.orientation = orientation
            Thread.sleep(forTimeInterval: 1.5)
        case "appearance":
            XCUIDevice.shared.appearance = argument == "dark" ? .dark : .light
            Thread.sleep(forTimeInterval: 1)
        #endif
        default:
            throw StepFailure(message: "unknown do op '\(parts[0])'")
        }
        tree = nil
    }

    private func foregroundCycle() throws {
        #if os(iOS)
        let app = try running()
        tree = nil
        try DriveAppLifecycle.foregroundCycle(state: { app.state }, home: {
            XCUIDevice.shared.press(.home)
        }, waitForBackground: {
            let background = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                let state = app.state
                return DriveAppLifecycle.isRunningInBackground(state)
            }, object: nil)
            return XCTWaiter().wait(for: [background], timeout: 10) == .completed
        }, activate: {
            app.activate()
        }, waitForForeground: {
            app.wait(for: .runningForeground, timeout: 10)
        })
        #else
        throw Unavailable(reason: "foregroundCycle is iOS only")
        #endif
    }

    private func launch(_ options: DriveScript.Launch) throws {
        #if os(macOS)
        // A build without the .dev id would replace the daily-driver app.
        let id = try build.bundleIdentifier
        guard id.hasSuffix(".dev") else {
            throw StepFailure(message: "never launch \(id) on the Mac: it would replace the daily-driver app")
        }
        #endif
        if !preserve { app?.terminate() }
        let app = preserve ? XCUIApplication(bundleIdentifier: try build.bundleIdentifier) : XCUIApplication()
        self.app = app
        // Register before starting: failed activation, launch, step and capture
        // paths all share this cleanup policy. The runner itself cleans up normally.
        let preserve = self.preserve
        test.addTeardownBlock { @MainActor in
            DriveAppLifecycle.finish(preserve: preserve) { app.terminate() }
        }
        if !preserve { app.launchArguments = privateInput ? [] : arguments(options) }
        DriveAppLifecycle.start(preserve: preserve, activate: { app.activate() }, launch: { app.launch() })
        guard app.wait(for: .runningForeground, timeout: 30) else {
            throw StepFailure(message: "the app is not running in the foreground 30 s after launch")
        }
        if preserve { log.activation_completed = true }
        try settleIdentifiers(app)
    }

    /// Waits, at most `limit` seconds, until the hierarchy's identifiers
    /// have not changed for `quiet` seconds. SwiftUI gives some UIKit views
    /// theirs a moment after launch: the first hierarchy shows an iPhone tab
    /// bar's buttons without them (U150). Two equal hierarchies 0.25 s apart
    /// were not enough: in 2 of 5 iPhone drives the identifiers came just
    /// after such a pair (2026-10-16). A launch's capture is the settled
    /// state, never that transient one (R1965).
    private func settleIdentifiers(_ app: XCUIApplication, quiet: TimeInterval = 1, limit: TimeInterval = 5) throws {
        func identifiers() throws -> [String] {
            try HierarchyExport.tree(app).root.allNodes().map(\.identifier)
        }
        let deadline = Date().addingTimeInterval(limit)
        var previous = try identifiers()
        var unchangedSince = Date()
        while Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.25))
            let current = try identifiers()
            if current != previous {
                previous = current
                unchangedSince = Date()
            } else if Date().timeIntervalSince(unchangedSince) >= quiet {
                return
            }
        }
    }

    private func arguments(_ options: DriveScript.Launch) -> [String] {
        let fixture = options.fixture.map { path in
            path.hasPrefix("/") ? URL(filePath: path) : build.repositoryRoot.appending(path: path)
        }
        return MeguriLaunch(
            fixture: fixture, now: options.now, timeZone: options.timeZone, locale: options.locale,
            animations: options.animations, reduceMotion: options.reduceMotion,
            reduceTransparency: options.reduceTransparency, windowSize: options.windowSize,
            ankiConnectPort: options.ankiConnectPort, migakuPort: options.migakuPort,
            route: options.route, extra: options.arguments
        ).arguments
    }

    /// Mac: resizes the running app's window, as the `windowSize` launch
    /// option would (R1962), without a relaunch: the app keeps its state.
    /// The hook build does it on a distributed notification and answers
    /// (`TestHooks.AppearanceOverrides.WindowHookView`, app/AGENTS.md).
    /// Simulators have no resizable iPhone window in this toolchain.
    private func resize(_ size: String) throws {
        #if os(macOS)
        _ = try running()
        let id = try build.bundleIdentifier
        let center = DistributedNotificationCenter.default()
        let resized = XCTNSNotificationExpectation(
            name: Notification.Name("\(id).testhooks.windowResized"), object: nil, notificationCenter: center
        )
        // Delivered at once: the app is usually not the active app.
        center.postNotificationName(
            Notification.Name("\(id).testhooks.resizeWindow"), object: size, userInfo: nil, deliverImmediately: true
        )
        guard XCTWaiter().wait(for: [resized], timeout: 10) == .completed else {
            throw StepFailure(message: "the app did not resize its window to \(size) within 10 s")
        }
        tree = nil
        #else
        throw Unavailable(reason: "resizable iPhone windows: devicectl appResize is not confirmed on simulators with this toolchain")
        #endif
    }

    private func drag(_ id: String, _ direction: DriveScript.Direction, scroll: Bool) throws {
        let point = try hitPoint(id)
        let element = try onlyElement(id)
        let start = try coordinate(at: point)
        #if os(macOS)
        if scroll {
            let delta: CGFloat = 200
            switch direction {
            case .up: start.scroll(byDeltaX: 0, deltaY: delta)
            case .down: start.scroll(byDeltaX: 0, deltaY: -delta)
            case .left: start.scroll(byDeltaX: delta, deltaY: 0)
            case .right: start.scroll(byDeltaX: -delta, deltaY: 0)
            }
            return
        }
        #endif
        // A swipe moves the finger in its direction; a scroll moves the view
        // through the content in its direction (down shows what is below), so
        // the finger goes the other way.
        let dx = element.frame.width * 0.3, dy = element.frame.height * 0.3
        var vector: CGVector
        switch direction {
        case .up: vector = CGVector(dx: 0, dy: -dy)
        case .down: vector = CGVector(dx: 0, dy: dy)
        case .left: vector = CGVector(dx: -dx, dy: 0)
        case .right: vector = CGVector(dx: dx, dy: 0)
        }
        if scroll { vector = CGVector(dx: -vector.dx, dy: -vector.dy) }
        let end = start.withOffset(vector)
        #if os(macOS)
        start.click(forDuration: 0.05, thenDragTo: end)
        #else
        start.press(forDuration: 0.05, thenDragTo: end, withVelocity: scroll ? .default : .fast, thenHoldForDuration: 0)
        #endif
    }

    // MARK: Hierarchy

    private func running() throws -> XCUIApplication {
        guard let app else { throw StepFailure(message: "the app is not launched; start the script with a launch step") }
        return app
    }

    /// The one element with this identifier in the latest hierarchy.
    private func onlyElement(_ id: String) throws -> HierarchyExport.Node {
        let tree = try currentTree()
        let matches = tree.elements(id)
        guard !matches.isEmpty else {
            throw StepFailure(message: "no element '\(id)' in the hierarchy")
        }
        guard matches.count == 1 else {
            throw StepFailure(message: "'\(id)' names \(matches.count) elements; identifiers must be unique")
        }
        return matches[0]
    }

    private func hitPoint(_ id: String) throws -> CGPoint {
        #if os(macOS)
        if try currentTree().nativeWeb?.binding.generatedIDs.contains(id) == true {
            throw StepFailure(message: "Native replay '\(id)' requires its original AX press")
        }
        #endif
        let element = try onlyElement(id)
        guard let point = element.hitPoint else {
            throw StepFailure(message: "'\(id)' is outside its window (frame \(element.frame)); nothing to tap")
        }
        guard element.enabled else {
            throw StepFailure(message: "'\(id)' is disabled")
        }
        return point
    }

    private func currentTree() throws -> HierarchyExport.Tree {
        if let tree { return tree }
        let tree = try HierarchyExport.tree(try running())
        self.tree = tree
        return tree
    }

    /// An absolute screen point, as a coordinate relative to the app's first
    /// window.
    private func coordinate(at point: CGPoint) throws -> XCUICoordinate {
        let tree = try currentTree()
        let origin = try running().windows.firstMatch.coordinate(withNormalizedOffset: .zero)
        return origin.withOffset(CGVector(dx: point.x - tree.window.minX, dy: point.y - tree.window.minY))
    }

    // MARK: Frames

    /// A sampler for a marked step that acts on the running app, started; nil
    /// otherwise. A launch starts a new app, a resize moves the window the
    /// sampler crops to, and a snapshot is still.
    private func startSampler(for step: DriveScript.Step) throws -> FrameSampler? {
        guard sampleFrames, step.transition != nil else { return nil }
        switch step.action {
        case .launch, .resize, .snapshot, .foregroundCycle:
            return nil
        default:
            let sampler = FrameSampler(window: try currentTree().window)
            sampler.start()
            return sampler
        }
    }

    private func attachFrames(_ sampler: FrameSampler, base: String) -> [FrameRecord] {
        var records: [FrameRecord] = []
        for (offset, frame) in sampler.frames.enumerated() {
            guard let png = FrameSampler.png(frame.image) else { continue }
            let name = String(format: "%@-frame-%03d.png", base, offset + 1)
            attach(png, name: name, type: "public.png")
            records.append(FrameRecord(file: name, time: frame.time))
        }
        return records
    }

    // MARK: Captures

    /// Attaches the hierarchy and a screenshot as `<base>-<phase>.json` and
    /// `.png`, and keeps the tree for the next step's hit points.
    private func capture(_ base: String, phase: String, foregroundOnly: Bool = false) throws -> Capture {
        let app = try running()
        if privateInput || foregroundOnly, app.state != .runningForeground {
            throw StepFailure(message: "drive capture unavailable")
        }
        let tree = try HierarchyExport.tree(app)
        if foregroundOnly, app.state != .runningForeground {
            throw StepFailure(message: "drive capture unavailable")
        }
        self.tree = tree
        #if os(macOS)
        _ = NSApplication.shared
        let screenshot = app.windows.firstMatch.screenshot()
        let origin = tree.window.origin
        #else
        // A foldable has two screens and the app may be on the other one
        // (unfolded: the inner display): capture the app there.
        // (unfolded: the inner display), where some captures come back
        // black: take the candidate with the most content.
        let candidates = XCUIScreen.screens.count > 1 && !privateInput && !foregroundOnly
            ? XCUIScreen.screens.map { $0.screenshot() } + [app.screenshot(), app.windows.firstMatch.screenshot()]
            : [(privateInput || foregroundOnly) ? app.screenshot() : XCUIScreen.main.screenshot()]
        print("drive: capture candidates \(candidates.map { $0.pngRepresentation.count })")
        let screenshot = candidates.max { $0.pngRepresentation.count < $1.pngRepresentation.count }!
        let origin = CGPoint.zero
        #endif
        let png = screenshot.pngRepresentation
        let width = HierarchyExport.pngPixelWidth(png).map(Double.init) ?? 0
        let scale = tree.window.width > 0 ? (width / Double(tree.window.width) * 100).rounded() / 100 : 0
        let json = try HierarchyExport.json(tree, platform: Self.platform, screenshotOrigin: origin, screenshotScale: scale)
        let name = phase == "before" || phase == "after" ? "\(base)-\(phase)" : base
        attach(json, name: "\(name).json", type: "public.json")
        attach(png, name: "\(name).png", type: "public.png")
        return Capture(phase: phase, hierarchy: "\(name).json", screenshot: "\(name).png")
    }

    private func attach(_ data: Data, name: String, type: String) {
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: type)
        attachment.name = name
        attachment.lifetime = .keepAlways
        test.add(attachment)
    }

    private func attachLog() {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        if let data = try? encoder.encode(log) {
            attach(data, name: "drive-log.json", type: "public.json")
        }
    }
}
