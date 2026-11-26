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
    @State private var interaction = MaskCanvasInteraction()
    @State private var pointer: CGPoint?
    @GestureState private var pointerGestureActive = false
    @GestureState private var handleGestureActive = false

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
                if let selectionBox = interaction.selectionBox {
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
                    interaction.navigationStarted(session: session)
                    if factor != 1 { viewport.magnify(by: factor, at: anchor) }
                    viewport.pan.width += delta.width; viewport.pan.height += delta.height
                },
                ended: { interaction.navigationEnded() },
                panStarted: { interaction.navigationStarted(session: session) }
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
        .gesture(DragGesture(minimumDistance: 0)
            .updating($pointerGestureActive) { _, active, _ in active = true }
            .onChanged(dragChanged).onEnded(dragEnded))
        .onChange(of: pointerGestureActive) { _, active in
            if !active { interaction.pointerGestureEnded(session: session) }
        }
        .onChange(of: handleGestureActive) { _, active in
            if !active { interaction.handleGestureEnded(session: session) }
        }
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
                    if session.canGroup(including: item.ids) {
                        Button(ioString("io.command.group")) {
                            session.group(including: item.ids)
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
        #if os(macOS)
        let extending = !NSEvent.modifierFlags.intersection([.shift, .command]).isEmpty
        #else
        let extending = false
        #endif
        interaction.changed(start: viewport.referencePoint(value.startLocation),
                            point: viewport.referencePoint(value.location), extending: extending, session: session)
    }
    private func dragEnded(_ value: DragGesture.Value) {
        #if os(macOS)
        let closingDistance = 5.0
        #else
        let closingDistance = max(5, 22 / viewport.zoom)
        #endif
        if let label = interaction.ended(point: viewport.referencePoint(value.location), fill: fill,
                                         closingDistance: closingDistance, session: session) { editLabel(label) }
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
                        .updating($handleGestureActive) { _, active, _ in active = true }
                        .onChanged { value in
                            guard handleGestureActive else { return }
                            interaction.resize(handle, delta: .init(width: value.translation.width / viewport.zoom, height: value.translation.height / viewport.zoom), finishing: false, session: session)
                        }.onEnded { value in
                            interaction.resize(handle, delta: .init(width: value.translation.width / viewport.zoom, height: value.translation.height / viewport.zoom), finishing: true, session: session)
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
                        .updating($handleGestureActive) { _, active, _ in active = true }
                        .onChanged { value in
                            guard handleGestureActive else { return }
                            interaction.rotate(degrees: rotation(value, around: center), finishing: false, session: session)
                        }
                        .onEnded { value in
                            interaction.rotate(degrees: rotation(value, around: center), finishing: true, session: session)
                        })
                    .accessibilityLabel(ioString("io.a11y.handle.rotate"))
                    .accessibilityIdentifier("io.handle.rotate")
                    .accessibilityAdjustableAction { direction in
                        session.rotateStep(increment: direction == .increment)
                    }
            }
        }
    }
    private func rotation(_ value: DragGesture.Value, around center: CGPoint) -> Double {
        (atan2(value.location.y - center.y, value.location.x - center.x)
         - atan2(value.startLocation.y - center.y, value.startLocation.x - center.x)) * 180 / .pi
    }
}
