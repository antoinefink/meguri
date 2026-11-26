import CoreBridge
import SwiftUI
#if os(macOS)
import AppKit
import WebKit
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
    @State private var retainedSession: MaskEditorSession?
    private let suppliedSession: MaskEditorSession?
    private let formStyle: Bool
    public init(image: Image, imageSize: CGSize, occlusions: Binding<String>, isEditing: Bool) {
        self.init(image: image, imageSize: imageSize, occlusions: occlusions, isEditing: isEditing, session: nil)
    }

    /// Use an externally retained session when switching between fields and
    /// masks removes the view. A new image needs a new session/reference size.
    /// `formStyle` opts macOS Add/Edit into the inline Option B toolbar/canvas;
    /// other callers and platforms retain their existing layout.
    public init(image: Image, imageSize: CGSize, occlusions: Binding<String>, isEditing: Bool,
                session: MaskEditorSession?, formStyle: Bool = false) {
        self.image = image; self.imageSize = imageSize; self._occlusions = occlusions; self.isEditing = isEditing
        suppliedSession = session
        self.formStyle = formStyle
    }

    public var body: some View {
        Group {
            if let session = suppliedSession ?? retainedSession {
                MaskEditorContent(image: image, imageSize: imageSize, isEditing: isEditing,
                                  occlusions: $occlusions, session: session, formStyle: formStyle)
                    .id(ObjectIdentifier(session))
            } else {
                ProgressView()
            }
        }
        .onAppear {
            if suppliedSession == nil && retainedSession == nil {
                retainedSession = MaskEditorSession(field: occlusions, isEditing: isEditing)
            }
        }
    }
}

private struct MaskEditorContent: View {
    let image: Image
    let imageSize: CGSize
    let isEditing: Bool
    @Binding var occlusions: String
    @Bindable var session: MaskEditorSession
    let formStyle: Bool
    @State private var viewport = CanvasViewport()
    @State private var showInspector = false
    @State private var labelEdit: LabelEdit?
    @State private var fill = MaskAppearance.defaultColor
    @State private var showFill = false
    @AppStorage("imageOcclusion.translucent") private var translucent = false
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.displayScale) private var displayScale
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var sizeClass
    #endif
    @FocusState private var canvasFocused: Bool

    #if os(macOS)
    @State private var windowScope = MaskEditorWindowScope()
    #endif

    var body: some View {
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
                               translucent: translucent, fill: selectedFill, editLabel: beginLabelEdit,
                               focusCanvas: { canvasFocused = true }, formStyle: usesFormStyle,
                               acceptsKeyboardNavigation: { !usesFormStyle || (canvasFocused && commandsAvailable) })
                        #if os(macOS)
                        // A focusable SwiftUI ancestor replaces the canvas AX
                        // subtree with a leaf. Keep keyboard focus in a sibling.
                        .background {
                            Color.clear.focusable().focused($canvasFocused)
                                .allowsHitTesting(false).accessibilityHidden(true)
                        }
                        #else
                        .focusable().focused($canvasFocused)
                        #endif
                        .task(id: CanvasLayout(width: geometry.size.width, height: geometry.size.height,
                                              revision: session.imageRevision)) {
                            // Initialization follows layout; a newer geometry or
                            // image cancels the earlier proposal before it locks W/H.
                            await Task.yield()
                            guard !Task.isCancelled else { return }
                            resize(geometry.size)
                        }
                }
            } else { Spacer() }
            if compact { Divider(); toolRow }
        }
        #if os(macOS)
        .background { MaskEditorWindowReader(scope: windowScope) }
        #endif
        .onKeyPress(phases: .down, action: keyPressed)
        .focusedValue(\.maskEditorCommands, commandContext)
        .onAppear {
            if !session.hasIndependentWriter {
                let binding = $occlusions
                session.onWrite = { binding.wrappedValue = $0 }
            }
            session.receive(occlusions, isEditing: isEditing)
        }
        .onDisappear {
            session.polygon = []; session.preview = nil
            if !session.hasIndependentWriter { session.onWrite = nil }
        }
        .onChange(of: isEnabled, initial: true) { _, enabled in
            // An inline image field must not take focus from a rich-text field
            // when it mounts or becomes enabled. A canvas interaction owns focus.
            if !usesFormStyle || !enabled { canvasFocused = enabled }
        }
        .onChange(of: occlusions) { _, field in session.receive(field, isEditing: isEditing) }
        .inspector(isPresented: Binding(get: { !usesFormStyle && showInspector }, set: { showInspector = $0 })) { inspector }
        .sheet(item: $labelEdit, onDismiss: { canvasFocused = true }) { edit in
            TextLabelEditor(initial: edit.text, reject: session.rejectLabelBraces) { text in
                session.editLabel(edit.id, text: text)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("io.editor.maskEditor")
    }

    private var compact: Bool {
        #if os(iOS)
        sizeClass == .compact
        #else
        false
        #endif
    }
    private var usesFormStyle: Bool {
        #if os(macOS)
        formStyle
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
              let channels = converted.components, channels.count >= 3 else { return MaskAppearance.defaultFill }
        let components = Array(channels.prefix(3))
        return "#" + components.map { String(format: "%02x", Int((min(1, max(0, $0)) * 255).rounded(.toNearestOrAwayFromZero))) }.joined()
    }

    @ViewBuilder private var toolRow: some View {
        #if os(macOS)
        if usesFormStyle { formToolbar } else { legacyToolRow }
        #else
        legacyToolRow
        #endif
    }
    private var legacyToolRow: some View {
        ViewThatFits(in: .horizontal) {
            HStack {
                toolPicker
                modeMenu
                historyButtons
                actionsMenu
                zoomMenu
                inspectorButton
            }
            VStack {
                HStack {
                    Menu { toolChoices } label: { Label(ioString(session.tool.key), systemImage: session.tool.symbol) }
                    modeMenu
                    Spacer()
                    inspectorButton
                }
                HStack { historyButtons; Spacer(); actionsMenu; zoomMenu }
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
        .pickerStyle(.segmented).labelsHidden().fixedSize(horizontal: true, vertical: false)
    }
    #if os(macOS)
    private var formTools: [MaskEditorTool] { MaskEditorTool.allCases.filter { $0 != .fill } }

    private var formToolbar: some View {
        ViewThatFits(in: .horizontal) {
            HStack {
                formToolPicker
                formFillPicker
                Divider()
                historyButtons
                Spacer()
                formModeMenu
                zoomMenu
                actionsMenu
            }.fixedSize(horizontal: false, vertical: true)
            VStack {
                HStack {
                    formToolPicker
                    formFillPicker
                    Spacer()
                }
                HStack {
                    historyButtons
                    Spacer()
                    formModeMenu
                    zoomMenu
                    actionsMenu
                }
            }
            VStack {
                HStack {
                    Menu {
                        ForEach(formTools) { tool in
                            Button { chooseTool(tool) } label: { Label(ioString(tool.key), systemImage: tool.symbol) }
                                .accessibilityIdentifier("io.tool.\(tool.rawValue)")
                        }
                    } label: { Label(ioString(session.tool.key), systemImage: session.tool.symbol) }
                    formFillPicker
                    Spacer()
                    historyButtons
                    actionsMenu
                }
                HStack { formModeMenu; Spacer(); zoomMenu }
            }
        }
        .padding()
        .disabled(!session.isReadable)
    }
    private var formToolPicker: some View {
        Picker(ioString("io.tools"), selection: Binding(get: { session.tool }, set: chooseTool)) {
            ForEach(formTools) { tool in
                Label(ioString(tool.key), systemImage: tool.symbol).tag(tool)
                    .accessibilityIdentifier("io.tool.\(tool.rawValue)")
            }
        }
        .pickerStyle(.segmented).labelsHidden().labelStyle(.iconOnly)
        .fixedSize(horizontal: true, vertical: false)
    }
    private var formFillPicker: some View {
        HStack {
            // Picking a colour keeps M3.5's explicit click-to-fill behavior:
            // changing the well never recolours the current selection.
            ColorPicker(ioString("io.tool.fill"), selection: $fill, supportsOpacity: false)
                .labelsHidden()
                .accessibilityIdentifier("io.fill")
                .onChange(of: fill) { _, _ in session.tool = .fill }
            resetFillButton
        }
        .fixedSize()
        .popover(isPresented: $showFill) { fillPicker.padding() }
    }
    private var formModeMenu: some View {
        HStack {
            Text(ioString("io.cards"))
            Menu {
                modeChoices
            } label: {
                Text(ioString(session.document.hideAll ? "io.mode.hideAll" : "io.mode.hideOne"))
            }
            .fixedSize()
            .accessibilityLabel(ioString("io.cards"))
            .accessibilityValue(ioString(session.document.hideAll ? "io.mode.hideAll" : "io.mode.hideOne"))
            .accessibilityIdentifier("io.mode")
        }
    }
    #endif
    private var modeChoices: some View {
        Picker(ioString("io.command.mode"), selection: Binding(get: { session.document.hideAll }, set: session.setMode)) {
            Text(ioString("io.mode.hideAll")).tag(true)
            Text(ioString("io.mode.hideOne")).tag(false)
        }
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
                    Button(ioString(alignment.key)) { session.align(alignment) }
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
        } label: {
            if usesFormStyle && viewport.isFitted {
                Text(ioString("io.zoom.fit"))
            } else {
                Text(viewport.zoom, format: .percent.precision(.fractionLength(0)))
            }
        }
        .accessibilityLabel(ioString("io.zoom"))
        .accessibilityValue(Text(viewport.zoom, format: .percent.precision(.fractionLength(0))))
        .accessibilityIdentifier("io.zoom")
    }
    private var inspectorButton: some View {
        Button { showInspector.toggle() } label: { Label(ioString("io.inspector"), systemImage: "sidebar.trailing") }
            .labelStyle(.iconOnly).accessibilityIdentifier("io.inspector.toggle")
            .popover(isPresented: $showFill) {
                fillPicker.padding()
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
                            Stepper {
                                Text(ioString("io.a11y.handle.rotate"))
                            } onIncrement: {
                                session.rotateStep(increment: true)
                            } onDecrement: {
                                session.rotateStep(increment: false)
                            }
                        }
                    }
                    Button(ioString("io.move.left")) { session.commit(session.moved(dx: -5, dy: 0, finishing: true)) }
                    Button(ioString("io.move.right")) { session.commit(session.moved(dx: 5, dy: 0, finishing: true)) }
                    Button(ioString("io.move.up")) { session.commit(session.moved(dx: 0, dy: -5, finishing: true)) }
                    Button(ioString("io.move.down")) { session.commit(session.moved(dx: 0, dy: 5, finishing: true)) }
                }
            }
            Section(ioString("io.tool.fill")) {
                fillPicker
            }
        }.formStyle(.grouped)
            .onKeyPress(phases: .down, action: keyPressed)
            .focusedValue(\.maskEditorCommands, commandContext)
    }

    private var fillPicker: some View {
        VStack(alignment: .leading) {
            ColorPicker(ioString("io.tool.fill"), selection: $fill, supportsOpacity: false)
            resetFillButton
        }
    }
    private var resetFillButton: some View {
        Button {
            fill = MaskAppearance.defaultColor
            if usesFormStyle { session.tool = .fill }
        } label: {
            Image(systemName: "circle.fill").foregroundStyle(MaskAppearance.defaultColor)
        }
        .accessibilityLabel(MaskAppearance.defaultFill)
        .help(ioString("io.fill.reset"))
        .accessibilityIdentifier("io.palette.swatch.ffeba2")
    }

    private func chooseTool(_ tool: MaskEditorTool) {
        session.tool = tool
        if tool == .fill { showFill = true }
        canvasFocused = true
    }
    private func resize(_ size: CGSize) {
        viewport.resize(session: session, imageSize: imageSize, area: size, displayScale: displayScale)
    }
    private func zoom(_ factor: Double) {
        viewport.magnify(by: factor, at: .init(x: viewport.area.width / 2, y: viewport.area.height / 2))
    }
    private func beginLabelEdit(_ id: UUID) {
        guard let shape = session.document.shapes.first(where: { $0.id == id }),
              case let .text(text, _, _) = shape.data.kind else { return }
        labelEdit = .init(id: id, text: text)
    }
    private var commandsAvailable: Bool {
        guard isEnabled, labelEdit == nil, !showFill, session.isReadable else { return false }
        #if os(macOS)
        guard let window = windowScope.window, window.isKeyWindow, window.attachedSheet == nil,
              !(window.firstResponder is NSTextView) else { return false }
        if usesFormStyle {
            guard !(window.firstResponder is NSTextField) else { return false }
            // Rich-text fields use a WebKit descendant as their responder,
            // rather than AppKit's field editor. SwiftUI focus can lag it.
            var view = window.firstResponder as? NSView
            while let current = view {
                if current is WKWebView { return false }
                view = current.superview
            }
        }
        #endif
        return true
    }
    private var commandContext: MaskEditorCommandContext {
        .init(enabled: commandEnabled, action: perform)
    }
    private func commandEnabled(_ command: MaskEditorCommand) -> Bool {
        guard commandsAvailable else { return false }
        switch command {
        case .undo: return session.canUndo
        case .redo: return session.canRedo
        case .selectAll: return !session.document.shapes.isEmpty
        case .delete, .duplicate: return !session.selection.isEmpty
        case .group: return session.canGroup
        case .ungroup: return session.canUngroup
        case .closePolygon: return session.polygon.count >= 3
        default: return command.alignment == nil || !session.selection.isEmpty
        }
    }
    private func perform(_ command: MaskEditorCommand) {
        guard commandEnabled(command) else { return }
        if let tool = command.tool { chooseTool(tool); return }
        if let alignment = command.alignment { session.align(alignment); return }
        switch command {
        case .undo: session.undo()
        case .redo: session.redo()
        case .selectAll: session.selectAll()
        case .delete: session.deleteSelection()
        case .duplicate: session.duplicate()
        case .group: session.group()
        case .ungroup: session.ungroup()
        case .zoomOut: zoom(1 / 1.1)
        case .zoomIn: zoom(1.1)
        case .zoomToFit: viewport.fit()
        case .translucent: translucent.toggle()
        case .closePolygon: session.closePolygon()
        default: break
        }
    }
    private func keyPressed(_ press: KeyPress) -> KeyPress.Result {
        guard commandsAvailable else { return .ignored }
        if usesFormStyle, canvasFocused, press.modifiers.isEmpty, !session.selection.isEmpty {
            let delta: CGSize
            switch press.key {
            case .leftArrow: delta = .init(width: -5, height: 0)
            case .rightArrow: delta = .init(width: 5, height: 0)
            case .upArrow: delta = .init(width: 0, height: -5)
            case .downArrow: delta = .init(width: 0, height: 5)
            default: delta = .zero
            }
            if delta != .zero {
                session.commit(session.moved(dx: delta.width, dy: delta.height, finishing: true))
                return .handled
            }
        }
        if press.key == .escape, !session.polygon.isEmpty { session.polygon = []; return .handled }
        let modifiers = press.modifiers.intersection([.command, .shift, .option, .control])
        if press.characters.lowercased() == "m", modifiers.isEmpty { return .handled }
        let character: String
        switch press.key {
        case .delete: character = "\u{7f}"
        case .deleteForward: character = "\u{f728}"
        case .return: character = "\r"
        default: character = press.characters.lowercased()
        }
        return commandContext.keyPressed(character: character, modifiers: modifiers) ? .handled : .ignored
    }
}

private struct LabelEdit: Identifiable { let id: UUID; let text: String }

func ioString(_ key: String) -> String { String(localized: String.LocalizationValue(key), bundle: .module) }

private struct CanvasLayout: Hashable {
    let width: CGFloat
    let height: CGFloat
    let revision: Int
}
