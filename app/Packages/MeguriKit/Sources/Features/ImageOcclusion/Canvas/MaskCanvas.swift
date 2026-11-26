import CoreBridge
import SwiftUI
#if os(macOS)
import AppKit
#endif

struct MaskCanvas: View {
    let image: Image
    @Bindable var session: MaskEditorSession
    @Binding var viewport: CanvasViewport
    let translucent: Bool
    let fill: String
    let editLabel: (UUID) -> Void
    @State private var dragStart: CGPoint?
    @State private var drawingTool: MaskEditorTool?
    @State private var selectionBox: CGRect?
    @State private var extending = false
    @State private var selectionAtStart: Set<UUID> = []
    @State private var isNavigating = false
    @State private var pointer: CGPoint?

    var body: some View {
        Canvas { context, _ in
            context.translateBy(x: viewport.origin.x, y: viewport.origin.y)
            context.scaleBy(x: viewport.zoom, y: viewport.zoom)
            context.draw(image, in: CGRect(origin: .zero, size: viewport.reference))
            for shape in session.displayed.shapes { draw(shape, in: &context) }
            drawPolygon(in: &context)
            if let selectionBox {
                context.stroke(Path(selectionBox), with: .color(.accentColor), style: .init(lineWidth: 1 / viewport.zoom, dash: [4 / viewport.zoom]))
            }
            if let box = session.displayed.bounds(of: session.selection, in: viewport.reference) {
                context.stroke(Path(box), with: .color(.accentColor), lineWidth: 1 / viewport.zoom)
            }
        }
        .contentShape(Rectangle())
        .onContinuousHover { phase in
            switch phase {
            case let .active(location): pointer = viewport.referencePoint(location)
            case .ended: pointer = nil
            }
        }
        .coordinateSpace(name: "io.canvas")
        .gesture(DragGesture(minimumDistance: 0).onChanged(dragChanged).onEnded(dragEnded))
        .overlay { handles }
        .overlay {
            CanvasNavigationInput(
                navigate: { factor, delta, anchor in
                    cancelDrag(); isNavigating = true
                    if factor != 1 { viewport.magnify(by: factor, at: anchor) }
                    viewport.pan.width += delta.width; viewport.pan.height += delta.height
                },
                ended: { isNavigating = false },
                panStarted: { cancelDrag(); isNavigating = true }
            )
            .allowsHitTesting(false)
        }
        .clipped()
        .accessibilityElement(children: .contain)
        .accessibilityLabel(ioString("io.a11y.canvas"))
        .accessibilityIdentifier("io.canvas")
        .accessibilityChildren {
            ForEach(session.document.items, id: \.self) { item in
                if let first = session.document.shapes.first(where: { $0.id == item[0] }) {
                    let box = viewport.screenRect(session.document.bounds(of: Set(item), in: viewport.reference) ?? .zero)
                    Color.clear
                        .frame(width: max(1, box.width), height: max(1, box.height))
                        .position(x: box.midX, y: box.midY)
                        .accessibilityLabel(itemLabel(item, first: first))
                        .accessibilityIdentifier(first.data.number > 0 ? "io.mask.\(first.data.number)" : "io.label.\(first.id.uuidString)")
                        .accessibilityAddTraits(session.selection.contains(first.id) ? [.isSelected, .isButton] : [.isButton])
                        .accessibilityAction { session.select(first.id) }
                        .accessibilityAction(named: Text(ioString("io.command.delete"))) {
                            session.select(first.id); session.deleteSelection()
                        }
                        .accessibilityAction(named: Text(ioString("io.command.duplicate"))) {
                            session.select(first.id); session.duplicate()
                        }
                        .accessibilityAction(named: Text(ioString("io.command.group"))) {
                            session.select(first.id, extending: true); session.group()
                        }
                        .accessibilityAction(named: Text(ioString("io.command.ungroup"))) {
                            session.select(first.id); session.ungroup()
                        }
                        .accessibilityAdjustableAction { direction in
                            session.select(first.id)
                            let delta = direction == .increment ? 5.0 : -5.0
                            session.commit(session.moved(dx: delta, dy: 0, finishing: true))
                        }
                }
            }
            if !session.polygon.isEmpty {
                Text(String(format: ioString("io.a11y.polygon.unfinished"), Int64(session.polygon.count)))
                    .accessibilityAction(named: Text(ioString("io.command.closePolygon"))) { session.closePolygon() }
            }
        }
    }

    private func draw(_ shape: OcclusionShape, in context: inout GraphicsContext) {
        var layer = context
        layer.opacity = translucent ? 0.4 : 1
        let box = shape.unrotatedBounds(in: viewport.reference)
        let path = Path(shape.path(in: viewport.reference))
        if case let .text(text, scale, fs) = shape.data.kind {
            layer.translateBy(x: box.minX, y: box.minY)
            layer.rotate(by: .degrees(shape.data.angle))
            layer.fill(Path(CGRect(origin: .zero, size: box.size)), with: .color(.white))
            let fontSize = (fs.map { $0 * viewport.reference.height } ?? 40) * scale
            // CoreText supplies the same Arial M-box line spacing as the model.
            let lines = text.components(separatedBy: "\n")
            let lineHeight = (box.height - 5 * scale) / Double(max(1, lines.count))
            for (index, line) in lines.enumerated() {
                layer.draw(Text(line).font(.custom("Arial", size: fontSize))
                    .foregroundStyle(maskColor(shape.data.fill, fallback: .black)),
                           at: .init(x: 0, y: Double(index) * lineHeight), anchor: .topLeading)
            }
        } else {
            layer.fill(path, with: .color(maskColor(shape.data.fill, fallback: maskDefaultColor)))
            layer.stroke(path, with: .color(.primary), lineWidth: 1 / viewport.zoom)
            let bounds = shape.bounds(in: viewport.reference)
            context.draw(Text(String(shape.data.number)).font(.caption).foregroundStyle(.primary),
                         at: .init(x: bounds.midX, y: bounds.midY))
        }
    }

    private func drawPolygon(in context: inout GraphicsContext) {
        guard let first = session.polygon.first else { return }
        var path = Path(); path.move(to: first)
        for point in session.polygon.dropFirst() { path.addLine(to: point) }
        if let pointer { path.addLine(to: pointer) }
        context.stroke(path, with: .color(.accentColor), lineWidth: 1 / viewport.zoom)
        let radius = 5.0
        context.stroke(Path(ellipseIn: .init(x: first.x - radius, y: first.y - radius, width: 2 * radius, height: 2 * radius)),
                       with: .color(.accentColor), lineWidth: 1 / viewport.zoom)
    }

    private func itemLabel(_ item: [UUID], first: OcclusionShape) -> String {
        if item.count > 1 { return String(format: ioString("io.a11y.group"), Int64(first.data.number), Int64(item.count)) }
        switch first.data.kind {
        case .rect: return String(format: ioString("io.a11y.rectangle"), Int64(first.data.number))
        case .ellipse: return String(format: ioString("io.a11y.ellipse"), Int64(first.data.number))
        case .polygon: return String(format: ioString("io.a11y.polygon"), Int64(first.data.number))
        case let .text(text, _, _):
            return text.isEmpty ? ioString("io.a11y.label.empty") : String(format: ioString("io.a11y.label"), text.replacingOccurrences(of: "\n", with: " "))
        }
    }

    private func dragChanged(_ value: DragGesture.Value) {
        guard !isNavigating, session.isReadable else { return }
        let point = viewport.referencePoint(value.location)
        if dragStart == nil {
            let start = viewport.referencePoint(value.startLocation)
            dragStart = start; selectionAtStart = session.selection
            #if os(macOS)
            extending = !NSEvent.modifierFlags.intersection([.shift, .command]).isEmpty
            #else
            extending = false
            #endif
            if session.tool == .polygon { drawingTool = .polygon; return }
            let hit = session.document.shapes.reversed().first { $0.path(in: viewport.reference).contains(start) }
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
        if drawingTool == .select {
            let hit = session.document.shapes.contains { $0.path(in: viewport.reference).contains(start) }
            if hit {
                session.preview = session.moved(dx: point.x - start.x, dy: point.y - start.y, finishing: false)
            } else {
                let box = rect(from: start, to: point); selectionBox = box
                var selected = extending ? selectionAtStart : []
                for item in session.document.items {
                    if let bounds = session.document.bounds(of: Set(item), in: viewport.reference), bounds.intersects(box) {
                        selected.formUnion(item)
                    }
                }
                session.selection = selected
            }
        } else if (drawingTool == .rectangle || drawingTool == .ellipse), session.inside(start) {
            let box = rect(from: start, to: point)
            var preview = session.document
            let width = box.width / viewport.reference.width, height = box.height / viewport.reference.height
            preview.shapes.append(.init(.init(number: 0, left: box.minX / viewport.reference.width,
                                              top: box.minY / viewport.reference.height,
                                              kind: drawingTool == .ellipse ? .ellipse(rx: width / 2, ry: height / 2) : .rect(width: width, height: height))))
            session.preview = preview
        }
    }
    private func dragEnded(_ value: DragGesture.Value) {
        defer { dragStart = nil; drawingTool = nil; session.preview = nil; selectionBox = nil }
        guard !isNavigating, let start = dragStart else { return }
        let end = viewport.referencePoint(value.location)
        switch drawingTool {
        case .rectangle: session.addBox(from: start, to: end)
        case .ellipse: session.addBox(from: start, to: end, ellipse: true)
        case .polygon:
            #if os(macOS)
            let distance = 5.0
            #else
            let distance = max(5, 22 / viewport.zoom)
            #endif
            session.addPolygonPoint(start, closingDistance: distance)
        case .text:
            guard session.inside(start) else { return }
            session.addLabel(at: start)
            if let id = session.selection.first { editLabel(id) }
        case .fill:
            if let shape = session.document.shapes.reversed().first(where: { $0.path(in: viewport.reference).contains(start) }) {
                session.fill(shape.id, color: fill)
            }
        case .select:
            if selectionBox == nil { session.commit(session.moved(dx: end.x - start.x, dy: end.y - start.y, finishing: true)) }
        case nil: break
        }
    }
    private func cancelDrag() {
        if dragStart != nil { session.selection = selectionAtStart }
        dragStart = nil; drawingTool = nil; session.preview = nil; selectionBox = nil
    }
    private func rect(from a: CGPoint, to b: CGPoint) -> CGRect {
        .init(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(a.x - b.x), height: abs(a.y - b.y))
    }

    @ViewBuilder private var handles: some View {
        if let box = session.displayed.bounds(of: session.selection, in: viewport.reference) {
            ForEach(ResizeHandle.allCases.filter { !session.uniformResize || $0.isCorner }) { handle in
                Image(systemName: "square.fill")
                    .foregroundStyle(.tint)
                    .padding()
                    .contentShape(Rectangle())
                    .position(viewport.screenPoint(session.handlePoint(handle, in: session.displayed)))
                    .gesture(DragGesture(coordinateSpace: .named("io.canvas"))
                        .onChanged { value in
                            session.preview = session.resized(handle: handle, delta: .init(width: value.translation.width / viewport.zoom, height: value.translation.height / viewport.zoom), finishing: false)
                        }.onEnded { value in
                            session.commit(session.resized(handle: handle, delta: .init(width: value.translation.width / viewport.zoom, height: value.translation.height / viewport.zoom), finishing: true))
                        })
                    .accessibilityLabel(ioString("io.a11y.handle.resize"))
                    .accessibilityIdentifier("io.handle.resize.\(handle.rawValue)")
                    .accessibilityAdjustableAction { direction in
                        let d = direction == .increment ? 5.0 : -5.0
                        session.commit(session.resized(handle: handle, delta: .init(width: d, height: d), finishing: true))
                    }
            }
            if session.canRotate {
                let origin = session.document.shapes.first { session.selection.contains($0.id) }
                    .map { shape in
                        if case .polygon = shape.data.kind { return CGPoint(x: box.midX, y: box.midY) }
                        return shape.unrotatedBounds(in: viewport.reference).origin
                    } ?? box.origin
                let center = viewport.screenPoint(origin)
                Image(systemName: "arrow.trianglehead.clockwise.rotate.90")
                    .foregroundStyle(.tint).padding().contentShape(Rectangle())
                    .position(x: viewport.screenPoint(.init(x: box.midX, y: box.minY)).x,
                              y: viewport.screenPoint(.init(x: box.midX, y: box.minY)).y - 44)
                    .gesture(DragGesture(coordinateSpace: .named("io.canvas"))
                        .onChanged { value in session.preview = session.rotated(degrees: rotation(value, around: center), finishing: false) }
                        .onEnded { value in session.commit(session.rotated(degrees: rotation(value, around: center), finishing: true)) })
                    .accessibilityLabel(ioString("io.a11y.handle.rotate"))
                    .accessibilityIdentifier("io.handle.rotate")
                    .accessibilityAdjustableAction { direction in
                        session.commit(session.rotated(degrees: direction == .increment ? 5 : -5, finishing: true))
                    }
            }
        }
    }
    private func rotation(_ value: DragGesture.Value, around center: CGPoint) -> Double {
        (atan2(value.location.y - center.y, value.location.x - center.x)
         - atan2(value.startLocation.y - center.y, value.startLocation.x - center.x)) * 180 / .pi
    }
}

// These are authored card colours, mandated by the format, not app chrome.
let maskDefaultColor = Color(.sRGB, red: 1, green: 235.0 / 255, blue: 162.0 / 255, opacity: 1)
func maskColor(_ value: String?, fallback: Color) -> Color {
    guard let value else { return fallback }
    if value == "transparent" { return .clear }
    guard value.hasPrefix("#"), value.count == 7, let rgb = UInt32(value.dropFirst(), radix: 16) else { return fallback }
    return Color(.sRGB, red: Double((rgb >> 16) & 255) / 255, green: Double((rgb >> 8) & 255) / 255,
                 blue: Double(rgb & 255) / 255, opacity: 1)
}
