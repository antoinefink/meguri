import CoreBridge
import CoreGraphics
import CoreText
import Foundation

struct OcclusionShape: Identifiable, Equatable {
    let id: UUID
    var data: OcclusionShapeData
    init(_ data: OcclusionShapeData, id: UUID = UUID()) { self.id = id; self.data = data }
    var isLabel: Bool { if case .text = data.kind { true } else { false } }

    /// All geometry calculations use reference points, including rotations.
    func unrotatedBounds(in size: CGSize) -> CGRect {
        let origin = CGPoint(x: data.left * size.width, y: data.top * size.height)
        let extent: CGSize
        switch data.kind {
        case let .rect(width, height): extent = .init(width: width * size.width, height: height * size.height)
        case let .ellipse(rx, ry): extent = .init(width: 2 * rx * size.width, height: 2 * ry * size.height)
        case let .polygon(points):
            let xs = points.map(\.x), ys = points.map(\.y)
            extent = .init(width: ((xs.max() ?? 0) - (xs.min() ?? 0)) * size.width,
                           height: ((ys.max() ?? 0) - (ys.min() ?? 0)) * size.height)
        case let .text(text, scale, fs):
            let font = CTFontCreateWithName("Arial" as CFString, (fs.map { $0 * size.height } ?? 40), nil)
            var character: UniChar = 77
            var glyph: CGGlyph = 0
            CTFontGetGlyphsForCharacters(font, &character, &glyph, 1)
            let glyphBounds = CTFontGetBoundingRectsForGlyphs(font, .default, &glyph, nil, 1)
            let lines = text.components(separatedBy: "\n")
            let width = lines.map { line in
                let attributed = NSAttributedString(string: line, attributes: [
                    NSAttributedString.Key(kCTFontAttributeName as String): font,
                ])
                return CTLineGetTypographicBounds(CTLineCreateWithAttributedString(attributed), nil, nil, nil)
            }.max() ?? 0
            extent = .init(width: (width + 5) * scale,
                           height: (Double(lines.count) * 1.5 * glyphBounds.height + 5) * scale)
        }
        return CGRect(origin: origin, size: extent)
    }

    func path(in size: CGSize) -> CGPath {
        let box = unrotatedBounds(in: size)
        let path = CGMutablePath()
        switch data.kind {
        case .rect, .text: path.addRect(box)
        case .ellipse: path.addEllipse(in: box)
        case let .polygon(points):
            let minX = points.map(\.x).min() ?? 0, minY = points.map(\.y).min() ?? 0
            let drawn = points.map { CGPoint(x: ($0.x - minX + data.left) * size.width,
                                            y: ($0.y - minY + data.top) * size.height) }
            path.addLines(between: drawn); path.closeSubpath()
            return path // Stored polygon angles are ignored by the format.
        }
        var transform = CGAffineTransform(translationX: box.minX, y: box.minY)
            .rotated(by: data.angle * .pi / 180).translatedBy(x: -box.minX, y: -box.minY)
        return path.copy(using: &transform) ?? path
    }

    func bounds(in size: CGSize) -> CGRect { path(in: size).boundingBoxOfPath }

    mutating func move(dx: Double, dy: Double) {
        if case let .polygon(points) = data.kind {
            let minX = points.map(\.x).min() ?? 0, minY = points.map(\.y).min() ?? 0
            data.kind = .polygon(points: points.map {
                .init(x: $0.x - minX + data.left + dx, y: $0.y - minY + data.top + dy)
            })
        }
        data.left += dx; data.top += dy
    }

    mutating func scale(x: Double, y: Double, about anchor: CGPoint, in size: CGSize) {
        let oldLeft = data.left, oldTop = data.top
        // Normalize polygon points before changing its origin (M5).
        if case let .polygon(points) = data.kind {
            let minX = points.map(\.x).min() ?? 0, minY = points.map(\.y).min() ?? 0
            data.kind = .polygon(points: points.map {
                .init(x: anchor.x / size.width + ($0.x - minX + oldLeft - anchor.x / size.width) * x,
                      y: anchor.y / size.height + ($0.y - minY + oldTop - anchor.y / size.height) * y)
            })
        }
        data.left = anchor.x / size.width + (oldLeft - anchor.x / size.width) * x
        data.top = anchor.y / size.height + (oldTop - anchor.y / size.height) * y
        switch data.kind {
        case let .rect(width, height): data.kind = .rect(width: width * x, height: height * y)
        case let .ellipse(rx, ry): data.kind = .ellipse(rx: rx * x, ry: ry * y)
        case let .text(text, scale, fs): data.kind = .text(text: text, scale: scale * x, fs: fs)
        case let .polygon(points):
            data.left = points.map(\.x).min() ?? data.left
            data.top = points.map(\.y).min() ?? data.top
        }
    }

    mutating func rotate(by degrees: Double, in size: CGSize) {
        if case .polygon = data.kind {
            let box = bounds(in: size), center = CGPoint(x: box.midX, y: box.midY)
            let path = path(in: size)
            var vertices: [OcclusionPoint] = []
            let angle = degrees * .pi / 180
            path.applyWithBlock { element in
                switch element.pointee.type {
                case .moveToPoint, .addLineToPoint:
                    let p = element.pointee.points[0]
                    let dx = p.x - center.x, dy = p.y - center.y
                    vertices.append(.init(x: (center.x + dx * cos(angle) - dy * sin(angle)) / size.width,
                                          y: (center.y + dx * sin(angle) + dy * cos(angle)) / size.height))
                default: break
                }
            }
            data.kind = .polygon(points: vertices)
            data.left = vertices.map(\.x).min() ?? data.left
            data.top = vertices.map(\.y).min() ?? data.top
            data.angle = 0
        } else {
            data.angle = Self.snappedAngle(data.angle + degrees)
        }
    }

    static func snappedAngle(_ angle: Double) -> Double {
        var a = angle.truncatingRemainder(dividingBy: 360)
        if a <= -180 { a += 360 }; if a > 180 { a -= 360 }
        return abs(a) < 3 ? 0 : a
    }
}

struct OcclusionDocument: Equatable {
    var shapes: [OcclusionShape] = []
    var hideAll = true

    /// Groups are derived from numbered masks; labels and c0 stay independent.
    var items: [[UUID]] {
        var result: [[UUID]] = []
        var used: Set<UUID> = []
        for shape in shapes where !used.contains(shape.id) {
            let members = shape.isLabel || shape.data.number == 0 ? [shape] : shapes.filter {
                !$0.isLabel && $0.data.number == shape.data.number
            }
            let ids = members.map(\.id)
            result.append(ids); used.formUnion(ids)
        }
        return result
    }
    func item(containing id: UUID) -> [UUID] { items.first { $0.contains(id) } ?? [] }
    func bounds(of ids: Set<UUID>, in size: CGSize) -> CGRect? {
        shapes.filter { ids.contains($0.id) }.map { $0.bounds(in: size) }.reduce(nil) { result, box in
            result.map { $0.union(box) } ?? box
        }
    }
}
