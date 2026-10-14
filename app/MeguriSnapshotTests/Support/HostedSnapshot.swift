import DesignSystem
import Foundation
import SnapshotTesting
import SwiftUI
import Testing
#if os(macOS)
import AppKit
#else
import UIKit
#endif

// App-hosted snapshots (R1973): the view renders in a real window of the
// host app, so glass and blur render, after an idle wait with animations off,
// and is compared with its baseline by swift-snapshot-testing's perceptual
// diff. Tests never write baselines: `tools/mg snapshots` reads the results
// this file writes and copies recorded images into app/Snapshots/ (the Mac
// test host is sandboxed and cannot write the checkout).

/// Comparison tolerances (R1973). `tools/scripts/image-diff` (the baseline
/// guard) uses the same values.
enum SnapshotTolerance {
    /// Every snapshot is drawn at 2x, whatever the device's own scale: the
    /// same pixels per point on every platform, and baselines a third the
    /// size of 3x ones.
    static let scale: CGFloat = 2
    /// Fraction of pixels that must match perceptually.
    static let precision: Float = 1
    /// Per-pixel perceptual match: CIE ΔE at most (1 - 0.98) × 100 = 2.
    static let perceptualPrecision: Float = 0.98
}

/// How a snapshot is laid out.
enum SnapshotLayout: String, Codable, Sendable {
    /// A component at the device class's width minus margins, as tall as it
    /// needs, on the primary background, with no safe area.
    case component
    /// A whole screen at the device class's size, in the safe area of the
    /// simulator's own window.
    case screen
}

/// One `tools/mg snapshots` test run, from the environment the job sets
/// (`TEST_RUNNER_MEGURI_SNAPSHOT_*`). Outside the job: verify mode, every
/// snapshot, the default settings group, output in the temporary directory.
struct SnapshotRun: Sendable {
    enum Mode: String, Sendable {
        /// Compare with the baselines; a failure or a missing baseline fails the test.
        case verify
        /// Compare, and hand failing and missing images to the job to record.
        case record
        /// List every snapshot (name, cell, group) without rendering.
        case plan
    }

    let mode: Mode
    /// Glob patterns over snapshot names (`subject/state/cell`); empty: all.
    let filters: [String]
    /// The simulator settings group this run has (`rt0-rm0-t50`); the Mac's
    /// is `mac`.
    let group: String
    let output: URL
    let baselines: URL
    /// `<model>-<os build>`: baselines are pinned per model and build (R1966).
    let key: String
    let inventory: URL?

    static let current = SnapshotRun(environment: ProcessInfo.processInfo.environment)

    /// The checkout this source file belongs to (the simulator and the Mac
    /// test host can read it).
    static let repositoryRoot: URL = {
        var url = URL(filePath: #filePath)
        for _ in 0..<4 { url.deleteLastPathComponent() }
        return url
    }()

    static var baselineRoot: URL { repositoryRoot.appending(path: "app/Snapshots", directoryHint: .isDirectory) }

    init(mode: Mode, filters: [String] = [], group: String, output: URL, baselines: URL, key: String, inventory: URL? = nil) {
        self.mode = mode
        self.filters = filters
        self.group = group
        self.output = output
        self.baselines = baselines
        self.key = key
        self.inventory = inventory
    }

    init(environment: [String: String]) {
        mode = environment["MEGURI_SNAPSHOT_MODE"].flatMap(Mode.init(rawValue:)) ?? .verify
        filters = (environment["MEGURI_SNAPSHOT_FILTER"] ?? "")
            .split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        #if os(macOS)
        group = "mac"
        #else
        group = environment["MEGURI_SNAPSHOT_GROUP"] ?? MatrixCell(device: .iphoneLarge).systemGroup
        #endif
        if let path = environment["MEGURI_SNAPSHOT_OUT"], !path.isEmpty {
            output = URL(filePath: path, directoryHint: .isDirectory)
        } else {
            output = FileManager.default.temporaryDirectory.appending(path: "meguri-snapshots", directoryHint: .isDirectory)
        }
        key = environment["MEGURI_SNAPSHOT_KEY"] ?? Self.defaultKey(environment)
        baselines = Self.baselineRoot.appending(path: key, directoryHint: .isDirectory)
        inventory = environment["MEGURI_INVENTORY_JSON"].map { URL(filePath: $0) }
    }

    /// `<model>-<OS build>` of this simulator or Mac.
    static func defaultKey(_ environment: [String: String]) -> String {
        #if os(macOS)
        return "\(sysctl("hw.model") ?? "Mac")-\(sysctl("kern.osversion") ?? "unknown")"
        #else
        let model = environment["SIMULATOR_MODEL_IDENTIFIER"] ?? "unknown"
        let build = environment["SIMULATOR_RUNTIME_BUILD_VERSION"] ?? "unknown"
        return "\(model)-\(build)"
        #endif
    }

    #if os(macOS)
    private static func sysctl(_ name: String) -> String? {
        var size = 0
        guard sysctlbyname(name, nil, &size, nil, 0) == 0, size > 0 else { return nil }
        var bytes = [UInt8](repeating: 0, count: size)
        guard sysctlbyname(name, &bytes, &size, nil, 0) == 0 else { return nil }
        return String(decoding: bytes.prefix { $0 != 0 }, as: UTF8.self)
    }
    #endif

    func selects(_ name: String) -> Bool {
        filters.isEmpty || filters.contains { fnmatch($0, name, 0) == 0 }
    }
}

/// Where the results of a run go: one JSON object per line in
/// `results.jsonl` (or `plan.jsonl`), images under `actual/` and `diff/`.
@MainActor
enum SnapshotLog {
    static func append(_ entry: [String: Any], to file: String, run: SnapshotRun = .current) {
        do {
            try FileManager.default.createDirectory(at: run.output, withIntermediateDirectories: true)
            let url = run.output.appending(path: file)
            var data = try JSONSerialization.data(withJSONObject: entry, options: [.sortedKeys])
            data.append(0x0A)
            if let handle = try? FileHandle(forWritingTo: url) {
                defer { try? handle.close() }
                try handle.seekToEnd()
                try handle.write(contentsOf: data)
            } else {
                try data.write(to: url)
            }
        } catch {
            Issue.record("could not write \(file) in \(run.output.path): \(error)")
        }
    }

    static func write(_ data: Data, as relativePath: String, run: SnapshotRun = .current) -> URL? {
        let url = run.output.appending(path: relativePath)
        do {
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try data.write(to: url)
            return url
        } catch {
            Issue.record("could not write \(url.path): \(error)")
            return nil
        }
    }
}

/// The outcome of one snapshot.
enum SnapshotStatus: String {
    case pass, fail, new, recorded, deferred
}

/// Renders `view` in `cell`, compares it with its baseline and reports it.
/// Cells of another settings group, deferred cells and names the run's
/// filter leaves out are skipped (deferred ones are listed as such).
@MainActor
@discardableResult
func assertSnapshot(
    subject: String,
    state: String,
    cell: MatrixCell,
    layout: SnapshotLayout,
    run: SnapshotRun = .current,
    keepActual: Bool = false,
    sourceLocation: SourceLocation = #_sourceLocation,
    @ViewBuilder _ view: () -> some View
) async -> SnapshotStatus? {
    let content = AnyView(view())
    return await assertImageSnapshot(
        subject: subject, state: state, cell: cell, layout: layout, run: run, keepActual: keepActual, sourceLocation: sourceLocation
    ) {
        try await HostedRenderer.render(content, cell: cell, layout: layout)
    }
}

/// One snapshot's bookkeeping before anything renders: the result entry, or
/// nil (with the status to return) when this run does not render it.
@MainActor
private func prepare(
    _ name: String, subject: String, state: String, cell: MatrixCell, layout: SnapshotLayout, run: SnapshotRun
) -> (entry: [String: Any]?, skipped: SnapshotStatus?) {
    guard run.selects(name), cell.device.platform == SnapshotPlatform.current else { return (nil, nil) }
    var entry: [String: Any] = [
        "name": name, "subject": subject, "state": state, "cell": cell.name, "layout": layout.rawValue,
        "group": cell.systemGroup, "platform": cell.device.platform.rawValue, "key": run.key,
        "axes": Dictionary(uniqueKeysWithValues: MatrixAxis.allCases.map { ($0.rawValue, cell.value(of: $0)) }),
    ]
    if let reason = cell.deferredReason {
        entry["deferred"] = reason
    }
    if run.mode == .plan {
        SnapshotLog.append(entry, to: "plan.jsonl", run: run)
        return (nil, nil)
    }
    if let reason = cell.deferredReason {
        // Listed once, by the run with the platform's default settings.
        guard run.group == MatrixCell(device: cell.device).systemGroup else { return (nil, nil) }
        entry["status"] = SnapshotStatus.deferred.rawValue
        entry["message"] = reason
        SnapshotLog.append(entry, to: "results.jsonl", run: run)
        return (nil, .deferred)
    }
    guard cell.systemGroup == run.group else { return (nil, nil) }
    return (entry, nil)
}

/// Records the outcome of a comparison and fails the test in verify mode.
@MainActor
private func report(
    _ entry: [String: Any], name: String, status: SnapshotStatus, message: String, run: SnapshotRun, sourceLocation: SourceLocation
) -> SnapshotStatus {
    var entry = entry
    entry["status"] = (run.mode == .record && status != .pass ? SnapshotStatus.recorded : status).rawValue
    entry["message"] = message
    SnapshotLog.append(entry, to: "results.jsonl", run: run)
    if run.mode == .verify, status != .pass {
        let hint = "record it with tools/mg snapshots --record '\(name)'"
        Issue.record(
            "\(name): \(message)\(entry["diff"].map { "; diff: \($0)" } ?? ""); \(hint)",
            sourceLocation: sourceLocation
        )
    }
    return run.mode == .record && status != .pass ? .recorded : status
}

private func repositoryPath(_ url: URL) -> String {
    String(url.path.dropFirst(SnapshotRun.repositoryRoot.path.count).drop { $0 == "/" })
}

/// The image snapshot pipeline: `render` draws the image (a SwiftUI view,
/// a web view), which is compared with `<name>.png` at the tolerance.
@MainActor
@discardableResult
func assertImageSnapshot(
    subject: String,
    state: String,
    cell: MatrixCell,
    layout: SnapshotLayout,
    run: SnapshotRun = .current,
    keepActual: Bool = false,
    sourceLocation: SourceLocation = #_sourceLocation,
    render: () async throws -> PlatformImage
) async -> SnapshotStatus? {
    let name = "\(subject)/\(state)/\(cell.name)"
    let (prepared, skipped) = prepare(name, subject: subject, state: state, cell: cell, layout: layout, run: run)
    guard var entry = prepared else { return skipped }
    let baseline = run.baselines.appending(path: "\(name).png")
    entry["baseline"] = repositoryPath(baseline)
    let image: PlatformImage
    do {
        image = try await render()
    } catch {
        entry["status"] = SnapshotStatus.fail.rawValue
        entry["message"] = "\(error)"
        SnapshotLog.append(entry, to: "results.jsonl", run: run)
        Issue.record("\(name): \(error)", sourceLocation: sourceLocation)
        return .fail
    }
    let diffing = PlatformImage.snapshotDiffing
    let status: SnapshotStatus
    var message = ""
    if let reference = try? Data(contentsOf: baseline) {
        if let (failure, attachments) = diffing.diffV2(diffing.fromData(reference), image) {
            status = .fail
            message = failure
            for case let .data(data, name: attachmentName) in attachments where attachmentName == "difference.png" {
                entry["diff"] = SnapshotLog.write(data, as: "diff/\(name).png", run: run)?.path
            }
        } else {
            status = .pass
        }
    } else {
        status = .new
        message = "no baseline at \(entry["baseline"] ?? "")"
    }
    if status != .pass || keepActual {
        entry["actual"] = SnapshotLog.write(diffing.toData(image), as: "actual/\(name).png", run: run)?.path
    }
    return report(entry, name: name, status: status, message: message, run: run, sourceLocation: sourceLocation)
}

/// A text snapshot (R1975: a card face's DOM): `text` must equal
/// `<name>.<fileExtension>` exactly; a difference names its first line.
@MainActor
@discardableResult
func assertTextSnapshot(
    _ text: String,
    subject: String,
    state: String,
    cell: MatrixCell,
    fileExtension: String = "html",
    run: SnapshotRun = .current,
    sourceLocation: SourceLocation = #_sourceLocation
) -> SnapshotStatus? {
    let name = "\(subject)/\(state)/\(cell.name)"
    let (prepared, skipped) = prepare(name, subject: subject, state: state, cell: cell, layout: .screen, run: run)
    guard var entry = prepared else { return skipped }
    entry["kind"] = "text"
    let baseline = run.baselines.appending(path: "\(name).\(fileExtension)")
    entry["baseline"] = repositoryPath(baseline)
    let status: SnapshotStatus
    var message = ""
    if let reference = try? String(contentsOf: baseline, encoding: .utf8) {
        if reference == text {
            status = .pass
        } else {
            status = .fail
            let old = reference.split(separator: "\n", omittingEmptySubsequences: false)
            let new = text.split(separator: "\n", omittingEmptySubsequences: false)
            let line = zip(old, new).enumerated().first { $0.element.0 != $0.element.1 }?.offset ?? min(old.count, new.count)
            message = "text differs from line \(line + 1): expected \(line < old.count ? "\"\(old[line])\"" : "the end"), got \(line < new.count ? "\"\(new[line])\"" : "the end")"
        }
    } else {
        status = .new
        message = "no baseline at \(entry["baseline"] ?? "")"
    }
    if status != .pass {
        entry["actual"] = SnapshotLog.write(Data(text.utf8), as: "actual/\(name).\(fileExtension)", run: run)?.path
    }
    return report(entry, name: name, status: status, message: message, run: run, sourceLocation: sourceLocation)
}

// MARK: - Rendering

#if os(macOS)
typealias PlatformImage = NSImage
extension NSImage {
    static var snapshotDiffing: Diffing<NSImage> {
        .image(precision: SnapshotTolerance.precision, perceptualPrecision: SnapshotTolerance.perceptualPrecision)
    }
}
#else
typealias PlatformImage = UIImage
extension UIImage {
    static var snapshotDiffing: Diffing<UIImage> {
        .image(precision: SnapshotTolerance.precision, perceptualPrecision: SnapshotTolerance.perceptualPrecision, scale: SnapshotTolerance.scale)
    }
}
#endif

struct RenderError: Error, CustomStringConvertible {
    let description: String
}

/// The cell's appearance, text size, contrast and accessibility settings as
/// SwiftUI environment values. On iOS the traits carry them too (system
/// chrome and asset colours read traits); Reduce Transparency and Reduce
/// Motion come from the simulator's settings there.
struct CellEnvironment: ViewModifier {
    let cell: MatrixCell

    func body(content: Content) -> some View {
        content
            // Fixed locale and zone: numbers and dates read the same on every
            // machine (R1963's fixtures are en_US).
            .environment(\.locale, Locale(identifier: "en_US"))
            .environment(\.timeZone, TimeZone(identifier: "UTC")!)
            .environment(\.colorScheme, cell.appearance == .dark ? .dark : .light)
            .environment(\.dynamicTypeSize, cell.textSize.dynamicTypeSize)
            .environment(\._colorSchemeContrast, cell.increaseContrast ? .increased : .standard)
            #if os(macOS)
            .environment(\._accessibilityReduceTransparency, cell.reduceTransparency)
            .environment(\._accessibilityReduceMotion, cell.reduceMotion)
            // Off-screen Mac renders draw no Liquid Glass (no backdrop):
            // chrome is drawn flat so its controls keep their labels.
            .environment(\.chromeGlass, .flat)
            #endif
            .transaction { transaction in
                transaction.disablesAnimations = true
                transaction.animation = nil
            }
    }
}

extension TextSize {
    var dynamicTypeSize: DynamicTypeSize {
        switch self {
        case .standard: .large
        case .xxxLarge: .xxxLarge
        case .ax4: .accessibility4
        case .ax5: .accessibility5
        }
    }

    #if !os(macOS)
    var contentSizeCategory: UIContentSizeCategory {
        switch self {
        case .standard: .large
        case .xxxLarge: .extraExtraExtraLarge
        case .ax4: .accessibilityExtraExtraLarge
        case .ax5: .accessibilityExtraExtraExtraLarge
        }
    }
    #endif
}

@MainActor
enum HostedRenderer {
    /// Frames rendered while waiting for the view to settle.
    static let maximumFrames = 20
    static let frameInterval: Duration = .milliseconds(50)

    /// The view's image once two consecutive frames match (R1973: never a
    /// mid-animation frame); an error when it never settles.
    static func render(_ view: AnyView, cell: MatrixCell, layout: SnapshotLayout) async throws -> PlatformImage {
        let host = try Host(view, cell: cell, layout: layout)
        defer { host.tearDown() }
        let diffing = PlatformImage.snapshotDiffing
        var previous: PlatformImage?
        for _ in 0..<maximumFrames {
            try await Task.sleep(for: frameInterval)
            let frame = try host.frame()
            if let previous, diffing.diffV2(previous, frame) == nil {
                return frame
            }
            previous = frame
        }
        throw RenderError(description: "the view did not settle within \(maximumFrames) frames (an animation or a timer keeps changing it)")
    }

    /// What the window hosts: a component padded on the primary background
    /// filling the window; a screen as it is (its safe area comes from the
    /// window, see `Host` on iOS).
    static func content(_ view: AnyView, cell: MatrixCell, layout: SnapshotLayout) -> AnyView {
        switch layout {
        case .component:
            return AnyView(
                view.padding(Spacing.medium)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(.token(.backgroundPrimary))
                    .modifier(CellEnvironment(cell: cell))
            )
        case .screen:
            return AnyView(view.modifier(CellEnvironment(cell: cell)))
        }
    }

    /// The width components are drawn at: the device class's, at most a
    /// phone's.
    static let maximumComponentWidth: CGFloat = 480
    /// The tallest component: a view that grows to any height it is offered
    /// is measured at its ideal height, and one with no ideal height fails.
    static let maximumComponentHeight: CGFloat = 4000

    /// The component's padded size at its cell's width, measured without
    /// the background that fills the window.
    static func componentSize(of view: AnyView, cell: MatrixCell) throws -> CGSize {
        let width = min(cell.device.size.width, maximumComponentWidth)
        let measured = AnyView(view.padding(Spacing.medium).fixedSize(horizontal: false, vertical: true).modifier(CellEnvironment(cell: cell)))
        #if os(macOS)
        let controller = NSHostingController(rootView: measured)
        #else
        let controller = UIHostingController(rootView: measured)
        controller.traitOverrides.preferredContentSizeCategory = cell.textSize.contentSizeCategory
        controller.traitOverrides.horizontalSizeClass = cell.device.horizontalSizeClassIsRegular ? .regular : .compact
        #endif
        let fitting = controller.sizeThatFits(in: CGSize(width: width, height: .infinity))
        guard fitting.height.isFinite, fitting.height > 0, fitting.height <= maximumComponentHeight else {
            throw RenderError(description: "the component has no ideal height (it measured \(fitting.height) pt); give it one, or snapshot it as a screen")
        }
        return CGSize(width: width, height: ceil(fitting.height))
    }

    #if os(macOS)
    /// An off-screen borderless window, never ordered in: Mac snapshots
    /// never appear on the user's screen. Drawn at `SnapshotTolerance.scale`
    /// whatever the display, so baselines do not depend on the monitor.
    @MainActor
    final class Host {
        private let window: NSWindow
        private let hostingView: NSView

        init(_ view: AnyView, cell: MatrixCell, layout: SnapshotLayout) throws {
            var size = cell.device.size
            if layout == .component {
                size = try HostedRenderer.componentSize(of: view, cell: cell)
            }
            let controller = NSHostingController(rootView: HostedRenderer.content(view, cell: cell, layout: layout))
            guard size.width > 0, size.height > 0 else { throw RenderError(description: "the view has no size") }
            window = NSWindow(
                contentRect: CGRect(origin: CGPoint(x: -30_000, y: -30_000), size: size),
                styleMask: [.borderless], backing: .buffered, defer: false
            )
            window.isReleasedWhenClosed = false
            window.appearance = NSAppearance(named: cell.appearance == .dark ? .darkAqua : .aqua)
            window.animationBehavior = .none
            window.contentViewController = controller
            window.setContentSize(size)
            hostingView = controller.view
            hostingView.frame = CGRect(origin: .zero, size: size)
            hostingView.layoutSubtreeIfNeeded()
        }

        func frame() throws -> NSImage {
            hostingView.layoutSubtreeIfNeeded()
            let bounds = hostingView.bounds
            guard let rep = NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: Int(bounds.width * SnapshotTolerance.scale), pixelsHigh: Int(bounds.height * SnapshotTolerance.scale),
                bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
            ) else { throw RenderError(description: "could not allocate a bitmap") }
            rep.size = bounds.size
            hostingView.cacheDisplay(in: bounds, to: rep)
            let image = NSImage(size: bounds.size)
            image.addRepresentation(rep)
            return image
        }

        func tearDown() {
            window.contentViewController = nil
            window.close()
        }
    }
    #else
    /// A window of its own on the app's scene, made key and visible, so the
    /// render server draws glass and blur (drawHierarchy). The iOS 27
    /// safe-area workaround (issue #1125 shifted renders by the window's
    /// insets): a component takes no safe area at all; a screen takes the
    /// window's own, which is the simulator screen's where the window
    /// overlaps it. Never pad a screen with other insets: UIKit (scroll edge
    /// effects) keeps reading the window's, and the two would disagree.
    @MainActor
    final class Host {
        private let window: UIWindow
        private let previousKeyWindow: UIWindow?

        init(_ view: AnyView, cell: MatrixCell, layout: SnapshotLayout) throws {
            let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            guard let scene = scenes.first(where: { $0.activationState == .foregroundActive }) ?? scenes.first
            else { throw RenderError(description: "the host app has no window scene") }
            previousKeyWindow = scene.keyWindow
            UIView.setAnimationsEnabled(false)
            let controller = UIHostingController(rootView: HostedRenderer.content(view, cell: cell, layout: layout))
            if layout == .component {
                controller.safeAreaRegions = []
            }
            controller.view.backgroundColor = .clear
            controller.traitOverrides.userInterfaceStyle = cell.appearance == .dark ? .dark : .light
            controller.traitOverrides.preferredContentSizeCategory = cell.textSize.contentSizeCategory
            controller.traitOverrides.accessibilityContrast = cell.increaseContrast ? .high : .normal
            controller.traitOverrides.horizontalSizeClass = cell.device.horizontalSizeClassIsRegular ? .regular : .compact
            controller.traitOverrides.verticalSizeClass = .regular
            var size = cell.device.size
            if layout == .component {
                size = try HostedRenderer.componentSize(of: view, cell: cell)
            }
            guard size.width > 0, size.height > 0 else { throw RenderError(description: "the view has no size") }
            window = UIWindow(windowScene: scene)
            window.windowLevel = .alert + 1
            window.overrideUserInterfaceStyle = cell.appearance == .dark ? .dark : .light
            window.backgroundColor = UIColor(ColorToken.backgroundPrimary.color)
            window.frame = CGRect(origin: .zero, size: size)
            // Core Animation stands still in this window: a spinner or any
            // other running layer animation renders at its first frame, the
            // same in every run.
            window.layer.speed = 0
            window.rootViewController = controller
            window.makeKeyAndVisible()
            controller.view.frame = window.bounds
            controller.view.layoutIfNeeded()
        }

        func frame() throws -> UIImage {
            window.layoutIfNeeded()
            let format = UIGraphicsImageRendererFormat(for: window.traitCollection)
            format.scale = SnapshotTolerance.scale
            // 8-bit sRGB: half the bytes of the extended-range default, and
            // the colour space the comparison works in.
            format.preferredRange = .standard
            // Every snapshot has a background: no alpha channel to store.
            format.opaque = true
            let renderer = UIGraphicsImageRenderer(bounds: window.bounds, format: format)
            var drawn = false
            let image = renderer.image { _ in
                drawn = window.drawHierarchy(in: window.bounds, afterScreenUpdates: true)
            }
            guard drawn else { throw RenderError(description: "drawHierarchy did not draw the window") }
            return image
        }

        func tearDown() {
            window.isHidden = true
            window.rootViewController = nil
            previousKeyWindow?.makeKey()
            UIView.setAnimationsEnabled(true)
        }
    }
    #endif
}
