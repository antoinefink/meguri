import AppModel
import CoreBridge
import Foundation
import ImageOcclusion
import Observation
import UniformTypeIdentifiers

/// Retained by the field host, so a picker or decode can never retarget a later
/// draft or Browse row. Media is written through the generation-bound importer.
@Observable @MainActor
final class OcclusionImageSession {
    private(set) var image: OcclusionImageInput?
    private(set) var loadedField: String?
    private(set) var busy = false
    private(set) var gettingMedia = false
    var error: String?
    var replacing = false
    var startsNewMasks = false
    var sizeWarning: String?
    private(set) var sizeWarningBytes = 0
    private var sizeAnswer: CheckedContinuation<Bool, Never>?
    private var task: Task<Void, Never>?
    private var request = UUID()
    var completion: Task<Void, Never>? { task }

    func reserveSource(host: FieldEditorHost) -> UUID? {
        guard host.acceptsHostInput, !busy else { return nil }
        cancel(); busy = true; error = nil
        return request
    }
    func receiveBitmap(_ data: Data?, ticket: UUID, host: FieldEditorHost, photo: Bool = false) {
        guard request == ticket, host.acceptsHostInput else { return }
        busy = false
        if let data { chooseBitmap(data, host: host, photo: photo) }
        else { replacing = false }
    }
    func receiveFile(_ url: URL?, ticket: UUID, host: FieldEditorHost) {
        guard request == ticket, host.acceptsHostInput else { return }
        busy = false
        if let url { chooseFile(url, host: host) } else { replacing = false }
    }
    func sourceFailed(_ message: String, ticket: UUID, host: FieldEditorHost) {
        guard request == ticket, host.ownsDraft else { return }
        busy = false; error = message
    }

    func chooseProvider(_ providers: [NSItemProvider], host: FieldEditorHost) -> Bool {
        guard let provider = providers.first, let ticket = reserveSource(host: host) else { return false }
        // Exactly one item. File URLs precede bitmap representations, even
        // when the source supplies both (Finder's bitmap can be its icon).
        let type = [UTType.fileURL.identifier, UTType.url.identifier, UTType.png.identifier].first {
            provider.hasItemConformingToTypeIdentifier($0)
        } ?? provider.registeredTypeIdentifiers.first { UTType($0)?.conforms(to: .image) == true }
        guard let type else { busy = false; return false }
        provider.loadDataRepresentation(forTypeIdentifier: type) { data, error in
            Task { @MainActor in
                guard self.request == ticket, host.acceptsHostInput else { return }
                self.busy = false
                if let error { self.error = error.localizedDescription; return }
                guard let data else { self.error = IONoteText.noPasteImage; return }
                if type == UTType.fileURL.identifier || type == UTType.url.identifier {
                    guard let url = URL(dataRepresentation: data, relativeTo: nil) else {
                        self.error = "This image URL can’t be opened. Choose a local image file."; return
                    }
                    if url.isFileURL { self.chooseFile(url, host: host) }
                    else { self.chooseRemoteURL(url, host: host) }
                } else { self.chooseBitmap(data, host: host) }
            }
        }
        return true
    }

    func cancel() {
        request = UUID(); task?.cancel(); task = nil; busy = false; gettingMedia = false
        answerSize(false)
    }
    func answerSize(_ accepted: Bool) {
        sizeWarning = nil
        let continuation = sizeAnswer; sizeAnswer = nil; continuation?.resume(returning: accepted)
    }

    func load(host: FieldEditorHost) async {
        guard let roles = host.draft.occlusionRoles else { return }
        let field = host.draft.values[roles.image]
        guard field != loadedField else { return }
        cancel(); image = nil; loadedField = nil; error = nil
        let expected = request
        guard let name = OcclusionImageField.name(in: field) else {
            loadedField = field
            if host.draft.noteID != nil { error = IONoteText.noImage }
            return
        }
        guard OcclusionImageField.isUsableName(name) else {
            loadedField = field
            error = IONoteText.unusableImage(name); return
        }
        do {
            let snapshot = try await CoreMediaService().trackedNames()
            guard snapshot.generation == host.draft.generation else { return }
            let bytes = try await Task.detached { try MediaFolder.read(name, in: snapshot.root) }.value
            let decoded = try await Task.detached {
                OcclusionImageInput(name: name, bytes: bytes, pixels: try OcclusionImageInput.decode(bytes))
            }.value
            guard request == expected, host.draft.values[roles.image] == field, host.ownsDraft else { return }
            image = decoded; loadedField = field
        } catch {
            guard request == expected, host.draft.values[roles.image] == field else { return }
            loadedField = field
            self.error = (error as? CocoaError)?.code == .fileNoSuchFile
                ? IONoteText.missingImage(name) : IONoteText.unusableImage(name)
        }
    }

    func chooseFile(_ url: URL, host: FieldEditorHost) {
        guard host.acceptsHostInput, !busy else { return }
        guard url.isFileURL, url.host == nil || url.host == "" || url.host == "localhost" else {
            error = IONoteText.unreadableImage(url.lastPathComponent); return
        }
        guard OcclusionImageField.isUsableName(url.lastPathComponent) ||
              UTType(filenameExtension: url.pathExtension)?.conforms(to: .image) == true else {
            error = IONoteText.notImage(url.lastPathComponent); return
        }
        begin(host: host, name: url.lastPathComponent) { asPNG in
            let bytes = try OcclusionImageInput.readFile(url)
            return try OcclusionImageInput.prepare(bytes, name: url.lastPathComponent, asPNG: asPNG)
        }
    }
    func chooseBitmap(_ data: Data, host: FieldEditorHost, photo: Bool = false) {
        let maximum = photo ? host.occlusionPhotoMaximumDimension : nil
        begin(host: host, name: "Image") { asPNG in
            try OcclusionImageInput.prepare(data, name: nil, asPNG: asPNG, maximumDimension: maximum)
        }
    }
    static func isRemoteImageURL(_ url: URL) -> Bool {
        ["http", "https"].contains(url.scheme?.lowercased() ?? "") &&
            OcclusionImageField.extensions.contains { url.absoluteString.lowercased().hasSuffix("." + $0) }
    }
    func chooseRemoteURL(_ url: URL, host: FieldEditorHost) {
        guard host.acceptsHostInput, !busy else { return }
        guard Self.isRemoteImageURL(url), let fetch = host.fetchOcclusionImage else {
            error = "This image URL can’t be opened. Choose a local image file."; return
        }
        begin(host: host, name: url.absoluteString) { asPNG in
            let fetched = try await fetch(url)
            try Task.checkCancellation()
            return try OcclusionImageInput.prepare(fetched.bytes, name: fetched.name, asPNG: asPNG)
        }
        gettingMedia = true
    }
    private func begin(host: FieldEditorHost, name: String,
                       prepare: @escaping @Sendable (Bool) async throws -> OcclusionImageInput) {
        guard host.acceptsHostInput, !busy, let roles = host.draft.occlusionRoles else { return }
        cancel(); busy = true; error = nil
        let expected = request, original = host.draft.values[roles.image]
        let current = { self.request == expected && host.acceptsHostInput && host.draft.values[roles.image] == original }
        task = Task {
            defer { if request == expected { busy = false; gettingMedia = false; task = nil } }
            do {
                let png = (try? await CoreEditorServices().pasteImagesAsPNG()) ?? false
                let preparation = Task.detached { try await prepare(png) }
                let prepared = try await withTaskCancellationHandler {
                    try await preparation.value
                } onCancel: { preparation.cancel() }
                guard current(), !Task.isCancelled else { return }
                if prepared.bytes.isEmpty || prepared.bytes.count > 104_857_600 {
                    let accepted = await withCheckedContinuation { continuation in
                        sizeWarning = prepared.name; sizeWarningBytes = prepared.bytes.count; sizeAnswer = continuation
                    }
                    guard accepted, current(), !Task.isCancelled else { return }
                }
                // Core enforces generation at writer admission, even if the
                // collection changes while this call is queued.
                let stored = try await CoreMediaService(expectedGeneration: host.draft.generation)
                    .addMedia(name: prepared.name, bytes: Array(prepared.bytes))
                guard current(), !Task.isCancelled else { return }
                let field = OcclusionImageField.html(name: stored)
                host.setHostField(field, at: roles.image)
                loadedField = field
                image = OcclusionImageInput(name: stored, bytes: prepared.bytes, pixels: prepared.pixels)
                replacing = false
            } catch {
                guard current(), !Task.isCancelled else { return }
                self.error = IONoteText.unreadableImage(name)
            }
        }
    }
}
