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
    @State private var handleDragActive = false
    @State private var handleDragCancelled = false

    var body: some View {
        ZStack(alignment: .topLeading) {
            // A real Image supplies the fitted image's native AX frame. It does
            // not take pointer/touch input from the canvas gesture beneath it.
            image.resizable()
                .frame(width: viewport.imageRect.width, height: viewport.imageRect.height)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(ioString("io.a11y.canvas"))
                .accessibilityIdentifier("io.editor.canvas")
                .position(x: viewport.imageRect.midX, y: viewport.imageRect.midY)
                .allowsHitTesting(false)
            Canvas { context, _ in
                context.translateBy(x: viewport.origin.x, y: viewport.origin.y)
                context.scaleBy(x: viewport.zoom, y: viewport.zoom)
                for shape in session.displayed.shapes { draw(shape, in: &context) }
                drawPolygon(in: &context)
                if let selectionBox {
                    context.stroke(Path(selectionBox), with: .color(.accentColor), style: .init(lineWidth: 1 / viewport.zoom, dash: [4 / viewport.zoom]))
                }
                if let box = session.displayed.bounds(of: session.selection, in: viewport.reference) {
                    context.stroke(Path(box), with: .color(.accentColor), lineWidth: 1 / viewport.zoom)
                }
            }
            .accessibilityHidden(true)
            accessibleItems
            handles
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
            .accessibilityHidden(true)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .contentShape(Rectangle())
        .onContinuousHover { phase in
            switch phase {
            case let .active(location): pointer = viewport.referencePoint(location)
            case .ended: pointer = nil
            }
        }
        .coordinateSpace(name: "io.canvas")
        .gesture(DragGesture(minimumDistance: 0).onChanged(dragChanged).onEnded(dragEnded))
        .clipped()
        .accessibilityElement(children: .contain)
        .accessibilityLabel(ioString("io.a11y.canvas"))
        .accessibilityIdentifier("io.canvas")
    }

    // These are layout-backed controls, rather than virtual accessibilityChildren
    // whose position has no relationship to the Canvas's native screen frame.
    private var accessibleItems: some View {
        ZStack(alignment: .topLeading) {
            ForEach(session.displayed.accessibilityItems(in: viewport), id: \.identifier) { item in
                Button { session.select(item.first.id) } label: {
                    Rectangle().fill(.clear)
                        .frame(width: item.frame.width, height: item.frame.height)
                }
                .buttonStyle(.plain)
                .frame(width: item.frame.width, height: item.frame.height)
                .accessibilityElement(children: .ignore)
                .accessibilityRespondsToUserInteraction(true)
                .accessibilityLabel(itemLabel(item.ids, first: item.first))
                .accessibilityIdentifier(item.identifier)
                .accessibilityAddTraits(session.selection.contains(item.first.id) ? [.isSelected] : [])
                .accessibilityActions {
                    Button(ioString("io.command.delete")) {
                        session.select(item.first.id); session.deleteSelection()
                    }
                    Button(ioString("io.command.duplicate")) {
                        session.select(item.first.id); session.duplicate()
                    }
                    if canGroup(with: item) {
                        Button(ioString("io.command.group")) {
                            session.selection.formUnion(item.ids); session.group()
                        }
                    }
                    if item.ids.count > 1 {
                        Button(ioString("io.command.ungroup")) {
                            session.select(item.first.id); session.ungroup()
                        }
                    }
                    ForEach(MaskAlignment.allCases, id: \.self) { alignment in
                        Button(ioString(alignment.key)) {
                            session.select(item.first.id); session.align(alignment)
                        }
                    }
                }
                .accessibilityAdjustableAction { direction in
                    session.select(item.first.id)
                    let delta = direction == .increment ? 5.0 : -5.0
                    session.commit(session.moved(dx: delta, dy: 0, finishing: true))
                }
                .position(x: item.frame.midX, y: item.frame.midY)
            }
            if let frame = viewport.polygonFrame(session.polygon) {
                Button { if session.polygon.count >= 3 { session.closePolygon() } } label: {
                    Rectangle().fill(.clear).frame(width: frame.width, height: frame.height)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(String(format: ioString("io.a11y.polygon.unfinished"), Int64(session.polygon.count)))
                .accessibilityIdentifier("io.polygon.unfinished")
                .accessibilityActions {
                    if session.polygon.count >= 3 {
                        Button(ioString("io.command.closePolygon")) { session.closePolygon() }
                    }
                }
                .position(x: frame.midX, y: frame.midY)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .allowsHitTesting(false)
    }

    private func canGroup(with item: OcclusionAccessibilityItem) -> Bool {
        let ids = session.selection.union(item.ids)
        return Set(session.document.shapes.filter { ids.contains($0.id) && !$0.isLabel }.map { $0.data.number }).count > 1
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
            layer.fill(path, with: .color(maskColor(shape.data.fill, fallback: MaskAppearance.defaultColor)))
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
        if handleDragActive { handleDragCancelled = true }
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
                            guard beginHandleDrag() else { return }
                            session.preview = session.resized(handle: handle, delta: .init(width: value.translation.width / viewport.zoom, height: value.translation.height / viewport.zoom), finishing: false)
                        }.onEnded { value in
                            defer { endHandleDrag() }
                            guard !handleDragCancelled, !isNavigating else { return }
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
                        .onChanged { value in
                            guard beginHandleDrag() else { return }
                            session.preview = session.rotated(degrees: rotation(value, around: center), finishing: false)
                        }
                        .onEnded { value in
                            defer { endHandleDrag() }
                            guard !handleDragCancelled, !isNavigating else { return }
                            session.commit(session.rotated(degrees: rotation(value, around: center), finishing: true))
                        })
                    .accessibilityLabel(ioString("io.a11y.handle.rotate"))
                    .accessibilityIdentifier("io.handle.rotate")
                    .accessibilityAdjustableAction { direction in
                        session.commit(session.rotated(degrees: direction == .increment ? 5 : -5, finishing: true))
                    }
            }
        }
    }
    private func beginHandleDrag() -> Bool {
        if !handleDragActive { handleDragActive = true; handleDragCancelled = isNavigating }
        return !handleDragCancelled && !isNavigating
    }
    private func endHandleDrag() {
        handleDragActive = false; handleDragCancelled = false; session.preview = nil
    }
    private func rotation(_ value: DragGesture.Value, around center: CGPoint) -> Double {
        (atan2(value.location.y - center.y, value.location.x - center.x)
         - atan2(value.startLocation.y - center.y, value.startLocation.x - center.x)) * 180 / .pi
    }
}
