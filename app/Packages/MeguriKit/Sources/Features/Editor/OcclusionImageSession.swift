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
    private nonisolated struct FetchFailure: Error { let cause: String }
    private(set) var image: OcclusionImageInput?
    private(set) var loadedField: String?
    private(set) var busy = false
    private(set) var gettingMedia = false
    var error: String?
    var fetchFailure: String?
    var replacing = false
    var startsNewMasks = false
    var sizeWarning: String?
    private(set) var sizeWarningBytes = 0
    private var sizeAnswer: CheckedContinuation<Bool, Never>?
    private var task: Task<Void, Never>?
    private var request = UUID()
    private var sourceRequest: ImageSourceRequest?
    private var providerProgress: Progress?
    private var providerCandidates: [(NSItemProvider, String)] = []
    var completion: Task<Void, Never>? { task }

    func reserveSource(host: FieldEditorHost) -> UUID? {
        guard host.acceptsHostInput, !busy else { return nil }
        cancel(); busy = true; error = nil
        let source = ImageSourceRequest()
        sourceRequest = source
        watchDraft(host, source: source)
        return request
    }
    func receiveBitmap(_ data: Data?, ticket: UUID, host: FieldEditorHost, photo: Bool = false) {
        guard request == ticket, sourceRequest?.isCancelled == false, host.acceptsHostInput else { return }
        busy = false
        if let data { chooseBitmap(data, host: host, photo: photo) }
        else { replacing = false }
    }
    func receiveFile(_ url: URL?, ticket: UUID, host: FieldEditorHost) {
        guard request == ticket, sourceRequest?.isCancelled == false, host.acceptsHostInput else { return }
        busy = false
        if let url { chooseFile(url, host: host) } else { replacing = false }
    }
    func loadBitmap(ticket: UUID, host: FieldEditorHost,
                    load: @escaping @Sendable () async throws -> Data?) {
        guard request == ticket, sourceRequest?.isCancelled == false else { return }
        task = Task {
            do {
                let bytes = try await load()
                guard request == ticket, !Task.isCancelled else { return }
                task = nil
                receiveBitmap(bytes, ticket: ticket, host: host, photo: true)
            } catch {
                guard request == ticket, !Task.isCancelled else { return }
                task = nil
                sourceFailed(error.localizedDescription, ticket: ticket, host: host)
            }
        }
    }
    func sourceFailed(_ message: String, ticket: UUID, host: FieldEditorHost) {
        guard request == ticket, sourceRequest?.isCancelled == false, host.ownsDraft else { return }
        busy = false; error = message
    }

    func chooseProvider(_ providers: [NSItemProvider], host: FieldEditorHost) -> Bool {
        // The first usable item, with file URL, eligible web URL, then bitmap
        // precedence within each item. An unrelated URL must not hide pixels.
        let candidates = providers.flatMap { provider in
            let images = provider.registeredTypeIdentifiers.filter {
                UTType($0)?.conforms(to: .image) == true && $0 != UTType.png.identifier
            }
            return ([UTType.fileURL.identifier, UTType.url.identifier, UTType.png.identifier] + images)
                .filter { provider.hasItemConformingToTypeIdentifier($0) }.map { (provider, $0) }
        }
        guard !candidates.isEmpty, let ticket = reserveSource(host: host) else { return false }
        providerCandidates = candidates
        loadProvider(at: 0, ticket: ticket, host: host)
        return true
    }

    private func loadProvider(at index: Int,
                              ticket: UUID, host: FieldEditorHost) {
        guard index < providerCandidates.count else { busy = false; error = IONoteText.noPasteImage; return }
        let (provider, type) = providerCandidates[index]
        providerProgress = provider.loadDataRepresentation(forTypeIdentifier: type) { data, _ in
            Task { @MainActor in
                guard self.request == ticket, self.sourceRequest?.isCancelled == false, host.acceptsHostInput else { return }
                self.providerProgress = nil
                guard let data else {
                    self.loadProvider(at: index + 1, ticket: ticket, host: host); return
                }
                if type == UTType.fileURL.identifier || type == UTType.url.identifier {
                    guard let url = URL(dataRepresentation: data, relativeTo: nil),
                          url.isFileURL || Self.isRemoteImageURL(url) else {
                        self.loadProvider(at: index + 1, ticket: ticket, host: host); return
                    }
                    self.busy = false
                    if url.isFileURL { self.chooseFile(url, host: host) }
                    else { self.chooseRemoteURL(url, host: host) }
                } else { self.busy = false; self.chooseBitmap(data, host: host) }
            }
        }
    }

    func cancel() {
        sourceRequest?.cancel(); sourceRequest = nil
        providerProgress?.cancel(); providerProgress = nil
        providerCandidates = []
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
                OcclusionImageInput(name: name, bytes: bytes, pixels: try await OcclusionImageInput.decode(bytes))
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
            return try await OcclusionImageInput.prepare(bytes, name: url.lastPathComponent, asPNG: asPNG)
        }
    }
    func chooseBitmap(_ data: Data, host: FieldEditorHost, photo: Bool = false) {
        let maximum = photo ? host.occlusionPhotoMaximumDimension : nil
        begin(host: host, name: "Image") { asPNG in
            try await OcclusionImageInput.prepare(data, name: nil, asPNG: asPNG, maximumDimension: maximum)
        }
    }
    static func isRemoteImageURL(_ url: URL) -> Bool {
        ["http", "https"].contains(url.scheme?.lowercased() ?? "") &&
            OcclusionImageField.extensions.contains { url.absoluteString.lowercased().hasSuffix("." + $0) }
    }
    func chooseRemoteURL(_ url: URL, host: FieldEditorHost) {
        guard host.acceptsHostInput, !busy else { return }
        guard Self.isRemoteImageURL(url) else {
            error = "This image URL can’t be opened. Choose a local image file."; return
        }
        let source = ImageSourceRequest()
        begin(host: host, name: url.absoluteString, source: source) { asPNG in
            let fetched: FetchedEditorImage
            do { fetched = try await source.fetch(url) }
            catch {
                try Task.checkCancellation()
                if let bridge = error as? BridgeError, case .unavailable(let reason) = bridge {
                    throw FetchFailure(cause: reason)
                }
                throw FetchFailure(cause: error.localizedDescription)
            }
            try Task.checkCancellation()
            return try await OcclusionImageInput.prepare(fetched.bytes, name: fetched.name, asPNG: asPNG)
        }
        gettingMedia = true
    }
    private func begin(host: FieldEditorHost, name: String,
                       source: ImageSourceRequest = ImageSourceRequest(),
                       prepare: @escaping @Sendable (Bool) async throws -> OcclusionImageInput) {
        guard host.acceptsHostInput, !busy, let roles = host.draft.occlusionRoles else { return }
        cancel(); busy = true; error = nil; fetchFailure = nil; sourceRequest = source
        watchDraft(host, source: source)
        let expected = request, original = host.draft.values[roles.image]
        let current = { self.request == expected && !source.isCancelled && host.acceptsHostInput && host.draft.values[roles.image] == original }
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
                let stored = try await source.add(name: prepared.name, bytes: prepared.bytes, generation: host.draft.generation)
                guard current(), !Task.isCancelled else { return }
                sourceRequest = nil
                let field = OcclusionImageField.html(name: stored)
                host.setHostField(field, at: roles.image)
                loadedField = field
                image = OcclusionImageInput(name: stored, bytes: prepared.bytes, pixels: prepared.pixels)
                replacing = false
            } catch {
                guard current(), !Task.isCancelled else { return }
                if let failure = error as? FetchFailure {
                    fetchFailure = name + "\n" + failure.cause
                } else { self.error = IONoteText.unreadableImage(name) }
            }
        }
    }

    private func watchDraft(_ host: FieldEditorHost, source: ImageSourceRequest) {
        // Observation invalidates synchronously, before a changed draft can
        // admit an already queued writer. UI completion checks alone are late.
        withObservationTracking {
            _ = host.acceptsHostInput
            _ = host.draft.notetype
            _ = host.draft.values
        } onChange: { [weak self] in
            source.cancel()
            Task { @MainActor [weak self] in
                guard self?.sourceRequest === source else { return }
                self?.cancel()
            }
        }
    }
}
