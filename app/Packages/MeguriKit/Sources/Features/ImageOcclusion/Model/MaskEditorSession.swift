import CoreBridge
import CoreGraphics
import Foundation
import Observation

public enum MaskEditorTool: String, CaseIterable, Identifiable {
    case select, rectangle, ellipse, polygon, text, fill
    public var id: Self { self }
    var key: String { "io.tool.\(rawValue)" }
    var symbol: String {
        switch self {
        case .select: "cursorarrow"
        case .rectangle: "rectangle"
        case .ellipse: "oval"
        case .polygon: "pentagon"
        case .text: "textformat"
        case .fill: "paintpalette"
        }
    }
}

enum MaskAlignment: String, CaseIterable {
    case left, center, right, top, middle, bottom
    var key: String { "io.command.align" + rawValue.prefix(1).uppercased() + rawValue.dropFirst() }
}

/// Retain this above layout switches when the host removes the canvas for fields.
/// The four-argument MaskEditorView owns one for its own mounted lifetime; the
/// session initializer lets the note workflow retain precisely that same state.
@MainActor @Observable public final class MaskEditorSession {
    var document = OcclusionDocument()
    var preview: OcclusionDocument?
    var selection: Set<UUID> = []
    var polygon: [CGPoint] = []
    public var tool: MaskEditorTool { didSet { polygon = [] } }
    public private(set) var field: String
    public private(set) var errorKey: String?
    public private(set) var isReadable = false
    public private(set) var referenceSize = CGSize.zero
    var onWrite: ((String) -> Void)?
    public let palette: MaskPaletteModel?
    private var codec: OcclusionCodec?
    private var initial = OcclusionDocument()
    private var history: [OcclusionDocument] = []
    private var historyIndex = 0

    public var isDirty: Bool { document != initial }
    public var canUndo: Bool { !polygon.isEmpty || historyIndex > 0 }
    public var canRedo: Bool { historyIndex + 1 < history.count }
    var canGroup: Bool {
        Set(document.shapes.filter { selection.contains($0.id) && !$0.isLabel }.map { $0.data.number }).count > 1
    }
    var canUngroup: Bool { document.items.contains { $0.count > 1 && selection.contains($0[0]) } }
    var displayed: OcclusionDocument { preview ?? document }

    public init(field: String, isEditing: Bool, palette: MaskPaletteModel? = nil) {
        self.palette = palette
        self.field = field
        tool = isEditing ? .select : .rectangle
        load(field, isEditing: isEditing)
    }

    /// Host changes (including field editing and collection undo) replace the
    /// source and clear the local history. Our own binding echo is ignored.
    public func receive(_ value: String, isEditing: Bool) {
        guard value != field else { return }
        load(value, isEditing: isEditing)
    }

    private func load(_ value: String, isEditing: Bool) {
        field = value; preview = nil; polygon = []; selection = []; errorKey = nil
        do {
            let codec = try OcclusionCodec(field: value)
            self.codec = codec
            document = .init(shapes: codec.shapes.map { OcclusionShape($0) },
                             hideAll: value.isEmpty && !isEditing ? true : codec.hideAll)
            isReadable = true
        } catch {
            codec = nil; document = .init(); isReadable = false
            errorKey = "io.editor.unreadable.generic"
        }
        initial = document; history = [document]; historyIndex = 0
    }

    public func setReferenceSizeIfNeeded(imageSize: CGSize, area: CGSize, displayScale: Double) {
        guard referenceSize == .zero else { return }
        referenceSize = Self.referenceSize(imageSize: imageSize, area: area, displayScale: displayScale)
    }

    public static func referenceSize(imageSize: CGSize, area: CGSize, displayScale: Double) -> CGSize {
        guard imageSize.width > 0, imageSize.height > 0, area.width > 0, area.height > 0,
              imageSize.width.isFinite, imageSize.height.isFinite, area.width.isFinite, area.height.isFinite,
              displayScale.isFinite, displayScale > 0 else { return .zero }
        let fit = min(area.width / imageSize.width, area.height / imageSize.height)
        var width = imageSize.width * fit * displayScale, height = imageSize.height * fit * displayScale
        let reduction = min(1, sqrt(16_777_216 / (width * height)))
        width = floor(width * reduction); height = floor(height * reduction)
        return .init(width: width / displayScale, height: height / displayScale)
    }

    @discardableResult func commit(_ value: OcclusionDocument) -> Bool {
        preview = nil
        guard isReadable, value != document else { return false }
        guard let output = write(value) else { return false }
        history = Array(history.prefix(historyIndex + 1)); history.append(value); historyIndex += 1
        document = value
        selection.formIntersection(Set(value.shapes.map(\.id)))
        publish(output)
        return true
    }

    private func write(_ value: OcclusionDocument) -> String? {
        guard let codec else { return nil }
        do {
            return try codec.write(shapes: value.shapes.map(\.data), hideAll: value.hideAll,
                                   width: referenceSize.width, height: referenceSize.height)
        } catch { errorKey = "io.editor.unwritable"; return nil }
    }

    private func publish(_ output: String) {
        errorKey = nil; field = output
        // Even an empty label can change the document without changing output.
        onWrite?(output)
    }

    public func undo() {
        if !polygon.isEmpty { polygon = []; return }
        restore(historyIndex - 1)
    }
    public func redo() { polygon = []; restore(historyIndex + 1) }
    private func restore(_ index: Int) {
        guard history.indices.contains(index), index != historyIndex,
              let output = write(history[index]) else { return }
        historyIndex = index; document = history[index]; preview = nil
        selection.formIntersection(Set(document.shapes.map(\.id))); publish(output)
    }

    func setMode(_ hideAll: Bool) { var next = document; next.hideAll = hideAll; commit(next) }
    func select(_ id: UUID, extending: Bool = false) {
        let item = Set(document.item(containing: id))
        if extending {
            if item.isSubset(of: selection) { selection.subtract(item) } else { selection.formUnion(item) }
        } else if !item.isSubset(of: selection) { selection = item }
    }
    func selectAll() { selection = Set(document.shapes.map(\.id)) }

    func addBox(from start: CGPoint, to end: CGPoint, ellipse: Bool = false) {
        guard inside(start), let codec else { return }
        let box = CGRect(x: min(start.x, end.x), y: min(start.y, end.y),
                         width: abs(end.x - start.x), height: abs(end.y - start.y))
        guard box.width >= 5, box.height >= 5 else { return }
        do {
            let number = try codec.allocate(shapes: document.shapes.map(\.data))
            let width = box.width / referenceSize.width, height = box.height / referenceSize.height
            add(.init(number: number, left: box.minX / referenceSize.width, top: box.minY / referenceSize.height,
                      kind: ellipse ? .ellipse(rx: width / 2, ry: height / 2) : .rect(width: width, height: height)))
        } catch { errorKey = "io.editor.unwritable" }
    }
    func add(_ data: OcclusionShapeData) {
        let shape = OcclusionShape(data)
        var next = document; next.shapes.append(shape)
        if commit(next) { selection = [shape.id] }
    }
    func addPolygonPoint(_ point: CGPoint, closingDistance: Double) {
        guard inside(point) else { return }
        if let first = polygon.first, hypot(point.x - first.x, point.y - first.y) <= closingDistance {
            closePolygon(); return
        }
        polygon.append(point)
    }
    func closePolygon() {
        let points = polygon
        polygon = []
        guard points.count >= 3, let codec,
              let minX = points.map(\.x).min(), let maxX = points.map(\.x).max(),
              let minY = points.map(\.y).min(), let maxY = points.map(\.y).max(),
              maxX - minX > 5, maxY - minY > 5 else { return }
        do {
            let number = try codec.allocate(shapes: document.shapes.map(\.data))
            add(.init(number: number, left: minX / referenceSize.width, top: minY / referenceSize.height,
                      kind: .polygon(points: points.map { .init(x: $0.x / referenceSize.width, y: $0.y / referenceSize.height) })))
        } catch { errorKey = "io.editor.unwritable" }
    }
    func addLabel(at point: CGPoint) {
        guard inside(point) else { return }
        add(.init(number: 0, left: point.x / referenceSize.width, top: point.y / referenceSize.height,
                  kind: .text(text: "text", scale: 1, fs: 40 / referenceSize.height)))
        tool = .select
    }
    func editLabel(_ id: UUID, text: String) {
        guard !text.contains("}}") else { errorKey = "io.label.braces"; return }
        var next = document
        guard let i = next.shapes.firstIndex(where: { $0.id == id }),
              case let .text(_, scale, fs) = next.shapes[i].data.kind else { return }
        next.shapes[i].data.kind = .text(text: text, scale: scale, fs: fs); commit(next)
    }
    func rejectLabelBraces() { errorKey = "io.label.braces" }
    func deleteSelection() {
        var next = document; next.shapes.removeAll { selection.contains($0.id) }; commit(next)
    }
    func fill(_ id: UUID, color: String) {
        var next = document
        guard let i = next.shapes.firstIndex(where: { $0.id == id }) else { return }
        let fallback = next.shapes[i].isLabel ? MaskAppearance.labelFill : MaskAppearance.defaultFill
        guard (next.shapes[i].data.fill ?? fallback).lowercased() != color else { return }
        next.shapes[i].data.fill = color == fallback ? nil : color; commit(next)
    }
    func duplicate() {
        guard let codec, !selection.isEmpty else { return }
        do {
            let indices = document.shapes.indices.filter { selection.contains(document.shapes[$0].id) }.map { UInt32($0) }
            var copies = try codec.numbers(shapes: document.shapes.map(\.data), action: .duplicate(selected: indices))
                .map { OcclusionShape($0) }
            for i in copies.indices { copies[i].move(dx: 10 / referenceSize.width, dy: 10 / referenceSize.height) }
            var next = document; next.shapes += copies
            if commit(next) { selection = Set(copies.map(\.id)) }
        } catch { errorKey = "io.editor.unwritable" }
    }
    func group() {
        guard canGroup else { return }
        numberChange(.group(selected: document.shapes.indices.filter { selection.contains(document.shapes[$0].id) }.map { UInt32($0) }))
    }
    func ungroup() {
        guard let codec, canUngroup else { return }
        do {
            var next = document
            let numbers = document.items.filter { $0.count > 1 && selection.contains($0[0]) }.compactMap { ids in
                document.shapes.first { $0.id == ids[0] }?.data.number
            }
            for number in numbers {
                let data = try codec.numbers(shapes: next.shapes.map(\.data), action: .ungroup(number: number))
                for i in data.indices { next.shapes[i].data = data[i] }
            }
            commit(next)
        } catch { errorKey = "io.editor.unwritable" }
    }
    private func numberChange(_ action: OcclusionNumberAction) {
        guard let codec else { return }
        do {
            let data = try codec.numbers(shapes: document.shapes.map(\.data), action: action)
            var next = document
            for i in data.indices { next.shapes[i].data = data[i] }
            commit(next)
        } catch { errorKey = "io.editor.unwritable" }
    }

    func inside(_ p: CGPoint) -> Bool {
        p.x >= 0 && p.y >= 0 && p.x <= referenceSize.width && p.y <= referenceSize.height
    }

    func moved(dx: Double, dy: Double, finishing: Bool) -> OcclusionDocument {
        var next = document
        for item in document.items where selection.contains(item[0]) {
            let ids = Set(item)
            guard let box = document.bounds(of: ids, in: referenceSize) else { continue }
            var x = dx, y = dy
            if box.width <= referenceSize.width { x = min(max(x, -box.minX), referenceSize.width + 5 - box.maxX) }
            if box.height <= referenceSize.height { y = min(max(y, -box.minY), referenceSize.height + 5 - box.maxY) }
            for i in next.shapes.indices where ids.contains(next.shapes[i].id) {
                next.shapes[i].move(dx: x / referenceSize.width, dy: y / referenceSize.height)
            }
        }
        if finishing { bringBack(&next) }
        return next
    }

    func bringBack(_ next: inout OcclusionDocument) {
        for item in next.items where selection.contains(item[0]) {
            let ids = Set(item)
            guard let box = next.bounds(of: ids, in: referenceSize) else { continue }
            let dx = box.minX < 0 ? -box.minX : (box.minX > referenceSize.width ? referenceSize.width - box.maxX : 0)
            let dy = box.minY < 0 ? -box.minY : (box.minY > referenceSize.height ? referenceSize.height - box.maxY : 0)
            for i in next.shapes.indices where ids.contains(next.shapes[i].id) {
                next.shapes[i].move(dx: dx / referenceSize.width, dy: dy / referenceSize.height)
            }
        }
    }

    func align(_ alignment: MaskAlignment) {
        let items = document.items.filter { selection.contains($0[0]) }
        guard !items.isEmpty, let all = document.bounds(of: selection, in: referenceSize) else { return }
        let target = items.count == 1 ? CGRect(origin: .zero, size: referenceSize) : all
        var next = document
        for item in items {
            guard let box = document.bounds(of: Set(item), in: referenceSize) else { continue }
            var dx = 0.0, dy = 0.0
            switch alignment {
            case .left: dx = target.minX - box.minX
            case .center: dx = target.midX - box.midX
            case .right: dx = target.maxX - box.maxX
            case .top: dy = target.minY - box.minY
            case .middle: dy = target.midY - box.midY
            case .bottom: dy = target.maxY - box.maxY
            }
            for i in next.shapes.indices where item.contains(next.shapes[i].id) {
                next.shapes[i].move(dx: dx / referenceSize.width, dy: dy / referenceSize.height)
            }
        }
        commit(next)
    }
}
