#if MEGURI_TEST_HOOKS && os(macOS)
@_spi(MeguriTestHooks) import AppModel
import AppKit
import CoreMedia
import Foundation
import Observation
import QuartzCore
import ScreenCaptureKit
import os

/// `NSApplication.postEvent(_:atStart:)`, which AppKit documents as callable
/// from any thread; the SDK's main-actor annotation covers the whole class.
@objc nonisolated fileprivate protocol EventPosting {
    func postEvent(_ event: NSEvent, atStart flag: Bool)
}

extension TestHooks {
    /// Times a click on a main-window sidebar row, from its mouse-down to the
    /// window frame that first shows the row selected and to the first frame of
    /// the destination (SidebarSelectionTimingTests). The test names points on
    /// screen; the app posts the mouse events to itself from a background
    /// thread (`NSApplication.postEvent`), so a click can wait in the queue
    /// behind a busy main thread exactly as a person's does, and the cursor
    /// never moves. ScreenCaptureKit captures the app's own window (no Screen
    /// Recording permission): each frame's display time and how much of the
    /// clicked row and of the detail column differ from the last frame shown
    /// before the first mouse-down. A run-loop observer records main-thread
    /// busy spans, and navigation changes are timed as they happen.
    ///
    /// `<bundle id>.testhooks.sidebarTimingRun`, object
    /// `x,y,holdMs,gapMs,label[|...]` (screen points, top-left origin, as
    /// XCUITest reports frames; `settle=<ms>` may lead), is answered with
    /// `.sidebarTimingResult` and a JSON object.
    final class SidebarSelectionTiming: NSObject {
        static let shared = SidebarSelectionTiming()
        private static let signposter = OSSignposter(subsystem: "com.antoinefink.meguri", category: "SidebarTiming")

        private weak var navigation: AppNavigation?
        private var listening = false
        private var running = false
        private let store = FrameStore()
        private var capture: TimingCapture?
        private var observer: CFRunLoopObserver?
        private var busyStart: CFTimeInterval = 0
        private var busy: [(start: CFTimeInterval, duration: CFTimeInterval)] = []
        private var changes: [(time: CFTimeInterval, kind: String, value: String)] = []
        private var deliveries: [(time: CFTimeInterval, type: String, stamp: TimeInterval)] = []
        private var monitor: Any?
        private var postedLate: TimeInterval = 0
        private var detailRect = CGRect.zero
        private var sampler: MainThreadSampler?
        /// Each step's sidebar list and row, when that row was first selected in
        /// it after a run-loop pass (after Core Animation's commit), and when it
        /// was first seen unselected again after that.
        private var watched: [(table: NSTableView, row: Int, down: TimeInterval, selected: TimeInterval?, lost: TimeInterval?)] = []
        private var commitObserver: CFRunLoopObserver?

        static func name(_ suffix: String) -> String { "\(Bundle.main.bundleIdentifier ?? "").testhooks.\(suffix)" }

        func listen(navigation: AppNavigation) {
            self.navigation = navigation
            guard !listening else { return }
            listening = true
            sampler = MainThreadSampler()
            installObservers()
            DistributedNotificationCenter.default().addObserver(
                self, selector: #selector(received(_:)), name: Notification.Name(Self.name("sidebarTimingRun")), object: nil,
                suspensionBehavior: .deliverImmediately)
            // Answered on the main thread: the test waits for a responsive app.
            DistributedNotificationCenter.default().addObserver(
                self, selector: #selector(ping), name: Notification.Name(Self.name("sidebarTimingPing")), object: nil,
                suspensionBehavior: .deliverImmediately)
        }

        /// Answers with the main thread's spans of half a second or more so far,
        /// `[seconds after the probe started, duration]`.
        @objc private func ping() {
            installObservers()
            let long = busy.filter { $0.duration >= 0.5 }.suffix(20).map { "[\(Int(($0.start - began) * 10) / 10),\(Int($0.duration * 1000))ms]" }
            DistributedNotificationCenter.default().postNotificationName(
                Notification.Name(Self.name("sidebarTimingPong")), object: long.joined(separator: " "), userInfo: nil, deliverImmediately: true)
        }
        private let began = CACurrentMediaTime()

        @objc private func received(_ notification: Notification) {
            guard !running, let script = notification.object as? String else { return }
            running = true
            Task { @MainActor in
                let result = await run(script)
                running = false
                DistributedNotificationCenter.default().postNotificationName(
                    Notification.Name(Self.name("sidebarTimingResult")), object: result, userInfo: nil, deliverImmediately: true)
            }
        }

        // MARK: A run

        struct Step {
            var screenPoint: CGPoint
            var hold: Double
            var gap: Double
            var label: String
            var windowPoint = CGPoint.zero
            var rowRect = CGRect.zero
            var down: TimeInterval = 0
            var up: TimeInterval = 0
            weak var table: NSTableView?
            var row = -1
        }

        private func run(_ script: String) async -> String {
            var settle = 2.5
            var steps: [Step] = []
            // Finite, non-negative and bounded: a run that never ends answers nothing.
            func valid(_ value: Double?, max: Double) -> Double? {
                guard let value, value.isFinite, value >= 0, value <= max else { return nil }
                return value
            }
            for part in script.split(separator: "|") {
                if part.hasPrefix("settle=") {
                    guard let value = valid(Double(part.dropFirst(7)), max: 10_000) else {
                        return Self.json(["error": "bad settle '\(part)'"])
                    }
                    settle = value / 1000
                    continue
                }
                let fields = part.split(separator: ",", omittingEmptySubsequences: false).map(String.init)
                guard fields.count == 5, let x = Double(fields[0]), let y = Double(fields[1]), x.isFinite, y.isFinite,
                      let hold = valid(Double(fields[2]), max: 2_000), let gap = valid(Double(fields[3]), max: 2_000) else {
                    return Self.json(["error": "bad step '\(part)'"])
                }
                steps.append(Step(screenPoint: CGPoint(x: x, y: y), hold: hold / 1000, gap: gap / 1000, label: fields[4]))
            }
            guard !steps.isEmpty else { return Self.json(["error": "no steps"]) }
            guard steps.count <= 10 else { return Self.json(["error": "more than 10 steps"]) }
            guard let window = NSApp.windows.first(where: { $0.isVisible && $0.canBecomeMain && !($0 is NSPanel) }),
                  let content = window.contentView else { return Self.json(["error": "no main window"]) }
            let screenHeight = NSScreen.screens.first?.frame.maxY ?? 0
            var detail = CGRect.zero
            for index in steps.indices {
                let global = CGPoint(x: steps[index].screenPoint.x, y: screenHeight - steps[index].screenPoint.y)
                let point = window.convertPoint(fromScreen: global)
                steps[index].windowPoint = point
                guard let table = Self.table(at: point, in: content) else {
                    return Self.json(["error": "step \(steps[index].label): no sidebar table at \(point) in the window"])
                }
                let row = table.row(at: table.convert(point, from: nil))
                guard row >= 0 else { return Self.json(["error": "step \(steps[index].label): no row at \(point)"]) }
                steps[index].table = table
                steps[index].row = row
                steps[index].rowRect = Self.topLeft(table.convert(table.rect(ofRow: row), to: nil), in: window).insetBy(dx: 4, dy: 3)
                if detail == .zero {
                    // Everything right of the sidebar's list: the detail column and its toolbar.
                    let list = (table.enclosingScrollView ?? table).convert((table.enclosingScrollView ?? table).bounds, to: nil)
                    let width = window.frame.width
                    detail = CGRect(x: list.maxX + 8, y: 6, width: max(0, width - list.maxX - 14), height: window.frame.height - 12)
                }
            }
            guard detail != .zero else { return Self.json(["error": "no detail column"]) }
            do {
                try await startCapture(window)
            } catch {
                return Self.json(["error": "capture: \(error)"])
            }
            installObservers()
            busy.removeAll()
            changes.removeAll()
            deliveries.removeAll()
            if let navigation {
                // The routes before the first press: only later changes are commits.
                changes.append((CACurrentMediaTime(), "selection", Self.describe(navigation.sidebarSelection)))
                changes.append((CACurrentMediaTime(), "displayed", Self.describe(navigation.displayedSidebarSelection)))
            }
            detailRect = detail
            // A frame shown before the first press is the baseline.
            try? await Task.sleep(for: .milliseconds(200))
            switch store.arm(rows: steps.map(\.rowRect), detail: detail) {
            case .armed: break
            case .noFrame: return Self.json(["error": "no captured frame to compare with"])
            case .outside: return Self.json(["error": "a row or the detail column is outside the captured frame"])
            }
            // Every exit from here disarms: a failed run must not keep recording.
            var frames: [FrameStore.Record] = []
            var disarmed = false
            defer { if !disarmed { _ = store.disarm() } }
            let start = CACurrentMediaTime()
            Self.signposter.emitEvent("run", "\(steps.map(\.label).joined(separator: ">"))")
            // Each event carries the time it is posted at, as a person's press does.
            var time = start + 0.05
            var events: [NSEvent] = []
            var times: [TimeInterval] = []
            for (index, step) in steps.enumerated() {
                steps[index].down = time
                steps[index].up = time + step.hold
                for (type, stamp) in [(NSEvent.EventType.leftMouseDown, time), (.leftMouseUp, time + step.hold)] {
                    guard let event = NSEvent.mouseEvent(with: type, location: step.windowPoint, modifierFlags: [], timestamp: stamp,
                        windowNumber: window.windowNumber, context: nil, eventNumber: index + 1, clickCount: 1,
                        pressure: type == .leftMouseDown ? 1 : 0) else { return Self.json(["error": "no event"]) }
                    events.append(event)
                    times.append(stamp)
                }
                time += step.hold + step.gap
            }
            watched = steps.compactMap { step in step.table.map { (table: $0, row: step.row, down: step.down, selected: TimeInterval?.none, lost: TimeInterval?.none) } }
            sampler?.start()
            let late = await Self.post(ScheduledEvents(
                events: events, times: times, application: unsafeBitCast(NSApplication.shared, to: (any EventPosting).self)))
            // Times count from each event's timestamp: a late post only adds to them.
            guard late < 0.02 else { return Self.json(["error": "events posted up to \(late * 1000) ms late"]) }
            postedLate = late
            try? await Task.sleep(for: .seconds(settle))
            sampler?.stop()
            frames = store.disarm()
            disarmed = true
            if capture?.stopped == true {
                // The stream failed during the run: its frames prove nothing, and the
                // next run starts a stream of its own.
                capture = nil
                store.reset()
                watched = []
                return Self.json(["error": "the window capture stopped during the run"])
            }
            let end = CACurrentMediaTime()
            let result = report(steps: steps, frames: frames, start: start, end: end)
            watched = []
            return result
        }

        /// Events made on the main thread, posted at their timestamps from
        /// another (`postEvent(_:atStart:)` may be called from any thread).
        fileprivate nonisolated struct ScheduledEvents: @unchecked Sendable {
            let events: [NSEvent]
            let times: [TimeInterval]
            let application: any EventPosting
        }

        /// Posts each event at its own timestamp, whatever the main thread is
        /// doing; returns how late the latest one was posted.
        nonisolated fileprivate static func post(_ scheduled: ScheduledEvents) async -> TimeInterval {
            await withCheckedContinuation { continuation in
                let thread = Thread {
                    var late: TimeInterval = 0
                    for (event, time) in zip(scheduled.events, scheduled.times) {
                        let wait = time - CACurrentMediaTime()
                        if wait > 0 { Thread.sleep(forTimeInterval: wait) }
                        scheduled.application.postEvent(event, atStart: false)
                        // Once queued: the event is as late as its enqueue.
                        late = max(late, CACurrentMediaTime() - time)
                    }
                    continuation.resume(returning: late)
                }
                // As the window server's input: ahead of the app's own work.
                thread.qualityOfService = .userInteractive
                thread.start()
            }
        }

        private func report(steps: [Step], frames all: [FrameStore.Record], start: CFTimeInterval, end: CFTimeInterval) -> String {
            func ms(_ value: Double) -> Double { (value * 10_000).rounded() / 10 }
            // Only frames with a display time time anything: a callback's arrival
            // does not say when its frame was shown.
            let frames = all.filter(\.displayed)
            let commits = changes.filter { $0.kind == "selection" }
            let shownRoutes = changes.filter { $0.kind == "displayed" }
            /// When the app received each step's mouse-down (its timestamp until then).
            let received = steps.map { step in
                deliveries.first { $0.type == "down" && abs($0.stamp - step.down) < 0.0005 }?.time ?? step.down
            }
            var rows: [[String: Any]] = []
            for (index, step) in steps.enumerated() {
                let shown = frames.first { $0.time >= step.down && $0.rows[index] >= FrameStore.rowThreshold }
                let nextDown = index + 1 < steps.count ? steps[index + 1].down : end
                let nextReceived = index + 1 < steps.count ? received[index + 1] : .infinity
                // The route this press asked for: the row the sidebar showed once the app
                // had its mouse-down, before the next press arrived.
                let intended = shownRoutes.first { $0.time >= received[index] && $0.time < nextReceived }?.value
                // Its own commit, never a later press's; none when a later press superseded it.
                let commit = intended.flatMap { route in commits.first { $0.time >= step.down && $0.value == route } }
                // The outgoing page before the route changes: any frame that changes it is a flash.
                let before = frames.filter { $0.time >= step.down && $0.time < (commit?.time ?? nextDown) }
                let content = commit.flatMap { commit in frames.first { $0.time > commit.time && $0.detail >= FrameStore.contentThreshold } }
                var entry: [String: Any] = [
                    "label": step.label,
                    "holdMs": ms(step.up - step.down),
                    "rowMs": shown.map { ms($0.time - step.down) } ?? NSNull(),
                    "rowMax": (frames.map { $0.rows[index] }.max() ?? 0),
                    "detailBeforeCommitMax": before.map(\.delta).max() ?? 0,
                    "detailBeforeCommitFrames": before.filter { $0.delta >= FrameStore.flashThreshold }.count,
                    "commitMs": commit.map { ms($0.time - step.down) } ?? NSNull(),
                    "commit": commit?.value ?? NSNull(),
                    "route": intended ?? NSNull(),
                    "superseded": intended != nil && commit == nil,
                    "contentMs": content.map { ms($0.time - step.down) } ?? NSNull(),
                    "detailMax": frames.filter { $0.time >= step.down && $0.time < nextDown }.map(\.detail).max() ?? 0,
                    "rowRect": "\(step.rowRect)",
                    "missingSamples": all.filter { $0.time >= step.down && $0.time < nextDown && ($0.detail < 0 || $0.rows[index] < 0) }.count,
                ]
                if let watch = watched.first(where: { $0.down == step.down }) {
                    if let selected = watch.selected { entry["rowStateMs"] = ms(selected - step.down) }
                    if let lost = watch.lost { entry["rowLostMs"] = ms(lost - step.down) }
                }
                // Where the main thread spent a busy press-to-next-press stretch.
                if busy.contains(where: { $0.start + $0.duration >= step.down && $0.start < nextDown && $0.duration >= 0.1 }),
                   let sampler {
                    entry["profile"] = sampler.profile(from: step.down, to: nextDown)
                }
                // The press's own work, before its route changes.
                if let commit, commit.time - step.down >= 0.06, let sampler {
                    entry["pressProfile"] = sampler.profile(from: step.down, to: commit.time)
                }
                if let delivered = deliveries.first(where: { $0.type == "down" && abs($0.stamp - step.down) < 0.0005 }) {
                    entry["downDeliveredMs"] = ms(delivered.time - step.down)
                }
                if let delivered = deliveries.first(where: { $0.type == "up" && abs($0.stamp - step.up) < 0.0005 }) {
                    entry["upDeliveredMs"] = ms(delivered.time - step.down)
                }
                let last = frames.last { $0.time > step.down && $0.time < nextDown + 0.001 && $0.delta >= FrameStore.flashThreshold }
                entry["lastChangeMs"] = last.map { ms($0.time - step.down) } ?? NSNull()
                // Main-thread spans of 8 ms or more from this press to the next.
                entry["busy"] = busy.filter { $0.start + $0.duration >= step.down && $0.start < nextDown && $0.duration >= 0.008 }
                    .prefix(30).map { [ms($0.start - step.down), ms($0.duration)] }
                rows.append(entry)
            }
            let first = steps.first?.down ?? start
            return Self.json([
                "steps": rows,
                "changes": changes.filter { $0.time >= first - 0.05 }.prefix(60).map { ["ms": ms($0.time - first), "kind": $0.kind, "value": $0.value] },
                "frames": all.count,
                "postLateMs": ms(postedLate),
                "detailRect": "\(detailRect)",
                "displayedFrames": frames.count,
            ])
        }

        // MARK: Observers

        private func installObservers() {
            if observer == nil {
                let observer = CFRunLoopObserverCreateWithHandler(
                    kCFAllocatorDefault, CFRunLoopActivity.afterWaiting.rawValue | CFRunLoopActivity.beforeWaiting.rawValue, true, 0
                ) { [weak self] _, activity in
                    MainActor.assumeIsolated {
                        guard let self else { return }
                        let now = CACurrentMediaTime()
                        if activity == .afterWaiting {
                            self.busyStart = now
                        } else if self.busyStart > 0 {
                            let duration = now - self.busyStart
                            if duration >= 0.004 {
                                if self.busy.count >= 4000 { self.busy.removeFirst(2000) }
                                self.busy.append((self.busyStart, duration))
                            }
                            self.busyStart = 0
                        }
                    }
                }
                CFRunLoopAddObserver(CFRunLoopGetMain(), observer, .commonModes)
                self.observer = observer
            }
            if commitObserver == nil {
                // After Core Animation's own before-waiting commit (order 2000000).
                let observer = CFRunLoopObserverCreateWithHandler(
                    kCFAllocatorDefault, CFRunLoopActivity.beforeWaiting.rawValue, true, Int.max
                ) { [weak self] _, _ in
                    MainActor.assumeIsolated {
                        guard let self, !self.watched.isEmpty else { return }
                        let now = CACurrentMediaTime()
                        for index in self.watched.indices where self.watched[index].lost == nil && self.watched[index].down <= now {
                            let selected = self.watched[index].table.selectedRowIndexes.contains(self.watched[index].row)
                            if self.watched[index].selected == nil {
                                if selected { self.watched[index].selected = now }
                            } else if !selected {
                                self.watched[index].lost = now
                            }
                        }
                    }
                }
                CFRunLoopAddObserver(CFRunLoopGetMain(), observer, .commonModes)
                commitObserver = observer
            }
            if monitor == nil {
                monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseUp]) { [weak self] event in
                    MainActor.assumeIsolated {
                        self?.deliveries.append((CACurrentMediaTime(), event.type == .leftMouseDown ? "down" : "up", event.timestamp))
                        if event.type == .leftMouseDown { Self.signposter.emitEvent("mouseDown") }
                    }
                    return event
                }
                track()
            }
        }

        /// Records each change of the committed sidebar route and of the row
        /// the sidebar shows, when it happens (Observation's willSet).
        private func track() {
            guard let navigation else { return }
            withObservationTracking {
                _ = navigation.sidebarSelection
                _ = navigation.displayedSidebarSelection
            } onChange: { [weak self] in
                let time = CACurrentMediaTime()
                Task { @MainActor [weak self] in
                    guard let self, let navigation = self.navigation else { return }
                    let committed = Self.describe(navigation.sidebarSelection)
                    let displayed = Self.describe(navigation.displayedSidebarSelection)
                    if self.changes.last(where: { $0.kind == "selection" })?.value != committed {
                        self.changes.append((time, "selection", committed))
                        Self.signposter.emitEvent("commit", "\(committed)")
                    }
                    if self.changes.last(where: { $0.kind == "displayed" })?.value != displayed {
                        self.changes.append((time, "displayed", displayed))
                    }
                    if self.changes.count > 2000 { self.changes.removeFirst(1000) }
                    self.track()
                }
            }
        }

        private func startCapture(_ window: NSWindow) async throws {
            if capture?.stopped == true {
                capture = nil
                store.reset()
            }
            if capture != nil { return }
            let capture = TimingCapture(store: store)
            try await capture.start(windowID: CGWindowID(window.windowNumber), scale: window.backingScaleFactor)
            self.capture = capture
        }

        // MARK: Geometry

        private static func table(at point: CGPoint, in content: NSView) -> NSTableView? {
            var view = content.hitTest(content.convert(point, from: nil))
            while let current = view {
                if let table = current as? NSTableView { return table }
                view = current.superview
            }
            return nil
        }

        /// A window rect (bottom-left origin) in points from the window
        /// frame's top left, as the capture maps it.
        private static func topLeft(_ rect: CGRect, in window: NSWindow) -> CGRect {
            let height = window.frame.height
            return CGRect(x: rect.minX, y: height - rect.maxY, width: rect.width, height: rect.height)
        }

        /// `stats`, `deck:<id>`: what the test names.
        private static func describe(_ item: SidebarItem) -> String {
            switch item {
            case .section(let section): section.rawValue
            case .deck(let deck): "deck:\(deck.rawValue)"
            }
        }

        private static func json(_ object: [String: Any]) -> String {
            guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
                  let text = String(data: data, encoding: .utf8) else { return "{\"error\":\"unencodable\"}" }
            return text
        }
    }

    /// What each captured frame shows, compared with the last frame displayed
    /// before a run's first press. Written on the capture queue.
    nonisolated final class FrameStore: @unchecked Sendable {
        struct Record: Sendable {
            let time: TimeInterval
            let displayed: Bool
            /// The fraction of each step's row that differs from the baseline
            /// (`missing` when the frame does not cover it).
            let rows: [Double]
            /// The fraction of the detail column that differs from the baseline.
            let detail: Double
            /// The fraction of the detail column that differs from the previous frame.
            let delta: Double
        }

        /// A comparison the frame could not make: below every threshold.
        static let missing = -1.0

        enum Arming { case armed, noFrame, outside }

        static let rowThreshold = 0.25
        static let contentThreshold = 0.01
        static let flashThreshold = 0.002
        private static let channel = 20

        private let lock = NSLock()
        private var latest: CVPixelBuffer?
        private var latestGeometry = Geometry()
        private var armed = false
        private var rowRects: [CGRect] = []
        private var detailRect = CGRect.zero
        private var baselineRows: [[UInt8]] = []
        private var baselineDetail: [UInt8] = []
        private var previousDetail: [UInt8] = []
        private var records: [Record] = []

        struct Geometry: Sendable {
            var contentRect = CGRect.zero
            var contentScale: CGFloat = 1
            var scaleFactor: CGFloat = 1
        }

        func arm(rows: [CGRect], detail: CGRect) -> Arming {
            lock.lock()
            defer { lock.unlock() }
            guard let latest else { return .noFrame }
            let baseRows = rows.map { Self.sample(latest, latestGeometry, $0, stride: 3) }
            let baseDetail = Self.sample(latest, latestGeometry, detail, stride: 8)
            guard !baseDetail.isEmpty, !baseRows.contains(where: \.isEmpty) else { return .outside }
            rowRects = rows
            detailRect = detail
            baselineRows = baseRows
            baselineDetail = baseDetail
            previousDetail = baselineDetail
            records.removeAll()
            armed = true
            return .armed
        }

        /// Forgets the last frame: a failed stream's last frame is no baseline.
        func reset() {
            lock.lock()
            defer { lock.unlock() }
            latest = nil
            armed = false
            records.removeAll()
        }

        func disarm() -> [Record] {
            lock.lock()
            defer { lock.unlock() }
            armed = false
            return records
        }

        func received(_ pixels: CVPixelBuffer, geometry: Geometry, time: TimeInterval, displayed: Bool) {
            lock.lock()
            defer { lock.unlock() }
            latest = pixels
            latestGeometry = geometry
            guard armed else { return }
            let rows = zip(rowRects, baselineRows).map { rect, base in
                Self.difference(Self.sample(pixels, geometry, rect, stride: 3), base)
            }
            let detail = Self.sample(pixels, geometry, detailRect, stride: 8)
            records.append(Record(time: time, displayed: displayed, rows: rows,
                                  detail: Self.difference(detail, baselineDetail), delta: Self.difference(detail, previousDetail)))
            if !detail.isEmpty { previousDetail = detail }
        }

        private static func difference(_ a: [UInt8], _ b: [UInt8]) -> Double {
            guard a.count == b.count, !a.isEmpty else { return missing }
            var changed = 0
            var index = 0
            while index + 2 < a.count {
                if abs(Int(a[index]) - Int(b[index])) > channel || abs(Int(a[index + 1]) - Int(b[index + 1])) > channel
                    || abs(Int(a[index + 2]) - Int(b[index + 2])) > channel { changed += 1 }
                index += 3
            }
            return Double(changed) / Double(a.count / 3)
        }

        /// RGB of every `stride`th pixel of `rect` (points from the window's top left).
        private static func sample(_ pixels: CVPixelBuffer, _ geometry: Geometry, _ rect: CGRect, stride: Int) -> [UInt8] {
            CVPixelBufferLockBaseAddress(pixels, .readOnly)
            defer { CVPixelBufferUnlockBaseAddress(pixels, .readOnly) }
            guard let base = CVPixelBufferGetBaseAddress(pixels) else { return [] }
            let width = CVPixelBufferGetWidth(pixels), height = CVPixelBufferGetHeight(pixels)
            let bytesPerRow = CVPixelBufferGetBytesPerRow(pixels)
            let scale = geometry.contentScale * geometry.scaleFactor
            let minX = max(0, Int((geometry.contentRect.minX * geometry.scaleFactor) + rect.minX * scale))
            let minY = max(0, Int((geometry.contentRect.minY * geometry.scaleFactor) + rect.minY * scale))
            let maxX = min(width, Int((geometry.contentRect.minX * geometry.scaleFactor) + rect.maxX * scale))
            let maxY = min(height, Int((geometry.contentRect.minY * geometry.scaleFactor) + rect.maxY * scale))
            guard maxX > minX, maxY > minY else { return [] }
            var out: [UInt8] = []
            out.reserveCapacity(((maxX - minX) / stride + 1) * ((maxY - minY) / stride + 1) * 3)
            var y = minY
            while y < maxY {
                let line = base.advanced(by: y * bytesPerRow).assumingMemoryBound(to: UInt8.self)
                var x = minX
                while x < maxX {
                    let pixel = line.advanced(by: x * 4)
                    out.append(pixel[2]); out.append(pixel[1]); out.append(pixel[0])
                    x += stride
                }
                y += stride
            }
            return out
        }
    }

    /// The app's own window, every frame, through ScreenCaptureKit
    /// (`SCShareableContent.currentProcess` needs no permission).
    nonisolated final class TimingCapture: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
        private let store: FrameStore
        private var stream: SCStream?
        private let queue = DispatchQueue(label: "testhooks.sidebarTiming.capture")
        private static let timebase: mach_timebase_info_data_t = {
            var info = mach_timebase_info_data_t()
            mach_timebase_info(&info)
            return info
        }()

        init(store: FrameStore) { self.store = store }

        struct Failure: Error, CustomStringConvertible { let description: String }

        func start(windowID: CGWindowID, scale: CGFloat) async throws {
            let content = try await SCShareableContent.currentProcess
            guard let target = content.windows.first(where: { $0.windowID == windowID }) else {
                throw Failure(description: "ScreenCaptureKit does not offer the main window (\(content.windows.count) windows)")
            }
            let configuration = SCStreamConfiguration()
            configuration.width = Int(target.frame.width * scale)
            configuration.height = Int(target.frame.height * scale)
            configuration.minimumFrameInterval = CMTime(value: 1, timescale: 120)
            configuration.pixelFormat = kCVPixelFormatType_32BGRA
            configuration.colorSpaceName = CGColorSpace.sRGB
            configuration.showsCursor = false
            configuration.ignoreShadowsSingleWindow = true
            configuration.queueDepth = 8
            let stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: target), configuration: configuration, delegate: self)
            try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: queue)
            try await stream.startCapture()
            self.stream = stream
        }

        func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
            let arrived = CACurrentMediaTime()
            guard type == .screen, sampleBuffer.isValid,
                  let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
                  let info = attachments.first,
                  let raw = info[.status] as? Int, SCFrameStatus(rawValue: raw) == .complete,
                  let pixels = sampleBuffer.imageBuffer else { return }
            var geometry = FrameStore.Geometry()
            geometry.contentRect = (info[.contentRect] as? NSDictionary).flatMap { CGRect(dictionaryRepresentation: $0 as CFDictionary) } ?? .zero
            geometry.contentScale = info[.contentScale] as? CGFloat ?? 1
            geometry.scaleFactor = info[.scaleFactor] as? CGFloat ?? 1
            var time = arrived
            var displayed = false
            if let display = info[.displayTime] as? UInt64, display > 0 {
                time = Double(display) * Double(Self.timebase.numer) / Double(Self.timebase.denom) / 1e9
                displayed = true
            }
            store.received(pixels, geometry: geometry, time: time, displayed: displayed)
        }

        private let stopLock = NSLock()
        private var stoppedWithError = false
        /// Whether the stream stopped with an error: it delivers no more frames.
        var stopped: Bool {
            stopLock.lock()
            defer { stopLock.unlock() }
            return stoppedWithError
        }

        func stream(_ stream: SCStream, didStopWithError error: any Error) {
            stopLock.lock()
            stoppedWithError = true
            stopLock.unlock()
        }
    }
}
#endif
