import CoreBridge
import CoreGraphics
import Foundation
import Observation

/// One lifecycle for drawing, moving and handles. Navigation cancels the
/// current gesture even if its final callback arrives after navigation ends.
@MainActor @Observable final class MaskCanvasInteraction {
    private var hitID: UUID?
    private var previewNumber: UInt16?
    private var previewNumberAttempted = false
    private var previewSource: OcclusionDocument?
    private var nativeHandle: CanvasHandleGeometry.Target?
    private var nativeStart: CGPoint?
    private var nativeInputRevision: Int?
    private var nativePivot: CGPoint?
    private var nativeMoved = false
    private var nativeLabelCandidate: UUID?
    private var nativeInside = false
    private var dragStart: CGPoint?
    private var drawingTool: MaskEditorTool?
    private(set) var selectionBox: CGRect?
    private var extending = false
    private var selectionAtStart: Set<UUID> = []
    private var isNavigating = false
    private var handleActive = false
    private var handleCancelled = false
    private var moved = false
    private var pointerCancelled = false

    func changed(start: CGPoint, point: CGPoint, extending: Bool, session: MaskEditorSession) {
        guard !isNavigating, !pointerCancelled, session.isReadable else { return }
        if dragStart == nil {
            dragStart = start; selectionAtStart = session.selection
            self.extending = extending
            moved = false
            if session.tool == .polygon { drawingTool = .polygon; return }
            let hit = session.shape(at: start)
            hitID = hit?.id
            if let hit {
                if session.tool == .fill { drawingTool = .fill; return }
                session.select(hit.id, extending: extending)
                drawingTool = .select
            } else {
                drawingTool = session.tool
                if session.tool == .select, !extending { session.selection = [] }
            }
        }
        guard let start = dragStart else { return }
        moved = moved || point != start
        if drawingTool == .select {
            if hitID != nil {
                if moved { session.preview = session.moved(dx: point.x - start.x, dy: point.y - start.y, finishing: false) }
            } else if moved {
                let box = rect(from: start, to: point); selectionBox = box
                var selected = self.extending ? selectionAtStart : []
                for item in session.document.items {
                    if let bounds = session.document.bounds(of: Set(item), in: session.referenceSize), bounds.intersects(box) {
                        selected.formUnion(item)
                    }
                }
                session.selection = selected
            }
        } else if (drawingTool == .rectangle || drawingTool == .ellipse), session.inside(start) {
            guard moved else { return }
            if !previewNumberAttempted {
                previewNumberAttempted = true
                previewSource = session.document
                previewNumber = session.boxPreviewNumber()
            }
            guard let previewNumber else { return }
            let box = rect(from: start, to: point)
            var preview = session.document
            let width = box.width / session.referenceSize.width, height = box.height / session.referenceSize.height
            preview.shapes.append(.init(.init(number: previewNumber, left: box.minX / session.referenceSize.width,
                                              top: box.minY / session.referenceSize.height,
                                              kind: drawingTool == .ellipse ? .ellipse(rx: width / 2, ry: height / 2) : .rect(width: width, height: height))))
            session.preview = preview
        }
    }

    func ended(point: CGPoint, fill: String, closingDistance: Double, session: MaskEditorSession) -> UUID? {
        defer { pointerGestureEnded(session: session) }
        guard !isNavigating, let start = dragStart else { return nil }
        switch drawingTool {
        case .rectangle, .ellipse:
            if !moved, point == start, session.inside(point) {
                session.selection = []
            } else {
                session.addBox(from: start, to: point, ellipse: drawingTool == .ellipse,
                               previewNumber: previewSource == session.document ? previewNumber : nil)
            }
        case .polygon: session.addPolygonPoint(start, closingDistance: closingDistance)
        case .text:
            guard session.inside(start) else { return nil }
            session.addLabel(at: start)
            return session.selection.first
        case .fill:
            if let hitID {
                session.fill(hitID, color: fill)
            }
        case .select:
            // A click only selects. Even a zero-distance transform can normalize
            // imported polygon points or bring an imported item into the image.
            if selectionBox == nil, moved, point != start {
                session.commit(session.moved(dx: point.x - start.x, dy: point.y - start.y, finishing: true))
            }
        case nil: break
        }
        return nil
    }

    func navigationStarted(session: MaskEditorSession) {
        isNavigating = true
        nativeStart = nil; nativeHandle = nil; nativePivot = nil; nativeLabelCandidate = nil
        if handleActive { handleCancelled = true }
        if dragStart != nil {
            pointerCancelled = true
            if nativeInputRevision == nil || nativeInputRevision == session.inputRevision {
                session.selection = selectionAtStart
            }
        }
        nativeInputRevision = nil
        dragStart = nil; drawingTool = nil; session.preview = nil; selectionBox = nil
        previewNumber = nil; previewSource = nil; previewNumberAttempted = false
    }
    func navigationEnded() { isNavigating = false }
    func pointerGestureEnded(session: MaskEditorSession) {
        dragStart = nil; drawingTool = nil; moved = false; pointerCancelled = false
        session.preview = nil; selectionBox = nil
        hitID = nil; previewNumber = nil; previewSource = nil; previewNumberAttempted = false
    }

    // Native press ownership is separate from SwiftUI's touch lifecycle. A
    // stationary handle press never invokes a normalizing transform.
    enum PointerCue: Equatable { case arrow, openHand, closedHand, crosshair, text, fill, resize(ResizeHandle), rotate }
    var nativeCue: PointerCue? {
        guard nativeStart != nil else { return nil }
        if let nativeHandle {
            switch nativeHandle {
            case let .resize(handle): return .resize(handle)
            case .rotate: return .rotate
            }
        }
        if hitID != nil, drawingTool == .select { return .closedHand }
        switch drawingTool {
        case .select: return .crosshair
        case .rectangle, .ellipse, .polygon: return nativeInside ? .crosshair : .arrow
        case .text: return nativeInside ? .text : .arrow
        case .fill: return .fill
        case nil: return .arrow
        }
    }
    func nativeBegan(at point: CGPoint, viewport: CanvasViewport, extending: Bool, clickCount: Int = 1, session: MaskEditorSession) {
        nativeStart = point; nativeMoved = false; nativeLabelCandidate = nil
        nativeInputRevision = session.inputRevision
        nativeInside = session.inside(viewport.referencePoint(point))
        nativeHandle = session.canvasHandle(at: point, in: viewport)?.target
        nativePivot = session.rotationPivot.map(viewport.screenPoint)
        if nativeHandle == nil {
            changed(start: viewport.referencePoint(point), point: viewport.referencePoint(point), extending: extending, session: session)
            if clickCount == 2, drawingTool == .select,
               let shape = session.document.shapes.first(where: { $0.id == hitID }), shape.isLabel {
                nativeLabelCandidate = shape.id
            }
        }
    }
    func nativeChanged(at point: CGPoint, viewport: CanvasViewport, session: MaskEditorSession) {
        guard let start = nativeStart else { return }
        guard nativeInputRevision == session.inputRevision else { nativeCancelled(session: session); return }
        nativeMoved = nativeMoved || point != start
        if let nativeHandle {
            guard nativeMoved else { return }
            updateNativeHandle(nativeHandle, start: start, point: point, viewport: viewport, finishing: false, session: session)
        } else {
            changed(start: viewport.referencePoint(start), point: viewport.referencePoint(point), extending: false, session: session)
        }
    }
    func nativeEnded(at point: CGPoint, viewport: CanvasViewport, fill: String, session: MaskEditorSession) -> UUID? {
        defer { nativeCancelled(session: session) }
        guard let start = nativeStart, nativeInputRevision == session.inputRevision else { return nil }
        if let nativeHandle {
            if nativeMoved, point != start {
                updateNativeHandle(nativeHandle, start: start, point: point, viewport: viewport, finishing: true, session: session)
            }
            return nil
        }
        let label = !nativeMoved && point == start ? nativeLabelCandidate : nil
        let addedLabel = ended(point: viewport.referencePoint(point), fill: fill, closingDistance: 5, session: session)
        if let label { session.select(label) }
        return label ?? addedLabel
    }
    func nativeCancelled(session: MaskEditorSession) {
        if dragStart != nil, nativeInputRevision == session.inputRevision { session.selection = selectionAtStart }
        nativeStart = nil; nativeHandle = nil; nativePivot = nil; nativeMoved = false; nativeLabelCandidate = nil
        nativeInputRevision = nil; nativeInside = false
        pointerGestureEnded(session: session); handleGestureEnded(session: session)
    }
    private func updateNativeHandle(_ target: CanvasHandleGeometry.Target, start: CGPoint, point: CGPoint,
                                    viewport: CanvasViewport, finishing: Bool, session: MaskEditorSession) {
        switch target {
        case let .resize(handle):
            resize(handle, delta: .init(width: (point.x - start.x) / viewport.zoom,
                                       height: (point.y - start.y) / viewport.zoom), finishing: finishing, session: session)
        case .rotate:
            guard let pivot = nativePivot else { return }
            let angle = atan2(point.y - pivot.y, point.x - pivot.x) - atan2(start.y - pivot.y, start.x - pivot.x)
            rotate(degrees: angle * 180 / .pi, finishing: finishing, session: session)
        }
    }

    func resize(_ handle: ResizeHandle, delta: CGSize, finishing: Bool, session: MaskEditorSession) {
        transform(finishing: finishing, session: session) { session.resized(handle: handle, delta: delta, finishing: finishing) }
    }
    func rotate(degrees: Double, finishing: Bool, session: MaskEditorSession) {
        transform(finishing: finishing, session: session) { session.rotated(degrees: degrees, finishing: finishing) }
    }
    // SwiftUI resets GestureState even when cancellation omits onEnded.
    func handleGestureEnded(session: MaskEditorSession) {
        handleActive = false; handleCancelled = false; session.preview = nil
    }
    private func transform(finishing: Bool, session: MaskEditorSession, value: () -> OcclusionDocument) {
        guard !finishing || handleActive else { return }
        defer {
            if finishing { handleActive = false; handleCancelled = false; session.preview = nil }
        }
        if !handleActive { handleActive = true }
        if isNavigating { handleCancelled = true }
        guard !handleCancelled, !isNavigating, session.isReadable else { return }
        if finishing { session.commit(value()) } else { session.preview = value() }
    }
    private func rect(from a: CGPoint, to b: CGPoint) -> CGRect {
        .init(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(a.x - b.x), height: abs(a.y - b.y))
    }
}
