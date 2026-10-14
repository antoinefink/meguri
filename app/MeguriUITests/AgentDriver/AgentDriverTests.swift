import Foundation
import XCTest

/// Runs one agent drive script (U8, R1961, R1965). `tools/mg drive` passes the
/// script's path as `MEGURI_DRIVE_SCRIPT` through the test-runner environment
/// (`TEST_RUNNER_MEGURI_DRIVE_SCRIPT`); without it the test skips, so the UI
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
        guard let path = ProcessInfo.processInfo.environment["MEGURI_DRIVE_SCRIPT"], !path.isEmpty else {
            throw XCTSkip("no drive script: tools/mg drive passes it as MEGURI_DRIVE_SCRIPT")
        }
        // The driver stops at the first failed step and reports it in the
        // drive log, which `tools/mg drive` reads; it records no test
        // failure of its own. A failure XCTest records still fails the step,
        // and must not stop the test before the log is attached.
        continueAfterFailure = true
        let driver = AgentDriver(build: try BuildUnderTest.current(), test: self)
        driver.run(scriptAt: URL(filePath: path))
    }
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
    private unowned let test: XCTestCase
    /// Fixed-rate frames around marked steps (`FrameSampler`).
    private let sampleFrames = ProcessInfo.processInfo.environment["MEGURI_DRIVE_FRAMES"] == "1"
    private var app: XCUIApplication?
    private var lastLaunch: DriveScript.Launch?
    private var tree: HierarchyExport.Tree?
    private var log: Log

    static var platform: String {
        #if os(macOS)
        "mac"
        #else
        "ios"
        #endif
    }

    init(build: BuildUnderTest, test: XCTestCase) {
        self.build = build
        self.test = test
        log = Log(script: "", path: "", platform: Self.platform)
    }

    func run(scriptAt url: URL) {
        log.path = url.path(percentEncoded: false)
        defer { attachLog() }
        let script: DriveScript
        do {
            script = try DriveScript(contentsOf: url)
        } catch {
            log.error = error.message
            return
        }
        log.script = script.name
        for (offset, step) in script.steps.enumerated() {
            guard perform(step, index: offset + 1) else { return }
        }
    }

    // MARK: Steps

    private func perform(_ step: DriveScript.Step, index: Int) -> Bool {
        let kind = step.action.kind
        var record = StepRecord(index: index, kind: kind, transition: step.transition, started: Date().timeIntervalSince1970)
        let failuresBefore = test.testRun?.totalFailureCount ?? 0
        let base = String(format: "drive-%02d-%@", index, kind)
        var failed = false
        var sampler: FrameSampler?
        do {
            switch step.action {
            case .launch, .snapshot:
                break
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
        if app != nil {
            do {
                if case let .snapshot(name) = step.action {
                    record.captures.append(try capture("\(base)-\(name)", phase: name))
                } else {
                    record.captures.append(try capture(base, phase: "after"))
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
        log.steps.append(record)
        if failed {
            // Not XCTFail: on a test failure XCTest attaches the app's whole
            // UI hierarchy, which on the Mac lists the user's recent files.
            print("drive: step \(index) (\(kind)) failed: \(record.message ?? "")")
            return false
        }
        return true
    }

    /// Mac: a click, key press or drag on a window that is not key only
    /// activates it, and another app is usually in front. The app comes to
    /// the front before the step's 'before' capture, so that capture shows
    /// the window the action reaches.
    private func prepareInput(_ action: DriveScript.Action) throws {
        #if os(macOS)
        switch action {
        case .tap, .type, .swipe, .scroll:
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
            try coordinate(at: try hitPoint(id)).tap()
        case let .type(id, text):
            try coordinate(at: try hitPoint(id)).tap()
            try running().typeText(text)
        case let .swipe(id, direction):
            try drag(id, direction, scroll: false)
        case let .scroll(id, direction):
            try drag(id, direction, scroll: true)
        case let .wait(id, timeout, gone):
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
        }
    }

    private func launch(_ options: DriveScript.Launch) throws {
        #if os(macOS)
        // A build without the .dev id would replace the daily-driver app.
        let id = try build.bundleIdentifier
        guard id.hasSuffix(".dev") else {
            throw StepFailure(message: "never launch \(id) on the Mac: it would replace the daily-driver app")
        }
        #endif
        app?.terminate()
        let app = XCUIApplication()
        app.launchArguments = arguments(options)
        app.launch()
        guard app.wait(for: .runningForeground, timeout: 30) else {
            throw StepFailure(message: "the app is not running in the foreground 30 s after launch")
        }
        self.app = app
        lastLaunch = options
        test.addTeardownBlock { @MainActor in app.terminate() }
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

    /// Mac: relaunches with the last launch options at the new window size
    /// (the window size is a launch option, R1962). Simulators have no
    /// resizable iPhone window in this toolchain.
    private func resize(_ size: String) throws {
        #if os(macOS)
        guard var options = lastLaunch else {
            throw StepFailure(message: "resize needs a launch step first")
        }
        options.windowSize = size
        try launch(options)
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
        // A swipe moves the finger in its direction; a scroll moves the
        // content in its direction, so the finger goes the other way.
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
        start.press(forDuration: 0.05, thenDragTo: end)
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
    /// otherwise. Launch and resize start a new app, and a snapshot is still.
    private func startSampler(for step: DriveScript.Step) throws -> FrameSampler? {
        guard sampleFrames, step.transition != nil else { return nil }
        switch step.action {
        case .launch, .resize, .snapshot:
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
    private func capture(_ base: String, phase: String) throws -> Capture {
        let app = try running()
        let tree = try HierarchyExport.tree(app)
        self.tree = tree
        #if os(macOS)
        let screenshot = app.windows.firstMatch.screenshot()
        let origin = tree.window.origin
        #else
        let screenshot = XCUIScreen.main.screenshot()
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
