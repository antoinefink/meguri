import CoreBridge
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Native mask canvas. The caller owns image acquisition and the surrounding
/// note workflow. Every completed change writes the binding before returning.
public struct MaskEditorView: View {
    private let image: Image
    private let imageSize: CGSize
    private let isEditing: Bool
    @Binding private var occlusions: String
    @State private var session: MaskEditorSession
    @State private var viewport = CanvasViewport()
    @State private var showInspector = false
    @State private var labelEdit: LabelEdit?
    @State private var fill = Color(.sRGB, red: 1, green: 235.0 / 255, blue: 162.0 / 255, opacity: 1)
    @State private var showFill = false
    @AppStorage("imageOcclusion.translucent") private var translucent = false
    @Environment(\.displayScale) private var displayScale
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @FocusState private var canvasFocused: Bool

    public init(image: Image, imageSize: CGSize, occlusions: Binding<String>, isEditing: Bool) {
        self.init(image: image, imageSize: imageSize, occlusions: occlusions, isEditing: isEditing, session: nil)
    }

    /// Use an externally retained session when switching between fields and
    /// masks removes the view. A new image needs a new session/reference size.
    public init(image: Image, imageSize: CGSize, occlusions: Binding<String>, isEditing: Bool,
                session: MaskEditorSession?) {
        self.image = image; self.imageSize = imageSize; self._occlusions = occlusions; self.isEditing = isEditing
        _session = State(initialValue: session ?? MaskEditorSession(field: occlusions.wrappedValue, isEditing: isEditing))
    }

    public var body: some View {
        VStack(spacing: 0) {
            if !compact { toolRow; Divider() }
            if let key = session.errorKey {
                Label(ioString(key), systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.primary).padding()
                    .accessibilityIdentifier("io.editor.error")
            }
            if session.isReadable {
                GeometryReader { geometry in
                    MaskCanvas(image: image, session: session, viewport: $viewport,
                               translucent: translucent, fill: selectedFill, editLabel: beginLabelEdit)
                        .focusable().focused($canvasFocused)
                        .onKeyPress(phases: .down, action: keyPressed)
                        .onAppear { resize(geometry.size) }
                        .onChange(of: geometry.size) { _, size in resize(size) }
                }
            } else { Spacer() }
            if compact { Divider(); toolRow }
        }
        .onAppear {
            let binding = $occlusions
            session.onWrite = { binding.wrappedValue = $0 }
            session.receive(occlusions, isEditing: isEditing)
        }
        .onDisappear { session.polygon = []; session.preview = nil; session.onWrite = nil }
        .onChange(of: occlusions) { _, field in session.receive(field, isEditing: isEditing) }
        .inspector(isPresented: $showInspector) { inspector }
        .sheet(item: $labelEdit, onDismiss: { canvasFocused = true }) { edit in
            TextLabelEditor(initial: edit.text, reject: session.rejectLabelBraces) { text in
                session.editLabel(edit.id, text: text)
            }
        }
        .accessibilityIdentifier("io.editor")
    }

    private var compact: Bool {
        #if os(iOS)
        sizeClass == .compact
        #else
        false
        #endif
    }
    private var selectedFill: String {
        #if os(macOS)
        let source = NSColor(fill).cgColor
        #else
        let source = UIColor(fill).cgColor
        #endif
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let converted = source.converted(to: space, intent: .defaultIntent, options: nil),
              let channels = converted.components, channels.count >= 3 else { return "#ffeba2" }
        let components = Array(channels.prefix(3))
        return "#" + components.map { String(format: "%02x", Int((min(1, max(0, $0)) * 255).rounded(.toNearestOrAwayFromZero))) }.joined()
    }

    private var toolRow: some View {
        ViewThatFits(in: .horizontal) {
            HStack {
                toolPicker
                modeMenu
                historyButtons
                actionsMenu
                zoomMenu
                inspectorButton
            }
            HStack {
                Menu { toolChoices } label: { Label(ioString(session.tool.key), systemImage: session.tool.symbol) }
                modeMenu
                Spacer()
                historyButtons
                actionsMenu
                zoomMenu
                inspectorButton
            }
        }
        .padding()
        .disabled(!session.isReadable)
    }
    private var toolPicker: some View {
        Picker(ioString("io.tools"), selection: Binding(get: { session.tool }, set: chooseTool)) {
            ForEach(MaskEditorTool.allCases) { tool in
                Label(ioString(tool.key), systemImage: tool.symbol).tag(tool)
                    .accessibilityIdentifier("io.tool.\(tool.rawValue)")
            }
        }
        .pickerStyle(.segmented).labelsHidden()
    }
    private var toolChoices: some View {
        ForEach(MaskEditorTool.allCases) { tool in
            Button { chooseTool(tool) } label: { Label(ioString(tool.key), systemImage: tool.symbol) }
                .accessibilityIdentifier("io.tool.\(tool.rawValue)")
        }
    }
    private var modeMenu: some View {
        Menu {
            Picker(ioString("io.command.mode"), selection: Binding(get: { session.document.hideAll }, set: session.setMode)) {
                Text(ioString("io.mode.hideAll")).tag(true)
                Text(ioString("io.mode.hideOne")).tag(false)
            }
        } label: { Label(ioString("io.command.mode"), systemImage: "square.on.square") }
        .accessibilityValue(ioString(session.document.hideAll ? "io.mode.hideAll" : "io.mode.hideOne"))
        .accessibilityIdentifier("io.mode")
        .labelStyle(.iconOnly)
    }
    private var historyButtons: some View {
        HStack {
            Button(action: session.undo) { Label(ioString("io.command.undo"), systemImage: "arrow.uturn.backward") }
                .disabled(!session.canUndo).accessibilityIdentifier("io.command.undo")
            Button(action: session.redo) { Label(ioString("io.command.redo"), systemImage: "arrow.uturn.forward") }
                .disabled(!session.canRedo).accessibilityIdentifier("io.command.redo")
        }.labelStyle(.iconOnly)
    }
    private var actionsMenu: some View {
        Menu {
            Button(ioString("io.command.selectAll"), action: session.selectAll).disabled(session.document.shapes.isEmpty)
            Button(ioString("io.command.duplicate"), action: session.duplicate).disabled(session.selection.isEmpty)
            Button(ioString("io.command.group"), action: session.group).disabled(!session.canGroup)
            Button(ioString("io.command.ungroup"), action: session.ungroup).disabled(!session.canUngroup)
            Menu(ioString("io.command.align")) {
                ForEach(MaskAlignment.allCases, id: \.self) { alignment in
                    Button(ioString(alignmentKey(alignment))) { session.align(alignment) }
                }
            }.disabled(session.selection.isEmpty)
            Button(ioString("io.command.closePolygon"), action: session.closePolygon).disabled(session.polygon.count < 3)
            Toggle(ioString("io.command.translucent"), isOn: $translucent)
            Divider()
            Button(ioString("io.command.delete"), role: .destructive, action: session.deleteSelection).disabled(session.selection.isEmpty)
        } label: { Label(ioString("io.commands"), systemImage: "ellipsis.circle") }
        .accessibilityIdentifier("io.commands")
        .labelStyle(.iconOnly)
    }
    private var zoomMenu: some View {
        Menu {
            Button(ioString("io.command.zoomIn")) { zoom(1.1) }
            Button(ioString("io.command.zoomOut")) { zoom(1 / 1.1) }
            Button(ioString("io.command.zoomToFit")) { viewport.fit() }
        } label: { Text(viewport.zoom, format: .percent.precision(.fractionLength(0))) }
        .accessibilityLabel(ioString("io.zoom"))
        .accessibilityIdentifier("io.zoom")
    }
    private var inspectorButton: some View {
        Button { showInspector.toggle() } label: { Label(ioString("io.inspector"), systemImage: "sidebar.trailing") }
            .labelStyle(.iconOnly).accessibilityIdentifier("io.inspector.toggle")
            .popover(isPresented: $showFill) {
                ColorPicker(ioString("io.tool.fill"), selection: $fill, supportsOpacity: false).padding()
            }
    }
    private var inspector: some View {
        Form {
            Section(ioString("io.selection")) {
                Text(String(format: ioString("io.selection.count"), Int64(session.selection.count)))
                if let shape = session.document.shapes.first(where: { session.selection.contains($0.id) }) {
                    if session.selection.count == 1 {
                        if case .text = shape.data.kind {
                            Button(ioString("io.label.edit")) { beginLabelEdit(shape.id) }
                        }
                        if session.canRotate {
                            Stepper(value: Binding(get: { shape.data.angle }, set: { value in
                                session.commit(session.rotated(degrees: value - shape.data.angle, finishing: true))
                            }), step: 1) { Text(ioString("io.a11y.handle.rotate")) }
                        }
                    }
                    Button(ioString("io.move.left")) { session.commit(session.moved(dx: -5, dy: 0, finishing: true)) }
                    Button(ioString("io.move.right")) { session.commit(session.moved(dx: 5, dy: 0, finishing: true)) }
                    Button(ioString("io.move.up")) { session.commit(session.moved(dx: 0, dy: -5, finishing: true)) }
                    Button(ioString("io.move.down")) { session.commit(session.moved(dx: 0, dy: 5, finishing: true)) }
                }
            }
            Section(ioString("io.tool.fill")) {
                ColorPicker(ioString("io.tool.fill"), selection: $fill, supportsOpacity: false)
            }
        }.formStyle(.grouped)
    }

    private func chooseTool(_ tool: MaskEditorTool) {
        session.tool = tool
        if tool == .fill { showFill = true }
        canvasFocused = true
    }
    private func resize(_ size: CGSize) {
        session.setReferenceSizeIfNeeded(imageSize: imageSize, area: size, displayScale: displayScale)
        viewport.reference = session.referenceSize; viewport.area = size; viewport.fit()
    }
    private func zoom(_ factor: Double) {
        viewport.magnify(by: factor, at: .init(x: viewport.area.width / 2, y: viewport.area.height / 2))
    }
    private func beginLabelEdit(_ id: UUID) {
        guard let shape = session.document.shapes.first(where: { $0.id == id }),
              case let .text(text, _, _) = shape.data.kind else { return }
        labelEdit = .init(id: id, text: text)
    }
    private func alignmentKey(_ alignment: MaskAlignment) -> String {
        "io.command.align" + alignment.rawValue.prefix(1).uppercased() + alignment.rawValue.dropFirst()
    }
    private func keyPressed(_ press: KeyPress) -> KeyPress.Result {
        guard canvasFocused, labelEdit == nil, !showFill, session.isReadable else { return .ignored }
        let command = press.modifiers.contains(.command), shift = press.modifiers.contains(.shift)
        if press.key == .escape, !session.polygon.isEmpty { session.polygon = []; return .handled }
        if press.key == .return, session.polygon.count >= 3 { session.closePolygon(); return .handled }
        if press.key == .delete || press.key == .deleteForward {
            guard !session.selection.isEmpty else { return .ignored }; session.deleteSelection(); return .handled
        }
        let key = press.characters.lowercased()
        if command {
            switch key {
            case "z":
                if shift { guard session.canRedo else { return .ignored }; session.redo() }
                else { guard session.canUndo else { return .ignored }; session.undo() }
            case "y": guard session.canRedo else { return .ignored }; session.redo()
            case "a": session.selectAll()
            default: return .ignored
            }
            return .handled
        }
        guard press.modifiers.subtracting(.shift).isEmpty else { return .ignored }
        if shift {
            let mapping: [String: MaskAlignment] = ["l": .left, "h": .center, "r": .right, "t": .top, "v": .middle, "b": .bottom]
            guard let alignment = mapping[key], !session.selection.isEmpty else { return .ignored }
            session.align(alignment); return .handled
        }
        switch key {
        case "s": chooseTool(.select)
        case "r": chooseTool(.rectangle)
        case "e": chooseTool(.ellipse)
        case "p": chooseTool(.polygon)
        case "t": chooseTool(.text)
        case "c": chooseTool(.fill)
        case "[": zoom(1 / 1.1)
        case "]": zoom(1.1)
        case "f": viewport.fit()
        case "l": translucent.toggle()
        case "d": session.duplicate()
        case "g": session.group()
        case "u": session.ungroup()
        case "a": session.selectAll()
        case "m": break
        default: return .ignored
        }
        return .handled
    }
}

private struct LabelEdit: Identifiable { let id: UUID; let text: String }

func ioString(_ key: String) -> String { String(localized: String.LocalizationValue(key), bundle: .module) }
