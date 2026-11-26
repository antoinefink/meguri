import CoreGraphics
import Foundation

/// View-only coordinates: document geometry and writer size never depend on zoom.
struct CanvasViewport {
    var reference = CGSize.zero
    var area = CGSize.zero
    var zoom = 1.0
    var pan = CGSize.zero

    var origin: CGPoint {
        .init(x: (area.width - reference.width * zoom) / 2 + pan.width,
              y: (area.height - reference.height * zoom) / 2 + pan.height)
    }
    var imageRect: CGRect { screenRect(CGRect(origin: .zero, size: reference)) }

    func polygonFrame(_ points: [CGPoint]) -> CGRect? {
        guard let first = points.first else { return nil }
        let marker = CGRect(x: first.x - 5, y: first.y - 5, width: 10, height: 10)
        let bounds = points.dropFirst().reduce(marker) { box, point in
            CGRect(x: min(box.minX, point.x), y: min(box.minY, point.y),
                   width: max(box.maxX, point.x) - min(box.minX, point.x),
                   height: max(box.maxY, point.y) - min(box.minY, point.y))
        }
        return screenRect(bounds)
    }

    func referencePoint(_ point: CGPoint) -> CGPoint {
        .init(x: (point.x - origin.x) / zoom, y: (point.y - origin.y) / zoom)
    }
    func screenPoint(_ point: CGPoint) -> CGPoint {
        .init(x: origin.x + point.x * zoom, y: origin.y + point.y * zoom)
    }
    func screenRect(_ rect: CGRect) -> CGRect {
        .init(origin: screenPoint(rect.origin), size: .init(width: rect.width * zoom, height: rect.height * zoom))
    }
    mutating func resize(session: MaskEditorSession, imageSize: CGSize, area: CGSize, displayScale: Double) {
        guard area.width.isFinite, area.height.isFinite, area.width > 0, area.height > 0 else { return }
        // A canvas smaller than one native touch target is a layout proposal,
        // not a usable first authoring area. Later resizes keep the locked W/H.
        guard session.referenceSize != .zero || (area.width >= 44 && area.height >= 44) else { return }
        session.setReferenceSizeIfNeeded(imageSize: imageSize, area: area, displayScale: displayScale)
        reference = session.referenceSize
        self.area = area
        fit()
    }
    mutating func fit() {
        guard reference.width > 0, reference.height > 0, area.width > 0, area.height > 0,
              reference.width.isFinite, reference.height.isFinite, area.width.isFinite, area.height.isFinite else { return }
        zoom = min(area.width / reference.width, area.height / reference.height); pan = .zero
    }
    mutating func magnify(by factor: Double, at point: CGPoint) {
        guard factor.isFinite, factor > 0, zoom > 0 else { return }
        let fixed = referencePoint(point)
        zoom = min(5, max(0.5, zoom * factor))
        pan = .init(width: point.x - fixed.x * zoom - (area.width - reference.width * zoom) / 2,
                    height: point.y - fixed.y * zoom - (area.height - reference.height * zoom) / 2)
    }
}

enum ResizeHandle: String, CaseIterable, Identifiable {
    case topLeft, top, topRight, right, bottomRight, bottom, bottomLeft, left
    var id: Self { self }
    var isCorner: Bool { [.topLeft, .topRight, .bottomRight, .bottomLeft].contains(self) }
    func point(in box: CGRect) -> CGPoint {
        switch self {
        case .topLeft: .init(x: box.minX, y: box.minY)
        case .top: .init(x: box.midX, y: box.minY)
        case .topRight: .init(x: box.maxX, y: box.minY)
        case .right: .init(x: box.maxX, y: box.midY)
        case .bottomRight: .init(x: box.maxX, y: box.maxY)
        case .bottom: .init(x: box.midX, y: box.maxY)
        case .bottomLeft: .init(x: box.minX, y: box.maxY)
        case .left: .init(x: box.minX, y: box.midY)
        }
    }
    var opposite: Self {
        switch self {
        case .topLeft: .bottomRight
        case .top: .bottom
        case .topRight: .bottomLeft
        case .right: .left
        case .bottomRight: .topLeft
        case .bottom: .top
        case .bottomLeft: .topRight
        case .left: .right
        }
    }
}

extension MaskEditorSession {
    var uniformResize: Bool {
        let selected = document.shapes.filter { selection.contains($0.id) }
        return selected.count > 1 || selected.first?.isLabel == true
    }
    var canRotate: Bool { selection.count == 1 }

    /// Match rotated(): the pivot comes from committed geometry, never preview.
    var rotationPivot: CGPoint? {
        guard canRotate, let shape = document.shapes.first(where: { selection.contains($0.id) }) else { return nil }
        if case .polygon = shape.data.kind {
            let box = shape.bounds(in: referenceSize)
            return CGPoint(x: box.midX, y: box.midY)
        }
        return shape.unrotatedBounds(in: referenceSize).origin
    }

    func handlePoint(_ handle: ResizeHandle, in value: OcclusionDocument) -> CGPoint {
        if selection.count == 1, let shape = value.shapes.first(where: { selection.contains($0.id) }) {
            if case .polygon = shape.data.kind {} else {
                let box = shape.unrotatedBounds(in: referenceSize)
                let p = handle.point(in: box), angle = shape.data.angle * .pi / 180
                let dx = p.x - box.minX, dy = p.y - box.minY
                return .init(x: box.minX + dx * cos(angle) - dy * sin(angle),
                             y: box.minY + dx * sin(angle) + dy * cos(angle))
            }
        }
        return handle.point(in: value.bounds(of: selection, in: referenceSize) ?? .zero)
    }

    func resized(handle: ResizeHandle, delta: CGSize, finishing: Bool) -> OcclusionDocument {
        guard var box = document.bounds(of: selection, in: referenceSize), box.width > 0, box.height > 0 else { return document }
        let single = selection.count == 1 ? document.shapes.first(where: { selection.contains($0.id) }) : nil
        var angle = 0.0
        if let single {
            if case .polygon = single.data.kind {} else {
                box = single.unrotatedBounds(in: referenceSize); angle = single.data.angle * .pi / 180
            }
        }
        let delta = CGSize(width: delta.width * cos(angle) + delta.height * sin(angle),
                           height: -delta.width * sin(angle) + delta.height * cos(angle))
        let anchor = handle.opposite.point(in: box), point = handle.point(in: box)
        let changesX = point.x != anchor.x, changesY = point.y != anchor.y
        var x = changesX ? (point.x + delta.width - anchor.x) / (point.x - anchor.x) : 1
        var y = changesY ? (point.y + delta.height - anchor.y) / (point.y - anchor.y) : 1
        let selected = document.shapes.filter { selection.contains($0.id) }
        var minX = 0.0001, minY = 0.0001
        for shape in selected where !shape.isLabel {
            let bounds = shape.unrotatedBounds(in: referenceSize)
            if bounds.width > 0 { minX = max(minX, 5 / bounds.width) }
            if bounds.height > 0 { minY = max(minY, 5 / bounds.height) }
        }
        if uniformResize {
            let factor = max(max(minX, minY), abs(x - 1) > abs(y - 1) ? x : y)
            x = factor; y = factor
        } else { x = max(minX, x); y = max(minY, y) }
        var next = document
        for i in next.shapes.indices where selection.contains(next.shapes[i].id) {
            next.shapes[i].scale(x: x, y: y, about: anchor, in: referenceSize)
            if angle != 0, let single {
                let dx = (next.shapes[i].data.left - single.data.left) * referenceSize.width
                let dy = (next.shapes[i].data.top - single.data.top) * referenceSize.height
                next.shapes[i].data.left = single.data.left + (dx * cos(angle) - dy * sin(angle)) / referenceSize.width
                next.shapes[i].data.top = single.data.top + (dx * sin(angle) + dy * cos(angle)) / referenceSize.height
            }
        }
        if finishing { bringBack(&next) }
        return next
    }

    func rotated(degrees: Double, finishing: Bool) -> OcclusionDocument {
        guard canRotate else { return document }
        var next = document
        for i in next.shapes.indices where selection.contains(next.shapes[i].id) {
            let value: Double
            if case .polygon = next.shapes[i].data.kind { value = OcclusionShape.snappedAngle(degrees) }
            else { value = degrees }
            next.shapes[i].rotate(by: value, in: referenceSize)
        }
        if finishing { bringBack(&next) }
        return next
    }
}

struct OcclusionAccessibilityItem {
    let ids: [UUID]
    let first: OcclusionShape
    let identifier: String
    let frame: CGRect
}

extension OcclusionDocument {
    func accessibilityItems(in viewport: CanvasViewport) -> [OcclusionAccessibilityItem] {
        items.compactMap { ids in
            guard let id = ids.first, let first = shapes.first(where: { $0.id == id }),
                  let bounds = bounds(of: Set(ids), in: viewport.reference) else { return nil }
            let identifier: String
            if first.isLabel { identifier = "io.label.\(id.uuidString)" }
            else if first.data.number == 0 { identifier = "io.mask.zero.\(id.uuidString)" }
            else { identifier = "io.mask.\(first.data.number)" }
            return .init(ids: ids, first: first, identifier: identifier, frame: viewport.screenRect(bounds))
        }
    }
}
