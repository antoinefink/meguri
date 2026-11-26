import AppModel
import CoreBridge
import ImageOcclusion
import DesignSystem
import SwiftUI
import UniformTypeIdentifiers
#if os(macOS)
import AppKit
#else
import PhotosUI
import UIKit
#endif

/// Add/Edit use the integrated Mac form. Browse and touch retain their
/// existing fields/masks presentation and the same generation-bound host.
public struct OcclusionNoteEditor: View {
    public let host: FieldEditorHost
    public init(host: FieldEditorHost) { self.host = host }
    @State private var filePicker = false
    @State private var changeImage = false
    @State private var fileTicket: UUID?
    #if os(iOS)
    @State private var photo: PhotosPickerItem?
    private struct CameraSource: Identifiable { let id: UUID }
    @State private var cameraSource: CameraSource?
    #endif
    private var draft: NoteEditorDraft { host.draft }
    private var session: OcclusionImageSession { host.occlusionImage }
    private var roles: OcclusionFieldRoles? { draft.occlusionRoles }
    private var readingError: String? { host.occlusionReadingError }
    private var masksShown: Bool { host.maskEditorVisible }

    public var body: some View {
        Group {
            #if os(macOS)
            if host.usesOcclusionForm { integratedForm }
            else { legacyBody }
            #else
            legacyBody
            #endif
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("io.editor.container")
        .onChange(of: masksShown, initial: true) { _, shown in host.showFields(host.usesOcclusionForm || !shown) }
        .onChange(of: host.acceptsMaskInput, initial: true) { _, enabled in
            if enabled { host.restoreTextFocusAfterMaskActivation() }
        }
        .onChange(of: roles.map { draft.values[$0.occlusions] }) { host.refreshMaskSession() }
        .onAppear { session.retryFailedLoad() }
        .task(id: host.occlusionImageLoadKey) {
            host.refreshMaskSession()
            await session.load(host: host)
        }
        .alert(Text(verbatim: host.maskSession?.retainedCardNotice.map(IONoteText.cardRemains) ?? ""), isPresented: Binding(
            get: { host.maskSession?.retainedCardNotice != nil },
            set: { if !$0 { host.maskSession?.retainedCardNotice = nil } }
        )) {
            Button { host.maskSession?.retainedCardNotice = nil } label: { Text("OK", bundle: .module) }
        }
        .fileImporter(isPresented: $filePicker, allowedContentTypes: [.image], allowsMultipleSelection: false,
            onCompletion: { [ticket = fileTicket] result in
                guard let ticket else { return }
                if fileTicket == ticket { fileTicket = nil }
                switch result {
                case .success(let urls):
                    session.receiveFile(urls.first, ticket: ticket, host: host)
                case .failure(let error):
                    if (error as NSError).code == NSUserCancelledError { session.cancelSource(ticket: ticket) }
                    else { session.sourceFailed(error.localizedDescription, ticket: ticket, host: host) }
                }
            }, onCancellation: { [ticket = fileTicket] in
                guard let ticket else { return }
                if fileTicket == ticket { fileTicket = nil }
                session.cancelSource(ticket: ticket)
            })
        .confirmationDialog(Text(verbatim: IONoteText.changeTitle), isPresented: $changeImage, titleVisibility: .visible) {
            Button { session.replacing = true } label: { Text(verbatim: IONoteText.confirmChange) }
            Button(role: .cancel) {} label: { Text("Cancel", bundle: .module) }
        } message: { Text(verbatim: IONoteText.changeMessage) }
        .alert(Text("Couldn't Get Media", bundle: .module), isPresented: Binding(
            get: { !host.usesOcclusionForm && session.fetchFailure != nil }, set: { if !$0 { session.fetchFailure = nil } }
        )) {
            Button { session.fetchFailure = nil } label: { Text("OK", bundle: .module) }
        } message: { Text(verbatim: session.fetchFailure ?? "") }
        .alert(Text("This File Won't Sync", bundle: .module), isPresented: Binding(
            get: { !host.usesOcclusionForm && session.sizeWarning != nil }, set: { if !$0 { session.answerSize(false) } }
        )) {
            Button { session.answerSize(true) } label: { Text("Add Anyway", bundle: .module) }
            Button(role: .cancel) { session.answerSize(false) } label: { Text("Cancel", bundle: .module) }
        } message: {
            let size = ByteCountFormatter.string(fromByteCount: Int64(session.sizeWarningBytes), countStyle: .file)
            Text("“\(session.sizeWarning ?? "")” is \(size). Files this size don't sync to your other devices.", bundle: .module)
        }
        #if os(iOS)
        .onChange(of: photo) { _, item in
            guard let item, let ticket = session.reserveSource(host: host) else { return }
            session.loadBitmap(ticket: ticket, host: host) { try await item.loadTransferable(type: Data.self) }
            photo = nil
        }
        .sheet(item: $cameraSource) { source in
            OcclusionCamera(maximumDimension: host.occlusionPhotoMaximumDimension) { data in
                if cameraSource?.id == source.id { cameraSource = nil }
                session.receiveBitmap(data, ticket: source.id, host: host, photo: true)
            }
            .onDisappear { session.cancelSource(ticket: source.id) }
        }
        #endif
    }

    private var legacyBody: some View {
        VStack(spacing: 0) {
            if roles != nil { controls }
            if draft.notetype.originalStockKind == 6, roles == nil {
                Text(verbatim: IONoteText.noRoles)
                    .padding().accessibilityIdentifier("io.editor.noRoles")
            }
            if let error = readingError, draft.noteID != nil {
                Text(verbatim: IONoteText.unreadableMasks(error, copying: false))
                    .padding().accessibilityIdentifier("io.editor.unreadable")
            }
            ZStack {
                VStack(spacing: 0) {
                    FieldEditorView(host: host)
                        .id(ObjectIdentifier(host.webView))
                    if draft.notetype.originalStockKind == 6, !host.browsing {
                        OcclusionTagsField(host: host).id(draft.identity)
                    }
                }
                    .opacity(masksShown ? 0 : 1)
                    .allowsHitTesting(!masksShown)
                    .accessibilityHidden(masksShown)
                if roles != nil {
                    maskContent
                        .opacity(masksShown ? 1 : 0)
                        .allowsHitTesting(masksShown)
                        .accessibilityHidden(!masksShown)
                }
            }
            if draft.occlusionAddRefused {
                Text(verbatim: IONoteText.noOcclusions).padding()
                    .accessibilityIdentifier("io.add.noOcclusions")
            }
        }
    }

    #if os(macOS)
    @State private var tagsHeight = 28.0
    @State private var formPosition = ScrollPosition()
    @State private var formVisible = CGRect.zero
    /// One field page, with native rows at reserved positions. It never
    /// remounts or flushes a field simply because the canvas receives focus.
    private var integratedForm: some View {
        GeometryReader { viewport in
            ScrollView {
                FieldEditorView(host: host)
                    .focusedValue(\.maskEditorCommands, nil)
                    .frame(height: max(host.contentHeight, viewport.size.height))
                    .overlay(alignment: .topLeading) {
                        if let layout = host.occlusionFormLayout, let roles {
                            ZStack(alignment: .topLeading) {
                                NoteFormFieldRow {
                                    Text(verbatim: draft.fields[roles.image].definition.name + ":")
                                } content: {
                                    imageField
                                }
                                .padding(.horizontal, Spacing.medium)
                                .frame(height: layout.imageHeight)
                                .offset(y: layout.imageTop)
                                NoteFormFieldRow {
                                    Text("Tags:", bundle: .module)
                                } content: {
                                    NativeOcclusionTags(host: host).id(draft.identity)
                                        .frame(maxWidth: .infinity)
                                        .focusedValue(\.maskEditorCommands, nil)
                                }
                                .padding(.horizontal, Spacing.medium)
                                .fixedSize(horizontal: false, vertical: true)
                                .onGeometryChange(for: Double.self) { $0.size.height } action: { tagsHeight = $0 }
                                .offset(y: layout.tagsTop)
                                .accessibilityIdentifier("io.editor.tags")
                            }
                        }
                    }
            }
            .scrollPosition($formPosition)
            .onScrollGeometryChange(for: CGRect.self) { $0.visibleRect } action: { _, rect in formVisible = rect }
            .onChange(of: host.caret) { _, caret in
                guard host.fieldFocused, let caret else { return }
                if caret.lowerBound < formVisible.minY {
                    formPosition.scrollTo(y: max(0, caret.lowerBound - Spacing.small))
                } else if caret.upperBound > formVisible.maxY {
                    formPosition.scrollTo(y: max(0, caret.upperBound - formVisible.height + Spacing.small))
                }
            }
            .task(id: FormSize(viewport: viewport.size.height, tags: tagsHeight, editable: host.editable)) {
                await host.layoutOcclusionForm(viewport: viewport.size.height, tagsHeight: tagsHeight)
            }
        }
    }
    private struct FormSize: Equatable { let viewport: Double; let tags: Double; let editable: Bool }

    private var imageField: some View {
        VStack(spacing: 0) {
            maskContent.frame(maxWidth: .infinity, maxHeight: .infinity)
            inlineImageMessages
            if session.image != nil, !session.replacing {
                Divider()
                imageFooter
            }
        }
        .background(Color(nsColor: .controlBackgroundColor))
        .border(Color(nsColor: .separatorColor))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("io.editor.imageField")
    }
    @ViewBuilder private var inlineImageMessages: some View {
        VStack(alignment: .leading) {
            if draft.occlusionAddRefused {
                Text(verbatim: IONoteText.noOcclusions).accessibilityIdentifier("io.add.noOcclusions")
            }
            if let failure = session.fetchFailure {
                Text("Couldn't Get Media", bundle: .module)
                Text(verbatim: failure)
                Button { session.fetchFailure = nil } label: { Text("OK", bundle: .module) }
            }
            if let name = session.sizeWarning {
                Text("This File Won't Sync", bundle: .module)
                let size = ByteCountFormatter.string(fromByteCount: Int64(session.sizeWarningBytes), countStyle: .file)
                Text("“\(name)” is \(size). Files this size don't sync to your other devices.", bundle: .module)
                HStack {
                    Button { session.answerSize(true) } label: { Text("Add Anyway", bundle: .module) }
                    Button { session.answerSize(false) } label: { Text("Cancel", bundle: .module) }
                }
            }
        }
        .padding(draft.occlusionAddRefused || session.fetchFailure != nil || session.sizeWarning != nil ? Spacing.small : 0)
    }
    private var imageFooter: some View {
        ViewThatFits(in: .horizontal) {
            HStack { imageSummary; Spacer(); changeImageButton }
            VStack(alignment: .leading) { imageSummary; changeImageButton }
        }
        .padding(Spacing.small)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("io.editor.imageFooter")
    }
    private var imageSummary: some View {
        Label {
            VStack(alignment: .leading) {
                Text(verbatim: session.image?.name ?? "").lineLimit(1).truncationMode(.middle)
                    .help(Text(verbatim: session.image?.name ?? ""))
                if let counts = host.occlusionCounts {
                    Text("\(counts.masks) masks make \(counts.cards) cards", bundle: .module)
                        .foregroundStyle(.secondary)
                }
            }
        } icon: { Image(systemName: "photo") }
    }
    private var changeImageButton: some View {
        Button { changeImage = true } label: { Text("Change Image…", bundle: .module) }
            .disabled(!host.acceptsHostInput || session.busy)
            .accessibilityIdentifier("io.editor.changeImage")
    }
    #endif

    private var controls: some View {
        HStack {
            Toggle(isOn: Binding(get: { masksShown }, set: { draft.showsMaskEditor = $0 })) {
                Text(verbatim: IONoteText.maskEditor)
            }
            .toggleStyle(.button)
            .keyboardShortcut("m", modifiers: [.command, .option, .shift])
            .disabled(!host.acceptsHostInput || (draft.noteID != nil && readingError != nil))
            .accessibilityIdentifier("io.maskEditor.toggle")
            Spacer()
            if masksShown, !session.replacing, session.image != nil || !draft.values[roles!.image].isEmpty || draft.noteID != nil {
                Button { changeImage = true } label: { Text(verbatim: IONoteText.changeImage) }
                    .disabled(!host.acceptsHostInput || session.busy)
                    .accessibilityIdentifier("io.editor.changeImage")
            }
        }.padding()
    }

    @ViewBuilder private var maskContent: some View {
        if let error = readingError, !session.startsNewMasks {
            ContentUnavailableView {
                Text(verbatim: IONoteText.unreadableMasks(error, copying: true))
            } actions: {
                if !host.usesOcclusionForm || draft.noteID == nil {
                    Button { host.drawNewMasks() } label: {
                        Text(verbatim: IONoteText.drawNew)
                    }.accessibilityIdentifier("io.editor.drawNewMasks")
                }
            }
        } else if let image = session.image, let roles {
            ZStack {
                maskEditor(image: image, roles: roles)
                    .disabled(!host.acceptsMaskInput)
                    .opacity(session.replacing ? 0 : 1)
                    .accessibilityHidden(session.replacing)
                if session.replacing { picker }
            }
        } else if !host.usesOcclusionForm, !session.replacing, draft.noteID != nil || roles.map({ !draft.values[$0.image].isEmpty }) == true {
            ContentUnavailableView {
                Label { Text(verbatim: session.error ?? IONoteText.noImage) } icon: { Image(systemName: "photo") }
            }
            .accessibilityIdentifier("io.editor.imageUnavailable")
        } else {
            picker
        }
    }

    @ViewBuilder private func maskEditor(image: OcclusionImageInput, roles: OcclusionFieldRoles) -> some View {
        let binding = Binding(get: { session.startsNewMasks && readingError != nil ? "" : draft.values[roles.occlusions] },
            set: { value in
                guard host.acceptsHostInput else { return }
                host.setHostField(value, at: roles.occlusions); session.startsNewMasks = false
            })
        #if os(macOS)
        if host.usesOcclusionForm {
            MaskEditorView(image: Image(decorative: image.pixels, scale: 1), imageSize: image.size,
                occlusions: binding, isEditing: draft.noteID != nil, session: host.maskSession, formStyle: true)
        } else {
            MaskEditorView(image: Image(decorative: image.pixels, scale: 1), imageSize: image.size,
                occlusions: binding, isEditing: draft.noteID != nil, session: host.maskSession)
        }
        #else
        MaskEditorView(image: Image(decorative: image.pixels, scale: 1), imageSize: image.size,
            occlusions: binding, isEditing: draft.noteID != nil, session: host.maskSession)
        #endif
    }

    private var picker: some View {
        VStack {
            Text(verbatim: IONoteText.pickerTitle).font(.headline)
            if let error = session.error { Text(verbatim: error).accessibilityIdentifier("io.picker.error") }
            if session.busy {
                Group {
                    if session.gettingMedia {
                        ProgressView { Text("Getting Media…", bundle: .module) }
                            .accessibilityLabel(Text("Getting Media", bundle: .module))
                    }
                    else { ProgressView() }
                }.accessibilityIdentifier("io.picker.busy")
                Button { session.cancel(); session.replacing = false } label: { Text("Cancel", bundle: .module) }
            } else {
                Button {
                    if let ticket = session.reserveSource(host: host) { fileTicket = ticket; filePicker = true }
                } label: { Text(verbatim: IONoteText.chooseFile) }
                    .accessibilityIdentifier("io.picker.chooseFile")
                Button(action: paste) { Text(verbatim: IONoteText.paste) }
                    .keyboardShortcut(masksShown && !host.usesOcclusionForm ? KeyboardShortcut("v", modifiers: .command) : nil)
                    .accessibilityIdentifier("io.picker.paste")
                #if os(iOS)
                PhotosPicker(selection: $photo, matching: .images) { Text(verbatim: IONoteText.photoLibrary) }
                    .accessibilityIdentifier("io.picker.photoLibrary")
                if UIImagePickerController.isSourceTypeAvailable(.camera) {
                    Button {
                        if let ticket = session.reserveSource(host: host) { cameraSource = CameraSource(id: ticket) }
                    } label: { Text(verbatim: IONoteText.takePhoto) }
                        .accessibilityIdentifier("io.picker.takePhoto")
                }
                #endif
                Text(verbatim: IONoteText.dropHint).foregroundStyle(.secondary)
                if session.replacing {
                    Button { session.replacing = false } label: { Text("Cancel", bundle: .module) }
                        .accessibilityIdentifier("io.changeImage.cancel")
                }
            }
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .contentShape(Rectangle())
        .disabled(!masksShown || !host.acceptsHostInput)
        .onDrop(of: [.fileURL, .url, .image], isTargeted: nil) { providers in
            session.chooseProvider(providers, host: host)
        }
        .accessibilityIdentifier("io.picker.container")
    }

    private func paste() {
        #if os(macOS)
        if let url = (NSPasteboard.general.readObjects(forClasses: [NSURL.self]) as? [URL])?.first {
            if url.isFileURL { session.chooseFile(url, host: host); return }
            if OcclusionImageSession.isRemoteImageURL(url) { session.chooseRemoteURL(url, host: host); return }
        }
        if let image = NSImage(pasteboard: .general), let data = image.tiffRepresentation {
            session.chooseBitmap(data, host: host); return
        }
        #else
        if let url = UIPasteboard.general.url, url.isFileURL { session.chooseFile(url, host: host); return }
        if let url = UIPasteboard.general.url, OcclusionImageSession.isRemoteImageURL(url) {
            session.chooseRemoteURL(url, host: host); return
        }
        if let image = UIPasteboard.general.image, let data = image.pngData() {
            session.chooseBitmap(data, host: host); return
        }
        #endif
        session.error = IONoteText.noPasteImage
    }
}

#if os(iOS)
struct OcclusionCamera: UIViewControllerRepresentable {
    let maximumDimension: Int?
    let finished: (Data?) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(maximumDimension: maximumDimension, finished: finished) }
    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = .camera; picker.mediaTypes = [UTType.image.identifier]; picker.delegate = context.coordinator
        return picker
    }
    func updateUIViewController(_ controller: UIImagePickerController, context: Context) {}
    /// UIImage.size is in points; render oriented source pixels, bounded before
    /// allocating the PNG intermediate. Off preserves the complete source size.
    static func renderSize(image: UIImage, maximumDimension: Int?) -> CGSize {
        let pixels = CGSize(width: image.size.width * image.scale, height: image.size.height * image.scale)
        guard let maximumDimension, maximumDimension > 0,
              max(pixels.width, pixels.height) > CGFloat(maximumDimension) else { return pixels }
        let scale = CGFloat(maximumDimension) / max(pixels.width, pixels.height)
        return CGSize(width: max(1, (pixels.width * scale).rounded()),
                      height: max(1, (pixels.height * scale).rounded()))
    }

    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let maximumDimension: Int?
        let finished: (Data?) -> Void
        init(maximumDimension: Int?, finished: @escaping (Data?) -> Void) {
            self.maximumDimension = maximumDimension; self.finished = finished
        }
        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) { finished(nil) }
        func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
            // Rendering applies UIImage's orientation before the common encoder.
            guard let image = info[.originalImage] as? UIImage else { finished(nil); return }
            let size = OcclusionCamera.renderSize(image: image, maximumDimension: maximumDimension)
            let format = UIGraphicsImageRendererFormat(); format.scale = 1
            let rendered = UIGraphicsImageRenderer(size: size, format: format).image { _ in
                image.draw(in: CGRect(origin: .zero, size: size))
            }
            finished(rendered.pngData())
        }
    }
}
#endif
