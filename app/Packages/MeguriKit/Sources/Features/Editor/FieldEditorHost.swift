import AppModel
import CoreBridge
import Foundation
import ImageOcclusion
import Instrumentation
import Observation
import OSLog
import SwiftUI
import WebKit

/// One web view per draft, retained above SwiftUI layout changes. The only
/// data bridge is in a named content world and admits its main frame only.
/// This component deliberately offers no Add/Edit/Browse persistence command.
@Observable @MainActor
public final class FieldEditorHost: NSObject, WKNavigationDelegate, AddNoteFieldRuntime, BrowseFieldRuntime {
    /// P6's device setting; nil is Off. The default is 1024 pixels.
    public var occlusionPhotoMaximumDimension: Int? = 1024
    public let draft: NoteEditorDraft
    public let webView: WKWebView
    public private(set) var editable = false
    public private(set) var held = false
    public private(set) var error: String?
    let occlusionImage = OcclusionImageSession()
    private(set) var maskSession: MaskEditorSession?
    private(set) var occlusionReadingError: String?
    private var readOcclusionField: String?
    private var referenceImageField: String?
    struct ImageLoadKey: Equatable {
        let draft: UUID
        let field: String?
        let retry: UInt64
    }
    var occlusionImageLoadKey: ImageLoadKey {
        .init(draft: draft.identity, field: draft.occlusionRoles.map { draft.values[$0.image] }, retry: occlusionImage.loadRetry)
    }

    /// Called at host creation and field-change boundaries, never from body.
    /// Binding echoes keep the codec, original source and local history intact.
    func refreshMaskSession(startingHideAll: Bool = true) {
        guard let roles = draft.occlusionRoles else { return }
        let field = draft.values[roles.occlusions], image = draft.values[roles.image]
        if field != readOcclusionField {
            if let maskSession { maskSession.receive(field, isEditing: draft.noteID != nil) }
            else {
                maskSession = MaskEditorSession(field: field, isEditing: draft.noteID != nil,
                    startingHideAll: startingHideAll,
                    acceptsWrite: { [weak self] session in
                        guard let self else { return false }
                        return self.maskSession === session && self.draft.occlusionRoles == roles && self.acceptsMaskInput
                    }, write: { [weak self] value in
                        guard let self else { return }
                        self.setHostField(value, at: roles.occlusions)
                        self.occlusionImage.startsNewMasks = false
                    })
            }
            readOcclusionField = field
            let readingError = maskSession?.readingError
            if occlusionReadingError != readingError { occlusionReadingError = readingError }
            if usesOcclusionForm, let shapes = try? OcclusionCodec(field: field).shapes {
                let masks = shapes.filter {
                    if case .text = $0.kind { return false }
                    return $0.number > 0
                }
                occlusionCounts = (masks.count, Set(masks.map(\.number)).count)
            } else { occlusionCounts = nil }
        }
        if let referenceImageField, referenceImageField != image { maskSession?.resetForImageChange() }
        referenceImageField = image
    }

    func drawNewMasks() {
        guard acceptsHostInput, draft.noteID == nil else { return }
        occlusionImage.startsNewMasks = true
        maskSession?.receive("", isEditing: false)
    }
    struct OcclusionFormLayout: Equatable {
        var imageTop = 0.0
        var imageHeight = 400.0
        var tagsTop = 408.0
    }
    private(set) var occlusionFormLayout: OcclusionFormLayout?
    private var occlusionFormViewport = 0.0
    private var occlusionTagsHeight = 28.0
    private(set) var occlusionCounts: (masks: Int, cards: Int)?

    /// Reserve native rows in the retained field page. No field is reloaded or
    /// serialized when the window changes size or focus moves to the canvas.
    func layoutOcclusionForm(viewport: Double, tagsHeight: Double) async {
        occlusionFormViewport = viewport; occlusionTagsHeight = tagsHeight
        guard usesOcclusionForm, editable, let expected = activeLoad else { return }
        do { try await configureOcclusionForm(load: expected) }
        catch {
            if !Task.isCancelled, activeLoad == expected { fail(error, load: expected) }
        }
    }
    private var occlusionFormOptions: [String: Any]? {
        guard usesOcclusionForm, let roles = draft.occlusionRoles else { return nil }
        return ["header": roles.header, "backExtra": roles.backExtra,
            "viewport": occlusionFormViewport, "tagsHeight": occlusionTagsHeight]
    }
    private func configureOcclusionForm(load expected: UInt64) async throws {
        guard var command = occlusionFormOptions else { return }
        command["type"] = "setOcclusionForm"; command["loadId"] = expected
        _ = try await call(command, load: expected)
        try requireCurrent(expected)
    }

    #if os(macOS)
    private final class MaskActivationFocus {
        weak var responder: NSView?
        let event: NSEvent?
        init(responder: NSView, event: NSEvent?) { self.responder = responder; self.event = event }
    }
    @ObservationIgnored private var maskActivationFocus: MaskActivationFocus?
    @ObservationIgnored weak var nativeTagsControl: NSControl?
    #endif
    /// The legacy canvas focuses itself when it becomes enabled. Capture a
    /// text responder before image readiness/release enables it, so decoding
    /// an image in the background cannot interrupt typing in this form.
    func preserveTextFocusForMaskActivation() {
        #if os(macOS)
        guard usesOcclusionForm, let responder = webView.window?.firstResponder as? NSView,
              responder is NSTextView || responder === webView || responder.isDescendant(of: webView) else { return }
        // AppKit's shared field editor can detach when the canvas focuses.
        // Restore its owning token control so editing is attached correctly.
        let target: NSView
        if let control = nativeTagsControl, control.currentEditor() === responder { target = control }
        else { target = responder }
        maskActivationFocus = .init(responder: target, event: NSApp.currentEvent)
        #endif
    }
    func restoreTextFocusAfterMaskActivation() {
        #if os(macOS)
        guard let focus = maskActivationFocus else { return }
        maskActivationFocus = nil
        Task { @MainActor [weak self] in
            await Task.yield()
            guard let self, self.usesOcclusionForm, self.acceptsMaskInput,
                  let responder = focus.responder, let window = self.webView.window,
                  responder.window === window, NSApp.currentEvent === focus.event else { return }
            // A deliberate click/key since capture supersedes this restoration.
            window.makeFirstResponder(responder)
        }
        #endif
    }

    var flushNativeTags: (@MainActor () -> Void)?
    public var acceptsHostInput: Bool { editable && !held && !draft.hostFieldsHeld && !closed && isCurrent() }
    /// O5 protects the hidden image role when an existing note's masks cannot
    /// be read. Metadata input remains available through acceptsHostInput.
    var acceptsOcclusionImageInput: Bool {
        acceptsHostInput && (draft.noteID == nil || occlusionReadingError == nil)
    }
    /// The canvas and its retained writer use the same production admission.
    /// Layout/image readiness alone does not mean the field host is editable.
    var acceptsMaskInput: Bool {
        acceptsOcclusionImageInput && maskEditorVisible && occlusionImage.image != nil &&
            !occlusionImage.busy && !occlusionImage.replacing
    }
    /// Option B applies only to macOS Add/Edit; Browse keeps its legacy switch.
    var usesOcclusionForm: Bool {
        #if os(macOS)
        !browsing && draft.occlusionRoles != nil
        #else
        false
        #endif
    }
    public var maskEditorVisible: Bool {
        if usesOcclusionForm { return true }
        guard draft.occlusionRoles != nil, draft.showsMaskEditor else { return false }
        return draft.noteID == nil || occlusionReadingError == nil
    }
    var ownsDraft: Bool { !closed && isCurrent() }
    func showFields(_ visible: Bool) {
        if !visible {
            flushNativeTags?()
            #if os(macOS)
            if let responder = webView.window?.firstResponder as? NSView,
               responder === webView || responder.isDescendant(of: webView) { webView.window?.makeFirstResponder(nil) }
            #else
            webView.endEditing(true)
            #endif
            fieldFocused = false
        }
        webView.isHidden = !visible
    }
    func setNativeTags(_ tags: [String]) {
        guard acceptsHostInput, tags != draft.tags else { return }
        draft.setTags(tags)
    }
    public func setHostField(_ value: String, at index: Int) {
        guard acceptsHostInput else { return }
        let sameImage = draft.occlusionRoles?.image == index && draft.values[index] == value
        draft.acceptHostField(value, at: index)
        refreshMaskSession()
        if sameImage { maskSession?.resetForImageChange() }
        // Browse uses the same durable patch/undo path as a field blur.
        onFieldBlur?()
    }
    public private(set) var contentHeight: Double = 0
    public private(set) var canUndo = false
    public private(set) var canRedo = false
    /// The native containing form supplies the next/previous focus target.
    public var focusLeave: ((Bool) -> Void)?
    /// Browse's inspector: a field lost the focus, after its commit (V12 saves).
    public var onFieldBlur: (@MainActor () -> Void)?
    /// A field of the page has the keyboard focus now (not only had it).
    public private(set) var fieldFocused = false
    /// Browse's inspector: the page opens a note in its "browse" mode, with
    /// no field focused (`browser/inspector` I3).
    public var browsing = false {
        didSet {
            #if os(macOS)
            (webView as? FieldEditorMacWebView)?.inInspector = browsing
            #endif
        }
    }
    /// Browse's inspector: the collection's media folder, which the page's
    /// thumbnails load from; nil loads none.
    public var mediaDirectory: URL? {
        get { document.mediaDirectory }
        set { document.mediaDirectory = newValue }
    }
    /// Browse's inspector: the caret's line in the page, top and bottom in
    /// points, so the inspector's scroll view keeps it in sight.
    public private(set) var caret: ClosedRange<Double>?
    /// Browse's inspector: a sound chip or a thumbnail was clicked.
    public var onPlayMedia: (@MainActor (String) -> Void)?
    public var onOpenMedia: (@MainActor (String) -> Void)?
    /// The page's thumbnails load from the collection's media folder, a sound
    /// chip plays its file and, on the Mac, a thumbnail opens its file.
    /// Without a folder the page shows no media.
    public func show(media: MediaFolder?) {
        mediaDirectory = media?.directory
        onPlayMedia = { [weak media] in media?.play($0) }
        #if os(macOS)
        onOpenMedia = { [weak media] name in
            if let url = media?.image(name) { NSWorkspace.shared.open(url) }
        }
        #endif
    }
    private let isCurrent: @MainActor () -> Bool
    private let document: EditorDocument
    private let relay: EditorMessageRelay
    private static let world = WKContentWorld.world(name: "Meguri.FieldEditor.v1")
    private static let log = Logger(subsystem: "com.antoinefink.meguri", category: "Editor")
    private var loadID: UInt64 = 0
    private var activeLoad: UInt64?
    private var openedLoad: UInt64?
    private static let maximumID: UInt64 = 9_007_199_254_740_991
    private var nextSequence: UInt64 = 1
    private var pending: [UInt64: Pending] = [:]
    private var draining: Task<Void, Never>?
    private var drainingLoad: UInt64?
    private var startup: Task<Void, any Error>?
    private var ready: CheckedContinuation<Void, any Error>?
    private var readyTimeout: Task<Void, Never>?
    private var gapTimeout: Task<Void, Never>?
    private var interval: IntervalToken?
    private var allowsDocumentNavigation = false
    private var documentNavigation: WKNavigation?
    private var closed = false
    private var commandLoad: UInt64?
    private var processing: Pending?
    private var processTerminated = false
    #if os(macOS)
    // The normal collection writer records the media addition for sync.
    var pictureMedia: (any MediaService)?
    var confirmPictureSize: (@MainActor (String, Int) async -> Bool)?
    private var pictureSizeAlert: NSAlert?
    var picturePasteboard: NSPasteboard = .general
    private var pictureTask: Task<Void, Never>?
    private(set) var pictureFailures: [(name: String, cause: String)] = []
    private var pictureFailureAlert: NSAlert?
    private var pictureUnhideObserver: (any NSObjectProtocol)?
    var picturePasteCompletion: Task<Void, Never>? { pictureTask }
    #endif
    // Tests can join the actual drain after releasing a held Core result.
    var processingCompletion: Task<Void, Never>? { draining }
    @MainActor private final class Pending {
        let message: EditorBridgeMessage
        private var completion: ((Result<Void, any Error>) -> Void)?
        init(message: EditorBridgeMessage, complete: @escaping (Result<Void, any Error>) -> Void) {
            self.message = message; completion = complete
        }
        func settle(_ result: Result<Void, any Error>) {
            let complete = completion; completion = nil; complete?(result)
        }
    }

    /// The page is already loaded and ready: this host continues another's web view.
    private var pageReady = false
    private var firstLoad: UInt64 = 0

    /// N17.1: the next draft in the page the previous host shows. The web view
    /// stays first responder, so on touch the keyboard stays up after an add.
    private init(continuing previous: FieldEditorHost, draft: NoteEditorDraft, isCurrent: @escaping @MainActor () -> Bool) {
        self.draft = draft; self.isCurrent = isCurrent
        document = previous.document; relay = previous.relay; webView = previous.webView
        super.init()
        // O7/M2: seed the blank next canvas without an edit or a history step,
        // before its next image is acquired.
        refreshMaskSession(startingHideAll: previous.maskSession?.hideAll ?? true)
        pageReady = true; loadID = previous.loadID; firstLoad = previous.loadID
        browsing = previous.browsing
        occlusionPhotoMaximumDimension = previous.occlusionPhotoMaximumDimension
        onPlayMedia = previous.onPlayMedia; onOpenMedia = previous.onOpenMedia
        relay.owner = self; webView.navigationDelegate = self
        #if os(iOS)
        (webView as? FieldEditorWebView)?.owner = self
        refreshAccessory()
        #endif
    }
    public func successor(for draft: NoteEditorDraft, isCurrent: @escaping @MainActor () -> Bool) -> (any AddNoteFieldRuntime)? {
        guard editable, !closed, !processTerminated, loadID < Self.maximumID else { return nil }
        // End this host as shutDown() does, but leave the page and its handler alive.
        occlusionImage.cancel()
        closed = true
        if let expected = activeLoad { fail(EditorRuntimeError.unavailable, load: expected) }
        return FieldEditorHost(continuing: self, draft: draft, isCurrent: isCurrent)
    }

    public init(draft: NoteEditorDraft, isCurrent: @escaping @MainActor () -> Bool) throws {
        self.draft = draft; self.isCurrent = isCurrent
        document = try EditorDocument(resources: .module); relay = EditorMessageRelay()
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        configuration.mediaTypesRequiringUserActionForPlayback = .all
        configuration.setURLSchemeHandler(document, forURLScheme: "meguri-editor")
        configuration.userContentController.addScriptMessageHandler(relay, contentWorld: Self.world, name: "meguriEditor")
        configuration.userContentController.addUserScript(WKUserScript(source: document.script, injectionTime: .atDocumentEnd,
            forMainFrameOnly: true, in: Self.world))
        #if os(iOS)
        let view = FieldEditorWebView(frame: .zero, configuration: configuration)
        // The form's grouped background shows through; the page paints only its boxes.
        view.isOpaque = false; view.backgroundColor = .clear; view.scrollView.backgroundColor = .clear
        // With no Done in the keyboard bar, dragging the form down hides the keyboard.
        view.scrollView.keyboardDismissMode = .interactive
        webView = view
        #else
        webView = FieldEditorMacWebView(frame: .zero, configuration: configuration)
        #endif
        super.init()
        refreshMaskSession()
        relay.owner = self; webView.navigationDelegate = self
        #if os(iOS)
        view.owner = self
        refreshAccessory()
        #endif
    }

    // MARK: The focused field's controls (N17.1)

    /// The field the one mode control acts on: the field that has, or last had, focus.
    public var modeField: Int? {
        guard editable, let ord = draft.focusedField, draft.fields.indices.contains(ord),
              (browsing || !draft.fields[ord].collapsed), !draft.hiddenFields.contains(ord),
              !draft.hostFields.contains(ord), (usesOcclusionForm || !(draft.occlusionRoles != nil && draft.showsMaskEditor)) else { return nil }
        return ord
    }
    public var focusedMode: NoteEditorDraft.Mode? { modeField.map { draft.fields[$0].mode } }
    /// F1's identifier for the mode button of the field it acts on.
    public var modeControlIdentifier: String {
        modeField.map { "editor.field.\($0).htmlToggle" } ?? "editor.add.htmlToggle"
    }
    public func toggleFocusedMode() {
        guard !held, let ord = modeField else { return }
        let mode: NoteEditorDraft.Mode = draft.fields[ord].mode == .html ? .rich : .html
        Task { try? await setMode(mode, ord: ord) }
    }
    /// The next or previous expanded field, as Tab moves (F4), without leaving the page.
    func adjacentField(previous: Bool) -> Int? {
        let shown = draft.fields.indices.filter { (browsing || !draft.fields[$0].collapsed) && !draft.hiddenFields.contains($0) && !draft.hostFields.contains($0) }
        guard let current = draft.focusedField, let index = shown.firstIndex(of: current) else { return nil }
        let target = index + (previous ? -1 : 1)
        return shown.indices.contains(target) ? shown[target] : nil
    }
    func moveFocus(previous: Bool) {
        guard !held, let ord = adjacentField(previous: previous) else { return }
        Task { try? await setFocus(ord) }
    }

    #if os(iOS)
    private func refreshAccessory() {
        guard let view = webView as? FieldEditorWebView, view.owner === self else { return }
        view.modeItem.isEnabled = modeField != nil && !held
        view.modeItem.isSelected = focusedMode == .html
        view.modeItem.accessibilityIdentifier = modeControlIdentifier
        view.previousItem.isEnabled = adjacentField(previous: true) != nil
        view.nextItem.isEnabled = adjacentField(previous: false) != nil
        for item in view.formatItems { item.isEnabled = canFormat }
        view.clozeItem.isHidden = !showsCloze
        view.clozeItem.isEnabled = canCloze
    }
    #else
    private func refreshAccessory() {}
    #endif

    public func start() async throws {
        if let startup, let expected = activeLoad {
            try requireCurrent(expected)
            try await startup.value
            try requireCurrent(expected)
            return
        }
        // Only explicit process recovery may replace a failed load.
        guard loadID == firstLoad else { throw EditorRuntimeError.unavailable }
        try await startLoad()
    }

    private func startLoad() async throws {
        guard !closed, isCurrent() else { throw EditorRuntimeError.unavailable }
        guard loadID < Self.maximumID else { throw EditorRuntimeError.unavailable }
        loadID += 1
        let expected = loadID
        activeLoad = expected; nextSequence = 1
        let work = Task { [self] in
            try requireCurrent(expected)
            interval = Signposts.begin(IntervalNames.editorOpenToEditable)
            if !pageReady {
                try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                    ready = continuation; allowsDocumentNavigation = true
                    readyTimeout = Task { [weak self] in
                        do { try await Task.sleep(for: .seconds(10)) } catch { return }
                        self?.fail(EditorRuntimeError.unavailable, load: expected)
                    }
                    documentNavigation = webView.load(URLRequest(url: EditorDocument.url))
                }
            }
            try requireCurrent(expected)
            try await openDraft(load: expected)
        }
        startup = work
        do { try await work.value; try requireCurrent(expected) }
        catch { fail(error, load: expected); throw error }
    }

    private func openDraft(load expected: UInt64) async throws {
        try requireCurrent(expected)
        let display = try await draft.displayValues()
        try requireCurrent(expected)
        let fields: [[String: Any]] = draft.fields.enumerated().filter { !draft.hostFields.contains($0.offset) }.map { index, field in
            var family = field.definition.fontName
            if family.hasSuffix(" L") { family.removeLast(2); family += " Light" }
            if !family.isEmpty && family.range(of: #"^[a-z-]+$"#, options: .regularExpression) == nil {
                // A CSS string token, assigned to a property rather than parsed
                // as a stylesheet. Preserve quotes and slashes in font names.
                family = "\"" + family.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
            }
            if !family.isEmpty { family += ", " }
            family += "-apple-system"
            var value: [String: Any] = ["ord": index, "name": field.definition.name,
                "html": display.targets[index], "description": field.definition.description,
                "fontFamily": family, "fontSize": field.definition.fontSize == 0 ? 20 : field.definition.fontSize,
                "rtl": field.definition.rtl, "collapsed": browsing ? false : field.collapsed, "mode": field.mode.rawValue,
                "clozeField": draft.notetype.clozeFields.contains(UInt32(index))]
            if field.changed { value["baselineHtml"] = display.baselines[index] }
            if draft.hiddenFields.contains(index) { value["hidden"] = true }
            return value
        }
        let shownFocus = draft.focusedField.flatMap { draft.hostFields.contains($0) ? nil : $0 }
        let defaultFocus = usesOcclusionForm ? draft.occlusionRoles?.header : nil
        let focus = !browsing && (usesOcclusionForm || !(draft.occlusionRoles != nil && draft.showsMaskEditor)) ? shownFocus ?? defaultFocus ?? draft.fields.indices.first(where: {
            !draft.fields[$0].collapsed && !draft.hiddenFields.contains($0) && !draft.hostFields.contains($0)
        }) : nil
        #if os(macOS)
        (webView as? FieldEditorMacWebView)?.inForm = usesOcclusionForm
        #endif
        var options: [String: Any] = ["closeHTMLTags": false, "shrinkImages": false]
        if let occlusionFormOptions { options["occlusionForm"] = occlusionFormOptions }
        openedLoad = expected
        let mode = browsing ? "browse" : draft.noteID == nil ? "add" : "edit"
        _ = try await call(["type": "openNote", "protocol": 1, "loadId": expected, "mode": mode, "fields": fields,
            "focus": focus.map { $0 as Any } ?? NSNull(), "theme": [String: String](),
            "options": options], load: expected)
        try requireCurrent(expected)
        guard editable else { throw EditorRuntimeError.unavailable }
        held = false
        draft.hostFieldsHeld = false
    }

    /// Resolve all earlier sequence numbers before returning the flushed draft
    /// targets. A caller must not submit a write before this returns.
    public func flush(hold: Bool) async throws {
        #if os(macOS)
        if hold { cancelPictureTask() }
        #endif
        // Browse's inspector flushes for a save, a cloze and a reload: a flush
        // waits for the one under way rather than fail (no input is skipped).
        while browsing, editable, commandLoad != nil { try await Task.sleep(for: .milliseconds(5)) }
        // A pending source has not committed the Image host field. Saving the
        // previous image here would silently discard the user's acquisition.
        guard !occlusionImage.busy else {
            throw BridgeError.unavailable(reason: "Finish choosing the image, or cancel it, before continuing.")
        }
        guard editable, let expected = activeLoad, commandLoad == nil else { throw EditorRuntimeError.unavailable }
        try requireCurrent(expected)
        flushNativeTags?()
        if hold { draft.hostFieldsHeld = true }
        commandLoad = expected
        defer { if commandLoad == expected { commandLoad = nil } }
        do {
            let result = try await call(["type": "flush", "loadId": expected, "hold": hold], load: expected)
            try requireCurrent(expected)
            guard let result else { throw EditorRuntimeError.invalidMessage }
            let message = try EditorBridgeMessage(result, ordinals: Set(draft.fields.indices))
            guard message.type == "flush", message.loadID == expected else { throw EditorRuntimeError.invalidMessage }
            try await enqueue(message, load: expected)
            try requireCurrent(expected)
            held = hold || held
        } catch { fail(error, load: expected); throw error }
    }

    public func release() async throws {
        guard editable, let expected = activeLoad else { throw EditorRuntimeError.unavailable }
        try requireCurrent(expected)
        // A refused flush never held the page. Avoid publishing a spurious
        // input-state change that would cancel the still-current source.
        guard held || draft.hostFieldsHeld else { return }
        _ = try await call(["type": "release", "loadId": expected], load: expected)
        try requireCurrent(expected)
        preserveTextFocusForMaskActivation()
        held = false; draft.hostFieldsHeld = false
    }
    /// Edit mode: the page loads again the fields the draft took from the
    /// stored note after a change made elsewhere (B3 `setField`).
    public func reload(fields ords: [Int]) async throws {
        guard editable, let expected = activeLoad else { throw EditorRuntimeError.unavailable }
        refreshMaskSession()
        occlusionImage.retryFailedLoad()
        for ord in ords where !draft.hostFields.contains(ord) {
            let html = try await draft.displayValue(ord)
            try requireCurrent(expected)
            _ = try await call(["type": "setField", "loadId": expected, "ord": ord, "html": html], load: expected)
        }
    }
    public func setFocus(_ ord: Int) async throws {
        guard editable, !held, let expected = activeLoad, draft.fields.indices.contains(ord) else { throw EditorRuntimeError.unavailable }
        _ = try await call(["type": "setFocus", "loadId": expected, "ord": ord], load: expected)
    }
    /// Browse's inspector (V12): the fields the page leaves out.
    public func setHiddenFields(_ ords: Set<Int>) async throws {
        draft.hiddenFields = ords
        guard editable, let expected = activeLoad else { throw EditorRuntimeError.unavailable }
        _ = try await call(["type": "setHidden", "loadId": expected, "ords": ords.sorted()], load: expected)
    }
    /// N21.8's Edit Note: the page takes the keyboard focus, then the field.
    public func focusField(_ ord: Int) async throws {
        #if os(macOS)
        webView.window?.makeFirstResponder(webView)
        #endif
        try await setFocus(ord)
    }

    // MARK: Formatting (editor/formatting F3, editor/cloze-wrap)

    /// F3's inline formats the inspector's bar and Format menu offer.
    public enum InlineFormat: String, CaseIterable, Sendable { case bold, italic, underline, superscript, `subscript` }

    /// F2: the commands act on the focused rich field only.
    public var canFormat: Bool { Self.formats(editable: editable, held: held, fieldFocused: fieldFocused, mode: focusedMode) }
    /// Option A: the source toggle, like the rest of the bar, only while a field has the focus.
    public var canToggleSource: Bool { editable && !held && fieldFocused && focusedMode != nil }
    /// The bar's rule: a page taking input, a field with the focus, in rich mode.
    nonisolated static func formats(editable: Bool, held: Bool, fieldFocused: Bool, mode: NoteEditorDraft.Mode?) -> Bool {
        editable && !held && fieldFocused && mode == .rich
    }
    /// W2: Cloze in a cloze note type's cloze field.
    public var canCloze: Bool {
        guard canFormat, let ord = draft.focusedField else { return false }
        return draft.notetype.kind == 1 && draft.notetype.clozeFields.contains(UInt32(ord))
    }

    /// W2: the cloze group shows for a cloze note type with at least one shown cloze field.
    public var showsCloze: Bool {
        draft.notetype.kind == 1 && draft.notetype.clozeFields.contains { field in
            let ord = Int(field)
            return draft.fields.indices.contains(ord) && !draft.hiddenFields.contains(ord) && !draft.hostFields.contains(ord)
        }
    }

    public func format(_ command: InlineFormat) {
        guard canFormat, let expected = activeLoad else { return }
        Task { _ = try? await call(["type": "format", "loadId": expected, "command": command.rawValue], load: expected) }
    }

    /// W3: the number from every field's session value after a flush; W4: the page wraps the selection.
    public func cloze(sameNumber: Bool) {
        guard canCloze, let expected = activeLoad, let ord = draft.focusedField else { return }
        Task {
            do {
                try await flush(hold: false)
                // F13: the wrap names the field Cloze was chosen in. Focus can move
                // during the flush (Next Field on the keyboard bar); the page refuses then.
                guard draft.focusedField == ord else { return }
                let highest = Self.highestCloze(draft.values)
                let number = sameNumber ? max(highest, 1) : highest + 1
                _ = try await call(["type": "wrap", "loadId": expected, "ord": ord, "prefix": "{{c\(number)::", "suffix": "}}"],
                                   load: expected)
            } catch {}
        }
    }

    /// W3 step 2: every `{{c` + ASCII digits + `::` in the text, saturating at 2^53 - 1.
    nonisolated static func highestCloze(_ values: [String]) -> UInt64 {
        let limit: UInt64 = 9_007_199_254_740_991
        var highest: UInt64 = 0
        for value in values {
            let bytes = Array(value.utf8)
            var index = 0
            while index + 3 < bytes.count {
                guard bytes[index] == 0x7B, bytes[index + 1] == 0x7B, bytes[index + 2] == 0x63 else { index += 1; continue }
                var cursor = index + 3, number: UInt64 = 0, digits = 0
                while cursor < bytes.count, (0x30...0x39).contains(bytes[cursor]) {
                    let digit = UInt64(bytes[cursor] - 0x30)
                    number = number > (limit - digit) / 10 ? limit : number * 10 + digit
                    cursor += 1; digits += 1
                }
                if digits > 0, cursor + 1 < bytes.count, bytes[cursor] == 0x3A, bytes[cursor + 1] == 0x3A {
                    highest = max(highest, number)
                    index = cursor + 2
                } else {
                    index += 3
                }
            }
        }
        return highest
    }

    public func setMode(_ mode: NoteEditorDraft.Mode, ord: Int) async throws {
        guard editable, !held, let expected = activeLoad, draft.fields.indices.contains(ord) else { throw EditorRuntimeError.unavailable }
        _ = try await call(["type": "setFieldState", "loadId": expected, "ord": ord, "mode": mode.rawValue], load: expected)
    }

    /// Recovery is explicit so a containing flow can explain the possible
    /// loss of the last uncommitted input. It never rebases the draft.
    public func recoverCommittedDraft() async throws {
        guard processTerminated, !editable, !closed, isCurrent() else { throw EditorRuntimeError.unavailable }
        guard activeLoad == nil, loadID < Self.maximumID else { throw EditorRuntimeError.unavailable }
        // Revocation already settled the old processing item. Recovery must
        // not wait indefinitely for its outstanding Core conversion.
        draft.acknowledgeProcessLoss()
        processTerminated = false
        startup = nil
        try await startLoad()
    }

    private func requireCurrent(_ expected: UInt64) throws {
        guard !closed, activeLoad == expected else { throw EditorRuntimeError.unavailable }
        guard isCurrent() else {
            fail(EditorRuntimeError.unavailable, load: expected)
            throw EditorRuntimeError.unavailable
        }
    }

    private func call(_ message: [String: Any], load expected: UInt64) async throws -> Any? {
        try requireCurrent(expected)
        // Structured arguments preserve quotes, </script>, Unicode and HTML.
        // No part of field content is ever evaluated as JavaScript source.
        do {
            let result = try await webView.callAsyncJavaScript("return await meguriEditorRuntime.receive(message);",
                arguments: ["message": message], in: nil, contentWorld: Self.world)
            try requireCurrent(expected)
            return result
        } catch { fail(error, load: expected); throw error }
    }

    fileprivate func receive(_ scriptMessage: WKScriptMessage, reply: @escaping @MainActor @Sendable (Any?, String?) -> Void) {
        guard !closed, scriptMessage.name == "meguriEditor", scriptMessage.webView === webView, scriptMessage.frameInfo.isMainFrame,
              scriptMessage.frameInfo.securityOrigin.protocol == "meguri-editor",
              scriptMessage.frameInfo.securityOrigin.host == "editor", webView.url == EditorDocument.url else {
            Self.log.error("Rejected editor message origin"); reply(nil, "Invalid editor origin"); return
        }
        guard let expected = activeLoad else { reply(nil, "Editor load revoked"); return }
        do {
            try requireCurrent(expected)
            let message = try EditorBridgeMessage(scriptMessage.body, ordinals: Set(draft.fields.indices))
            if message.type == "ready" {
                guard let ready else { throw EditorRuntimeError.invalidMessage }
                self.ready = nil; readyTimeout?.cancel(); readyTimeout = nil
                reply(["accepted": true], nil); ready.resume(); return
            }
            if let incoming = message.loadID, incoming < expected { reply(["ignored": true], nil); return }
            guard openedLoad == expected, message.loadID == expected else { throw EditorRuntimeError.invalidMessage }
            insert(message, load: expected) { result in
                switch result {
                case .success: reply(["accepted": true], nil)
                case .failure: reply(nil, "Editor message refused")
                }
            }
        } catch {
            Self.log.error("Rejected editor payload"); reply(nil, "Invalid editor payload"); fail(error, load: expected)
        }
    }

    private func enqueue(_ message: EditorBridgeMessage, load expected: UInt64) async throws {
        try await withCheckedThrowingContinuation { continuation in
            insert(message, load: expected) { continuation.resume(with: $0) }
        }
    }
    private func insert(_ message: EditorBridgeMessage, load expected: UInt64,
                        complete: @escaping (Result<Void, any Error>) -> Void) {
        do { try requireCurrent(expected) }
        catch { complete(.failure(error)); return }
        guard openedLoad == expected, let sequence = message.sequence, message.loadID == expected, sequence >= nextSequence,
              sequence < Self.maximumID, sequence - nextSequence <= 256,
              pending[sequence] == nil, processing?.message.sequence != sequence else {
            complete(.failure(EditorRuntimeError.sequenceGap)); fail(EditorRuntimeError.sequenceGap, load: expected); return
        }
        pending[sequence] = Pending(message: message, complete: complete)
        guard draining == nil else { return }
        drainingLoad = expected
        draining = Task { [self] in
            defer {
                if drainingLoad == expected { draining = nil; drainingLoad = nil; processing = nil }
            }
            while activeLoad == expected, let item = pending.removeValue(forKey: nextSequence) {
                processing = item
                do {
                    try requireCurrent(expected)
                    try await apply(item.message, load: expected)
                    try requireCurrent(expected)
                    nextSequence += 1; processing = nil; item.settle(.success(()))
                    gapTimeout?.cancel(); gapTimeout = nil
                }
                catch { item.settle(.failure(error)); fail(error, load: expected); return }
            }
            if activeLoad == expected, !pending.isEmpty, gapTimeout == nil {
                gapTimeout = Task { [weak self] in
                    do { try await Task.sleep(for: .seconds(10)) } catch { return }
                    self?.fail(EditorRuntimeError.sequenceGap, load: expected)
                }
            }
        }
    }
    private func apply(_ message: EditorBridgeMessage, load expected: UInt64) async throws {
        try requireCurrent(expected)
        switch message.type {
        case "loaded":
            editable = true; error = nil
            if let interval { Signposts.end(interval); self.interval = nil }
            checkFields(load: expected)
        case "commit", "flush":
            try await draft.accept(message.commits, ifCurrent: { [weak self] in
                guard let self else { return false }
                return (try? self.requireCurrent(expected)) != nil
            })
            try requireCurrent(expected)
            checkFields(load: expected)
        case "dirty": draft.markDirty(EditorBridgeMessage.integer(message.body["ord"])!)
        case "focus": draft.setFocus(EditorBridgeMessage.integer(message.body["ord"])); fieldFocused = true
        case "blur": fieldFocused = false; onFieldBlur?()
        case "modeChanged": draft.setMode(NoteEditorDraft.Mode(rawValue: message.body["mode"] as! String)!, ord: EditorBridgeMessage.integer(message.body["ord"])!)
        case "collapsedChanged": draft.setCollapsed(EditorBridgeMessage.boolean(message.body["collapsed"])!, ord: EditorBridgeMessage.integer(message.body["ord"])!)
        case "historyState":
            if draft.focusedField == EditorBridgeMessage.integer(message.body["ord"]) {
                canUndo = EditorBridgeMessage.boolean(message.body["canUndo"])!
                canRedo = EditorBridgeMessage.boolean(message.body["canRedo"])!
            }
        case "focusLeave": draft.setFocus(nil); focusLeave?(message.body["direction"] as? String == "previous")
        case "occlusionFormLayout":
            guard usesOcclusionForm else { throw EditorRuntimeError.invalidMessage }
            occlusionFormLayout = .init(imageTop: EditorBridgeMessage.number(message.body["imageTop"])!,
                imageHeight: EditorBridgeMessage.number(message.body["imageHeight"])!,
                tagsTop: EditorBridgeMessage.number(message.body["tagsTop"])!)
            contentHeight = EditorBridgeMessage.number(message.body["height"])!
        case "contentHeight": contentHeight = EditorBridgeMessage.number(message.body["height"])!
        case "caret":
            caret = EditorBridgeMessage.number(message.body["top"])!...EditorBridgeMessage.number(message.body["bottom"])!
        case "playMedia": if let name = (message.body["name"] as? String).flatMap(MediaName.plain) { onPlayMedia?(name) }
        case "openMedia": if let name = (message.body["name"] as? String).flatMap(MediaName.plain) { onOpenMedia?(name) }
        case "pastePicture":
            #if os(macOS)
            pastePicture(ord: EditorBridgeMessage.integer(message.body["ord"])!,
                         request: EditorBridgeMessage.integer(message.body["request"])!, load: expected)
            #endif
        default: throw EditorRuntimeError.invalidMessage
        }
        refreshAccessory()
    }
    private func checkFields(load expected: UInt64) {
        Task { [weak self] in
            guard let self, (try? requireCurrent(expected)) != nil else { return }
            await draft.checkFields(ifCurrent: { [weak self] in
                guard let self else { return false }
                return (try? self.requireCurrent(expected)) != nil
            })
        }
    }
    #if os(macOS)
    private func pastePicture(ord: Int, request: Int, load expected: UInt64) {
        // Only the immutable pasteboard snapshot runs on AppKit's actor.
        let snapshot = FieldPicturePaste.snapshot(picturePasteboard)
        cancelPictureTask()
        let importer = pictureMedia ?? CoreMediaService(expectedGeneration: draft.generation)
        pictureTask = Task { [weak self] in
            guard let self else { return }
            var html = ""
            do {
                let clipboardHTML = await Task.detached { FieldPicturePaste.htmlText(snapshot) }.value
                let isCopyImage: Bool
                if let clipboardHTML {
                    let result = try await call(["type": "classifyPicturePaste", "loadId": expected,
                        "html": clipboardHTML, "urls": snapshot.urls.map(\.absoluteString)], load: expected) as? [String: Any]
                    isCopyImage = result?["copyImage"] as? Bool == true
                } else { isCopyImage = false }
                let sources = await Task.detached { FieldPicturePaste.sources(snapshot, isCopyImage: isCopyImage) }.value
                try await requirePictureTarget(ord: ord, request: request, load: expected)
                let asPNG = sources.contains(where: \.isBitmap) ? (try? await CoreEditorServices().pasteImagesAsPNG()) ?? false : false
                for source in sources {
                    var name = source.name
                    do {
                        try await requirePictureTarget(ord: ord, request: request, load: expected)
                        // Disk files warn before allocation. Compressed bitmap
                        // input size says nothing about the encoded output.
                        var approvedSize: Int?
                        if !source.isBitmap {
                            let size = try await Task.detached { try source.size() }.value
                            if FieldPicturePaste.needsSizeWarning(size) {
                                guard try await approvePictureSize(name: name, size: size, ord: ord, request: request, load: expected) else { continue }
                                approvedSize = size
                            }
                        }
                        let picture = try await Task.detached { try source.prepare(asPNG: asPNG) }.value
                        name = picture.name
                        if FieldPicturePaste.needsSizeWarning(picture.bytes.count), approvedSize != picture.bytes.count {
                            guard try await approvePictureSize(name: name, size: picture.bytes.count, ord: ord, request: request, load: expected) else { continue }
                        }
                        // The existing MediaService accepts an array; allocate
                        // it on the worker and release each file before the next.
                        let bytes = await Task.detached { Array(picture.bytes) }.value
                        try await requirePictureTarget(ord: ord, request: request, load: expected)
                        let stored = try await importer.addMedia(name: name, bytes: bytes)
                        try await requirePictureTarget(ord: ord, request: request, load: expected)
                        let safe = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_.-~/")
                        guard let src = stored.addingPercentEncoding(withAllowedCharacters: safe) else { throw EditorRuntimeError.invalidMessage }
                        html += "<img src=\"\(src)\">"
                    } catch {
                        // One unreadable file never discards preceding pictures
                        // or stops the later items of this paste.
                        try await requirePictureTarget(ord: ord, request: request, load: expected)
                        reportPictureFailure(error, name: name.isEmpty ? nil : name)
                    }
                }
                try await requirePictureTarget(ord: ord, request: request, load: expected)
                _ = try await call(["type": "finishPicturePaste", "loadId": expected, "ord": ord, "request": request, "html": html], load: expected)
            } catch {
                // A hold, reload, later edit or retired owner invalidates the
                // request. Never clear a newer paste's state or restore its caret.
            }
        }
    }
    private func requirePictureTarget(ord: Int, request: Int, load expected: UInt64) async throws {
        try Task.checkCancellation(); try requireCurrent(expected)
        guard !held else { throw EditorRuntimeError.unavailable }
        let result = try await call(["type": "picturePasteCurrent", "loadId": expected, "ord": ord, "request": request], load: expected) as? [String: Any]
        try Task.checkCancellation(); try requireCurrent(expected)
        guard result?["current"] as? Bool == true else { throw EditorRuntimeError.unavailable }
    }
    private func cancelPictureTask() {
        pictureTask?.cancel(); pictureTask = nil
        if let alert = pictureSizeAlert, let window = alert.window.sheetParent {
            window.endSheet(alert.window, returnCode: .alertSecondButtonReturn)
        }
        pictureSizeAlert = nil
    }
    private func approvePictureSize(name: String, size: Int, ord: Int, request: Int, load expected: UInt64) async throws -> Bool {
        try await requirePictureTarget(ord: ord, request: request, load: expected)
        if let confirmPictureSize { return await confirmPictureSize(name, size) }
        // Present only on a visible owning window, including after unhide.
        let window: NSWindow
        while true {
            while webView.window?.isVisible != true || NSApp.isHidden || pictureFailureAlert != nil {
                try await Task.sleep(for: .milliseconds(50))
                try Task.checkCancellation(); try requireCurrent(expected)
                guard !held else { throw EditorRuntimeError.unavailable }
            }
            try await requirePictureTarget(ord: ord, request: request, load: expected)
            // The WebKit check suspends: recheck visibility before presenting.
            if let visible = webView.window, visible.isVisible, !NSApp.isHidden, pictureFailureAlert == nil {
                window = visible; break
            }
        }
        let alert = NSAlert()
        alert.messageText = String(localized: "This File Won't Sync", bundle: .module)
        let formatted = ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)
        alert.informativeText = String(localized: "“\(name)” is \(formatted). Files this size don't sync to your other devices.", bundle: .module)
        alert.addButton(withTitle: String(localized: "Add Anyway", bundle: .module))
        alert.addButton(withTitle: String(localized: "Cancel", bundle: .module))
        pictureSizeAlert = alert
        let response = await alert.beginSheetModal(for: window)
        if pictureSizeAlert === alert { pictureSizeAlert = nil }
        try Task.checkCancellation(); try requireCurrent(expected)
        presentPictureFailure()
        return response == .alertFirstButtonReturn
    }
    private func reportPictureFailure(_ failure: any Error, name: String? = nil) {
        let path = (failure as NSError).userInfo[NSFilePathErrorKey] as? String
        pictureFailures.append((name ?? path.map { URL(filePath: $0).lastPathComponent }
            ?? String(localized: "Picture", bundle: .module), failure.localizedDescription))
        if pictureUnhideObserver == nil {
            pictureUnhideObserver = NotificationCenter.default.addObserver(forName: NSApplication.didUnhideNotification,
                object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.presentPictureFailure() }
            }
        }
        presentPictureFailure()
    }
    private func presentPictureFailure() {
        // AppKit must never present an alert while the app is hidden.
        guard !closed, isCurrent(), pictureFailureAlert == nil, pictureSizeAlert == nil,
              let window = webView.window, window.isVisible, !NSApp.isHidden, !pictureFailures.isEmpty else { return }
        let failure = pictureFailures.removeFirst(), alert = NSAlert()
        alert.messageText = String(localized: "Couldn't Add “\(failure.name)”", bundle: .module)
        alert.informativeText = failure.cause
        alert.addButton(withTitle: String(localized: "OK", bundle: .module))
        pictureFailureAlert = alert
        alert.beginSheetModal(for: window) { [weak self] _ in
            Task { @MainActor in self?.pictureFailureAlert = nil; self?.presentPictureFailure() }
        }
    }
    #endif
    private func fail(_ failure: any Error, load expected: UInt64) {
        guard activeLoad == expected else { return }
        activeLoad = nil; openedLoad = nil; allowsDocumentNavigation = false; documentNavigation = nil
        error = String(describing: failure); editable = false
        commandLoad = nil
        #if os(macOS)
        cancelPictureTask()
        pictureFailures = []
        if let alert = pictureFailureAlert, let window = alert.window.sheetParent { window.endSheet(alert.window) }
        pictureFailureAlert = nil
        if let pictureUnhideObserver { NotificationCenter.default.removeObserver(pictureUnhideObserver) }
        pictureUnhideObserver = nil
        #endif
        readyTimeout?.cancel(); readyTimeout = nil
        gapTimeout?.cancel(); gapTimeout = nil
        let waiter = ready; ready = nil; waiter?.resume(throwing: failure)
        let buffered = pending.values; pending.removeAll()
        let actual = processing; processing = nil
        draining?.cancel(); draining = nil; drainingLoad = nil
        actual?.settle(.failure(failure))
        for item in buffered { item.settle(.failure(failure)) }
        if let interval { Signposts.end(interval); self.interval = nil }
    }

    /// Final disposal follows the containing owner's held flush/close guard.
    /// It never deletes targets or writes a note. Do not call on layout change.
    public func shutDown() {
        occlusionImage.cancel()
        guard !closed else { return }
        closed = true
        if let expected = activeLoad { fail(EditorRuntimeError.unavailable, load: expected) }
        relay.owner = nil; webView.stopLoading(); webView.navigationDelegate = nil
        webView.configuration.userContentController.removeScriptMessageHandler(forName: "meguriEditor", contentWorld: Self.world)
    }
    public func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        guard webView === self.webView, !closed, let expected = activeLoad else { return }
        processTerminated = true; fail(EditorRuntimeError.processTerminated, load: expected)
    }
    public func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: any Error) {
        if webView === self.webView, let expected = activeLoad, let navigation,
           navigation === documentNavigation { fail(error, load: expected) }
    }
    public func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: any Error) {
        if webView === self.webView, let expected = activeLoad, let navigation,
           navigation === documentNavigation { fail(error, load: expected) }
    }
    public func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                        decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
        let allowed = allowsDocumentNavigation && navigationAction.targetFrame?.isMainFrame == true &&
            navigationAction.request.url == EditorDocument.url && navigationAction.navigationType == .other
        if allowed { allowsDocumentNavigation = false }
        decisionHandler(allowed ? .allow : .cancel)
    }
}

private final class EditorMessageRelay: NSObject, WKScriptMessageHandlerWithReply {
    weak var owner: FieldEditorHost?
    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage,
                               replyHandler: @escaping @MainActor @Sendable (Any?, String?) -> Void) {
        guard let owner else { replyHandler(nil, "Editor closed"); return }
        owner.receive(message, reply: replyHandler)
    }
}

/// A mount for the retained view, with WebKit as the sole scroll/focus owner
/// for fields. The integrated Mac IO form reserves native rows in this page
/// and, like Browse, gives scrolling to its enclosing native view.
public struct FieldEditorView: View {
    public let host: FieldEditorHost
    public init(host: FieldEditorHost) { self.host = host }
    public var body: some View {
        VStack(spacing: 0) {
            FieldEditorMount(host: host)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("editor.fields")
    }
}
#if os(macOS)
private struct FieldEditorMount: NSViewRepresentable {
    let host: FieldEditorHost
    func makeNSView(context: Context) -> WKWebView { host.webView }
    func updateNSView(_ nsView: WKWebView, context: Context) {}
}
#else
private struct FieldEditorMount: UIViewRepresentable {
    let host: FieldEditorHost
    func makeUIView(context: Context) -> WKWebView { host.webView }
    func updateUIView(_ uiView: WKWebView, context: Context) {}
}
#endif

#if os(macOS)
/// The page paints no background of its own, so the sheet's, panel's or
/// pane's shows through (the page's own CSS is transparent on the Mac). In
/// Browse's inspector the page is as tall as its fields and sits in the
/// pane's scroll view: it never scrolls itself, so the wheel goes to the
/// scroll view.
final class FieldEditorMacWebView: WKWebView {
    var inInspector = false
    var inForm = false
    override init(frame: CGRect, configuration: WKWebViewConfiguration) {
        super.init(frame: frame, configuration: configuration)
        // WebKit's background switch has no public setter on the Mac; KVC reaches it
        // only where it exists, and the page stays readable on its default otherwise.
        if responds(to: NSSelectorFromString("_setDrawsBackground:")) { setValue(false, forKey: "drawsBackground") }
    }
    @available(*, unavailable) required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }
    override func scrollWheel(with event: NSEvent) {
        if inInspector || inForm, let next = nextResponder { next.scrollWheel(with: event) } else { super.scrollWheel(with: event) }
    }
}
#endif

#if os(iOS)
/// WebKit asks the web view for the keyboard bar, so a subclass supplies it
/// (N17.1, 2026-11-27): Bold, Italic, Underline and Cloze at the leading
/// edge, then previous and next field, then the HTML control. There is no
/// Done: the form's own Done saves, and dragging the form down hides the
/// keyboard. The bar outlives a draft; its actions go to the host that
/// currently owns the page.
final class FieldEditorWebView: WKWebView {
    weak var owner: FieldEditorHost?
    private(set) lazy var boldItem = item("bold", "Bold", "editor.toolbar.bold") { $0.format(.bold) }
    private(set) lazy var italicItem = item("italic", "Italic", "editor.toolbar.italic") { $0.format(.italic) }
    private(set) lazy var underlineItem = item("underline", "Underline", "editor.toolbar.underline") { $0.format(.underline) }
    /// A tap adds the next number; touching and holding offers the same number (`editor/cloze-wrap` W2).
    private(set) lazy var clozeItem: UIBarButtonItem = {
        let clozeItem = item("curlybraces", "Cloze Deletion", "editor.toolbar.clozeNew") { $0.cloze(sameNumber: false) }
        let same = UIAction(title: String(localized: "Cloze Deletion (Same Number)", bundle: .module)) { [weak self] _ in
            self?.owner?.cloze(sameNumber: true)
        }
        same.accessibilityIdentifier = "editor.toolbar.clozeSame"
        clozeItem.menu = UIMenu(children: [same])
        return clozeItem
    }()
    var formatItems: [UIBarButtonItem] { [boldItem, italicItem, underlineItem] }
    private(set) lazy var modeItem = item("chevron.left.forwardslash.chevron.right", "HTML Editor", "editor.add.htmlToggle") { $0.toggleFocusedMode() }
    private(set) lazy var previousItem = item("chevron.up", "Previous Field", "editor.add.previousField") { $0.moveFocus(previous: true) }
    private(set) lazy var nextItem = item("chevron.down", "Next Field", "editor.add.nextField") { $0.moveFocus(previous: false) }
    private lazy var accessory: UIToolbar = {
        let bar = UIToolbar()
        bar.items = formatItems + [clozeItem, .flexibleSpace(), previousItem, nextItem, .fixedSpace(0), modeItem]
        bar.sizeToFit()
        return bar
    }()
    override var inputAccessoryView: UIView? { accessory }

    private func item(_ symbol: String, _ label: String.LocalizationValue, _ identifier: String,
                      _ action: @escaping @MainActor (FieldEditorHost) -> Void) -> UIBarButtonItem {
        let item = UIBarButtonItem(image: UIImage(systemName: symbol), primaryAction: UIAction { [weak self] _ in
            if let owner = self?.owner { action(owner) }
        })
        item.accessibilityLabel = String(localized: label, bundle: .module)
        item.accessibilityIdentifier = identifier
        return item
    }
}
#endif

// The collection retains this actual host through the AppModel runtime boundary.
// A remounted form reads it; it must not create/dispose a host on appear/disappear.
extension ReviewCollection {
    @discardableResult public func prepareAddNoteEditor(presetDeck: Int64? = nil) async -> FieldEditorHost? {
        let flow = addNoteFlow
        let media = self.media
        guard await flow.open(presetDeck: presetDeck, continuingPage: true, makeRuntime: { draft, current in
            let host = try FieldEditorHost(draft: draft, isCurrent: current)
            host.show(media: media)
            return host
        }) else { return nil }
        return flow.runtime as? FieldEditorHost
    }
}
