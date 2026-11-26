import AppModel
import CoreBridge
import ImageOcclusion
import SwiftUI
import UniformTypeIdentifiers
#if os(macOS)
import AppKit
#else
import PhotosUI
import UIKit
#endif

/// One native entry for Add, Edit Current and Browse. The field page and
/// canvas stay mounted across the Mask Editor toggle; a toggle never saves.
public struct OcclusionNoteEditor: View {
    public let host: FieldEditorHost
    public init(host: FieldEditorHost) { self.host = host }
    @State private var filePicker = false
    @State private var changeImage = false
    @State private var fileTicket: UUID?
    #if os(iOS)
    @State private var photo: PhotosPickerItem?
    @State private var camera = false
    @State private var cameraTicket: UUID?
    #endif
    private var draft: NoteEditorDraft { host.draft }
    private var session: OcclusionImageSession { host.occlusionImage }
    private var roles: OcclusionFieldRoles? { draft.occlusionRoles }
    private var readingError: String? { host.occlusionReadingError }
    private var masksShown: Bool { host.maskEditorVisible }

    public var body: some View {
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
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("io.editor")
        .onChange(of: masksShown, initial: true) { _, shown in host.showFields(!shown) }
        .onChange(of: roles.map { draft.values[$0.occlusions] }) { host.refreshMaskSession() }
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
        .fileImporter(isPresented: $filePicker, allowedContentTypes: [.image], allowsMultipleSelection: false) { result in
            guard let ticket = fileTicket else { return }
            fileTicket = nil
            switch result {
            case .success(let urls):
                session.receiveFile(urls.first, ticket: ticket, host: host)
            case .failure(let error):
                session.receiveFile(nil, ticket: ticket, host: host)
                if (error as NSError).code != NSUserCancelledError {
                    session.sourceFailed(error.localizedDescription, ticket: ticket, host: host)
                }
            }
        }
        .confirmationDialog(Text(verbatim: IONoteText.changeTitle), isPresented: $changeImage, titleVisibility: .visible) {
            Button { session.replacing = true } label: { Text(verbatim: IONoteText.confirmChange) }
            Button(role: .cancel) {} label: { Text("Cancel", bundle: .module) }
        } message: { Text(verbatim: IONoteText.changeMessage) }
        .alert(Text("Couldn't Get Media", bundle: .module), isPresented: Binding(
            get: { session.fetchFailure != nil }, set: { if !$0 { session.fetchFailure = nil } }
        )) {
            Button { session.fetchFailure = nil } label: { Text("OK", bundle: .module) }
        } message: { Text(verbatim: session.fetchFailure ?? "") }
        .alert(Text("This File Won't Sync", bundle: .module), isPresented: Binding(
            get: { session.sizeWarning != nil }, set: { if !$0 { session.answerSize(false) } }
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
        .sheet(isPresented: $camera) {
            OcclusionCamera { data in
                camera = false
                if let ticket = cameraTicket { session.receiveBitmap(data, ticket: ticket, host: host, photo: true) }
                cameraTicket = nil
            }
        }
        #endif
    }

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
                    .accessibilityIdentifier("io.changeImage")
            }
        }.padding()
    }

    @ViewBuilder private var maskContent: some View {
        if let error = readingError, !session.startsNewMasks {
            ContentUnavailableView {
                Text(verbatim: IONoteText.unreadableMasks(error, copying: true))
            } actions: {
                Button { host.drawNewMasks() } label: {
                    Text(verbatim: IONoteText.drawNew)
                }.accessibilityIdentifier("io.editor.drawNewMasks")
            }
        } else if let image = session.image, let roles {
            ZStack {
                MaskEditorView(image: Image(decorative: image.pixels, scale: 1), imageSize: image.size,
                    occlusions: Binding(get: { session.startsNewMasks && readingError != nil ? "" : draft.values[roles.occlusions] },
                        set: { value in
                            guard host.acceptsHostInput else { return }
                            host.setHostField(value, at: roles.occlusions); session.startsNewMasks = false
                        }), isEditing: draft.noteID != nil, session: host.maskSession)
                    .disabled(!masksShown || !host.acceptsHostInput || session.busy || session.replacing)
                    .opacity(session.replacing ? 0 : 1)
                    .accessibilityHidden(session.replacing)
                if session.replacing { picker }
            }
        } else if !session.replacing, draft.noteID != nil || roles.map({ !draft.values[$0.image].isEmpty }) == true {
            ContentUnavailableView {
                Label { Text(verbatim: session.error ?? IONoteText.noImage) } icon: { Image(systemName: "photo") }
            }
            .accessibilityIdentifier("io.editor.imageUnavailable")
        } else {
            picker
        }
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
                    .keyboardShortcut("v", modifiers: .command)
                    .accessibilityIdentifier("io.picker.paste")
                #if os(iOS)
                PhotosPicker(selection: $photo, matching: .images) { Text(verbatim: IONoteText.photoLibrary) }
                    .accessibilityIdentifier("io.picker.photoLibrary")
                if UIImagePickerController.isSourceTypeAvailable(.camera) {
                    Button {
                        if let ticket = session.reserveSource(host: host) { cameraTicket = ticket; camera = true }
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
        .disabled(!host.acceptsHostInput)
        .onDrop(of: [.fileURL, .url, .image], isTargeted: nil) { providers in
            session.chooseProvider(providers, host: host)
        }
        .accessibilityIdentifier("io.picker")
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
private struct OcclusionCamera: UIViewControllerRepresentable {
    let finished: (Data?) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(finished: finished) }
    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = .camera; picker.mediaTypes = [UTType.image.identifier]; picker.delegate = context.coordinator
        return picker
    }
    func updateUIViewController(_ controller: UIImagePickerController, context: Context) {}
    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let finished: (Data?) -> Void
        init(finished: @escaping (Data?) -> Void) { self.finished = finished }
        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) { finished(nil) }
        func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
            // Rendering applies UIImage's orientation before the common encoder.
            guard let image = info[.originalImage] as? UIImage else { finished(nil); return }
            let format = UIGraphicsImageRendererFormat(); format.scale = 1
            let rendered = UIGraphicsImageRenderer(size: image.size, format: format).image { _ in
                image.draw(in: CGRect(origin: .zero, size: image.size))
            }
            finished(rendered.pngData())
        }
    }
}
#endif
