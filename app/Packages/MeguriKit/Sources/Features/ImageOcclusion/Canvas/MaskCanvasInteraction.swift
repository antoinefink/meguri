import CoreBridge
import CoreGraphics
import Foundation
import Observation

/// One lifecycle for drawing, moving and handles. Navigation cancels the
/// current gesture even if its final callback arrives after navigation ends.
@MainActor @Observable final class MaskCanvasInteraction {
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
            let hit = session.document.shapes.reversed().first { $0.path(in: session.referenceSize).contains(start) }
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
            let hit = session.document.shapes.contains { $0.path(in: session.referenceSize).contains(start) }
            if hit {
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
            let box = rect(from: start, to: point)
            var preview = session.document
            let width = box.width / session.referenceSize.width, height = box.height / session.referenceSize.height
            preview.shapes.append(.init(.init(number: 0, left: box.minX / session.referenceSize.width,
                                              top: box.minY / session.referenceSize.height,
                                              kind: drawingTool == .ellipse ? .ellipse(rx: width / 2, ry: height / 2) : .rect(width: width, height: height))))
            session.preview = preview
        }
    }

    func ended(point: CGPoint, fill: String, closingDistance: Double, session: MaskEditorSession) -> UUID? {
        defer { dragStart = nil; drawingTool = nil; moved = false; pointerCancelled = false; session.preview = nil; selectionBox = nil }
        guard !isNavigating, let start = dragStart else { return nil }
        switch drawingTool {
        case .rectangle: session.addBox(from: start, to: point)
        case .ellipse: session.addBox(from: start, to: point, ellipse: true)
        case .polygon: session.addPolygonPoint(start, closingDistance: closingDistance)
        case .text:
            guard session.inside(start) else { return nil }
            session.addLabel(at: start)
            return session.selection.first
        case .fill:
            if let shape = session.document.shapes.reversed().first(where: { $0.path(in: session.referenceSize).contains(start) }) {
                session.fill(shape.id, color: fill)
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
        if handleActive { handleCancelled = true }
        if dragStart != nil { pointerCancelled = true; session.selection = selectionAtStart }
        dragStart = nil; drawingTool = nil; session.preview = nil; selectionBox = nil
    }
    func navigationEnded() { isNavigating = false }
    func pointerGestureEnded(session: MaskEditorSession) {
        dragStart = nil; drawingTool = nil; moved = false; pointerCancelled = false
        session.preview = nil; selectionBox = nil
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
