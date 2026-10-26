import CoreBridge
import CardRendering
import WebKit
import Foundation
import Darwin

/// Inert unless the app's guarded launch parser installs it. Owns F11.2 input
/// and evidence only; the existing collection/session owners perform all work.
@MainActor public final class NativeReviewInput {
    nonisolated public struct Request: Sendable, Equatable {
        public let root: URL, caseName: String, platform: String, freezeSHA256: String, freezeBytes: UInt64
        public init(root: URL, caseName: String, platform: String, freezeSHA256: String, freezeBytes: UInt64) {
            self.root = root; self.caseName = caseName; self.platform = platform
            self.freezeSHA256 = freezeSHA256; self.freezeBytes = freezeBytes
        }
    }
    nonisolated public struct Refusal: Error, Sendable, CustomStringConvertible {
        public let reason: String
        public var description: String { "Native review input refused: \(reason)" }
    }
    nonisolated public struct AnswerUse: Codable, Sendable, Equatable {
        public let owner: UUID, coordinator: UUID, session: UInt64, generation: UInt64
        public let deck: Int64, card: Int64, token: [UInt8], rating: String, elapsed: UInt64, wall: Int64, cap: UInt32
    }
    nonisolated struct Media: Codable, Sendable, Equatable {
        let incoming: UInt64, decodedBytes: UInt64, reused: UInt64, added: UInt64, trashed: UInt64
        init(_ value: CollectionImportMedia) {
            incoming = value.incoming; decodedBytes = value.decodedBytes; reused = value.reused
            added = value.added; trashed = value.trashed
        }
    }
    nonisolated struct Inspection: Codable, Sendable {
        let sha256: String, notes: UInt64, cards: UInt64, reviews: UInt64, schema: Int64
        let replacing: Bool, mediaCount: UInt64, mediaBytes: UInt64
        init(_ f: CollectionInspectionFacts) {
            sha256 = f.sourceSHA256.map { String(format: "%02x", $0) }.joined()
            notes = f.notes; cards = f.cards; reviews = f.reviews; schema = f.sourceSchema
            replacing = f.replacingExisting; mediaCount = f.mediaCount; mediaBytes = f.mediaDecodedBytes
        }
    }
    nonisolated struct Activation: Codable, Sendable {
        struct Backup: Codable, Sendable {
            let name: String, size: UInt64, instant: Int64, snapshotModified: Int64
            init(_ b: ImportBackupReceipt) {
                name = b.name; size = b.sizeBytes; instant = b.instantMilliseconds; snapshotModified = b.snapshotModifiedMilliseconds
            }
        }
        let generation: UInt64, backup: Backup?, media: Media, retention: String?
        init(_ a: CollectionActivation) {
            generation = a.generation; backup = a.backup.map(Backup.init); media = Media(a.media); retention = a.mediaRetentionReference
        }
    }
    nonisolated struct RenderBinding: Codable, Sendable, Equatable {
        let coordinator: UUID, session: UInt64, generation: UInt64
        let card: Int64, update: UInt64, side: String, document: UUID
    }
    nonisolated struct ImageEvidence: Codable, Sendable {
        let binding: RenderBinding
        let name: String, width: Int, height: Int
        let x: Double, y: Double, displayWidth: Double, displayHeight: Double
    }
    nonisolated struct AudioEvidence: Codable, Sendable {
        let binding: RenderBinding
        let name: String, index: Int
        var samples: UInt64 = 0
        var firstPosition: Double?, lastPosition: Double?, duration: Double?
        var firstObservedAt: Double?, lastObservedAt: Double?, completedAt: Double?
        var success: Bool = false
    }
    nonisolated struct JournalEvidence: Codable, Sendable {
        struct Name: Codable, Sendable { let name: String, caseKey: String }
        let generation: UInt64, root: String, names: [Name]
    }
    nonisolated struct Receipt: Codable, Sendable {
        let version: Int
        let run: String, platform: String
        let freezeIdentity: NativeReviewIdentity
        let input: NativeReviewIdentity
        let collection: String
        var owner: UUID
        var phase: String
        var seeds = [String: UInt64]()
        var selectedDeck: Int64?
        var selectedDeckName: String?
        var selectionCalls: Int = 0
        var inspection: Inspection?
        var prepared: Media?
        var activation: Activation?
        var admission: AnswerUse?
        var completion: String?
        var failure: String?
        var ended: Bool = false
        var closedFiles: [String: NativeReviewIdentity]?
        var image: ImageEvidence?
        var audio: AudioEvidence?
        var journal: JournalEvidence?
        var reopenedJournal: JournalEvidence?
    }
    public enum FlushStatus: Equatable, Sendable { case idle, pending, saved, failed(String) }
    public private(set) var flushStatus: FlushStatus = .idle
    public let receiptURL: URL
    private let request: Request, collection: CollectionLocation, admittedRoot: URL
    private let identity = UUID()
    private static var current: [URL: UUID] = [:]
    private var loaded: FrozenNativeReviewInput.Loaded?
    private var receipt: Receipt?
    private var loading: Task<FrozenNativeReviewInput.Loaded, any Error>?
    private var persistence: Task<Void, Never>?
    private var persistenceFailure: String?
    private var writeRevision: UInt64 = 0
    private var sessionOwner: (coordinator: UUID, session: UInt64, generation: UInt64)?
    private var reload = false
    private var opened = false
    private var inspectionObject: ObjectIdentifier?
    private var mediaPage: ObjectIdentifier?
    private var mediaBinding: RenderBinding?
    private var mediaTerminations: Int?

    public convenience init(request: Request, collection: CollectionLocation) {
        var root = URL(filePath: #filePath)
        for _ in 0..<7 { root.deleteLastPathComponent() }
        let evidence = URL.applicationSupportDirectory.appending(path: "test-evidence/native-review", directoryHint: .isDirectory)
        self.init(request: request, collection: collection, admittedRoot: root, evidenceRoot: evidence)
    }
    // Internal test seam: production always uses the compiled checkout and
    // private Application Support evidence directory above.
    init(request: Request, collection: CollectionLocation, admittedRoot: URL, evidenceRoot: URL) {
        self.request = request; self.collection = collection; self.admittedRoot = admittedRoot
        receiptURL = evidenceRoot.appending(path: request.caseName).appending(path: request.platform).appending(path: "receipt.json")
        Self.current[collection.directory] = identity
    }
    private func requireCurrent() throws {
        try FrozenNativeReviewInput.check(Self.current[collection.directory] == identity, "stale input owner")
        if persistenceFailure != nil { throw Refusal(reason: "evidence persistence failed") }
        try FrozenNativeReviewInput.check(receipt?.failure == nil, "case already refused")
    }
    /// Checks the sole initial-reset source before FixtureInstaller touches data.
    public func validateInitialReset(from source: URL) throws {
        try requireCurrent()
        try FrozenNativeReviewInput.check(loaded == nil && receipt == nil, "reset after input preparation")
        try FrozenNativeReviewInput.check(request.root == admittedRoot, "reset checkout mismatch")
        let relative = "app/Fixtures/CollectionEntry/EmptyCollection"
        let expected = admittedRoot.appending(path: relative)
        try FrozenNativeReviewInput.check(source.isFileURL && source.path == expected.path, "reset fixture path")
        let directory = try FrozenNativeReviewInput.scoped(admittedRoot, relative)
        let attributes = try directory.resourceValues(forKeys: [.isDirectoryKey])
        try FrozenNativeReviewInput.check(attributes.isDirectory == true, "reset fixture directory")
        try FrozenNativeReviewInput.check(try FileManager.default.contentsOfDirectory(atPath: directory.path) == [".gitkeep"], "reset fixture contents")
        try FrozenNativeReviewInput.check(try FrozenNativeReviewInput.read(directory, ".gitkeep", limit: 0).isEmpty, "reset marker bytes")
        var info = stat()
        let result = lstat(receiptURL.path, &info)
        try FrozenNativeReviewInput.check(result != 0 && errno == ENOENT, "reset refused after receipt")
        _ = try FrozenNativeReviewInput.load(request, admittedRoot: admittedRoot)
    }

    /// Bootstrap validates input before choosing an open seed.
    public func prepare() async throws {
        try requireCurrent()
        guard loaded == nil else { return }
        let pending: Task<FrozenNativeReviewInput.Loaded, any Error>
        if let loading {
            pending = loading
        } else {
            let arguments = request, root = admittedRoot
            pending = Task.detached { try FrozenNativeReviewInput.load(arguments, admittedRoot: root) }
            loading = pending
        }
        let checked = try await pending.value
        try requireCurrent()
        guard loaded == nil else { return }
        let destination = collection, recordPath = receiptURL
        let priorRecord: Task<Receipt?, any Error> = Task.detached {
            let manager = FileManager.default
            let hasDatabase = manager.fileExists(atPath: destination.collectionFile.path)
            let hasRecord = manager.fileExists(atPath: recordPath.path)
            if !hasDatabase {
                try FrozenNativeReviewInput.check(!hasRecord, "record forbids initial reset")
                if manager.fileExists(atPath: destination.directory.path) {
                    let entries = try manager.contentsOfDirectory(atPath: destination.directory.path)
                    if !entries.isEmpty {
                        try FrozenNativeReviewInput.check(entries == [".gitkeep"], "initial directory contents")
                        try FrozenNativeReviewInput.check(try FrozenNativeReviewInput.read(destination.directory, ".gitkeep", limit: 0).isEmpty, "initial marker bytes")
                    }
                }
                return nil
            }
            try FrozenNativeReviewInput.check(hasRecord, "database needs prior record")
            let bytes = try FrozenNativeReviewInput.read(recordPath.deletingLastPathComponent(), recordPath.lastPathComponent, limit: 128 * 1024)
            let saved = try JSONDecoder().decode(Receipt.self, from: bytes)
            try FrozenNativeReviewInput.check(saved.phase == "closed" && saved.ended && saved.completion == "accepted" &&
                saved.admission != nil && saved.failure == nil && saved.closedFiles != nil, "prior record unfinished")
            try FrozenNativeReviewInput.check(try Self.closedFiles(destination.directory) == saved.closedFiles, "closed files changed")
            return saved
        }
        let stored = try await priorRecord.value
        try requireCurrent()
        let freezeIdentity = NativeReviewIdentity(sha256: request.freezeSHA256, bytes: request.freezeBytes)
        if var saved = stored {
            let expected = checked.input
            try FrozenNativeReviewInput.check(saved.version == 1 && saved.run == expected.run && saved.platform == request.platform &&
                saved.freezeIdentity == freezeIdentity &&
                saved.input == checked.identity && saved.collection == collection.directory.path &&
                saved.admission?.card == expected.chosen.card_id && saved.admission?.elapsed == expected.answer.elapsed_ms &&
                saved.admission?.rating == "easy" && saved.admission?.deck == expected.chosen.deck_id &&
                saved.admission?.wall == expected.wall.now_ms && saved.admission?.cap == expected.answer.preset_cap_answer_time_secs &&
                saved.admission?.token.count == 32 && saved.admission?.generation == saved.activation?.generation &&
                saved.seeds["inspection_and_initial_activation"] == expected.seeds.inspection_and_initial_activation &&
                saved.inspection?.sha256 == expected.package.identity.sha256 && saved.activation?.backup == nil &&
                saved.activation?.media.incoming == 3 && saved.activation?.media.added == 3 &&
                saved.activation?.media.reused == 0 && saved.activation?.media.trashed == 0 &&
                saved.activation?.media.decodedBytes == expected.mediaBytes, "prior record mismatch")
            saved.owner = identity
            saved.phase = "relaunch-loaded"
            saved.closedFiles = nil
            receipt = saved
            reload = true
        } else {
            receipt = Receipt(version: 1, run: checked.input.run, platform: request.platform, freezeIdentity: freezeIdentity,
                input: checked.identity, collection: collection.directory.path, owner: identity, phase: "empty")
        }
        loaded = checked
        enqueueReceipt()
    }
    /// Invoked inside the real coordinated source owner, immediately before
    /// CoreCollectionEntryService's startCollectionInspection argument site.
    public func inspectionSeed() async throws -> UInt64 {
        try await recheckFrozen()
        guard let input = loaded?.input, receipt?.phase == "empty", !reload else { throw Refusal(reason: "undeclared/repeated inspection") }
        receipt?.phase = "inspecting"
        receipt?.seeds["inspection_and_initial_activation"] = input.seeds.inspection_and_initial_activation
        enqueueReceipt()
        return input.seeds.inspection_and_initial_activation
    }
    public func inspected(_ inspection: InspectedCollection) throws {
        try inspected(Inspection(inspection.facts), identity: ObjectIdentifier(inspection))
    }
    func inspected(_ facts: Inspection, identity object: ObjectIdentifier) throws {
        try requireCurrent()
        guard let input = loaded?.input, receipt?.phase == "inspecting" else { throw Refusal(reason: "inspection phase") }
        receipt?.inspection = facts; enqueueReceipt()
        try FrozenNativeReviewInput.check(facts.sha256 == input.package.identity.sha256 && !facts.replacing &&
            facts.notes == 1 && facts.cards == 1 && facts.reviews == loaded?.sourceReviews && facts.mediaCount == 3 && facts.mediaBytes == input.mediaBytes, "actual owned package facts")
        inspectionObject = object; receipt?.phase = "inspected"; enqueueReceipt()
    }
    public func willActivate(_ inspection: InspectedCollection) throws {
        try willActivate(identity: ObjectIdentifier(inspection))
    }
    func willActivate(identity object: ObjectIdentifier) throws {
        try requireCurrent()
        try FrozenNativeReviewInput.check(receipt?.phase == "inspected" && inspectionObject == object && !reload, "activation owner/phase")
        receipt?.phase = "activating"; enqueueReceipt()
    }
    public func prepared(_ media: CollectionImportMedia) {
        guard Self.current[collection.directory] == identity, receipt?.phase == "activating" else { return }
        receipt?.prepared = Media(media); enqueueReceipt()
    }
    public func activated(_ activation: CollectionActivation) { activated(Activation(activation)) }
    func activated(_ activation: Activation) {
        // Retain real outcome even if its counts do not satisfy this case.
        guard Self.current[collection.directory] == identity else { return }
        receipt?.activation = activation
        if let input = loaded?.input, receipt?.phase == "activating", activation.backup == nil,
           activation.media.incoming == 3, activation.media.added == 3, activation.media.reused == 0,
           activation.media.trashed == 0, activation.media.decodedBytes == input.mediaBytes {
            receipt?.phase = "activated"
        } else { receipt?.failure = "actual activation differs from declared initial case" }
        enqueueReceipt()
    }
    public func failed(_ error: any Error) {
        guard Self.current[collection.directory] == identity else { return }
        if let entry = error as? CollectionEntryFailure {
            if let activation = entry.completedActivation { receipt?.activation = Activation(activation) }
            if let prepared = entry.preparedMedia { receipt?.prepared = Media(prepared) }
        }
        receipt?.failure = String(describing: error).prefix(1024).description
        enqueueReceipt()
    }
    public func reopenSeed() async throws -> UInt64 {
        try await recheckFrozen()
        guard let input = loaded?.input, reload || receipt?.phase == "closed", !opened else { throw Refusal(reason: "undeclared/repeated reopen") }
        opened = true; receipt?.phase = "reopened"; receipt?.seeds["reopen"] = input.seeds.reopen
        enqueueReceipt(); return input.seeds.reopen
    }
    private func recheckFrozen() async throws {
        try requireCurrent()
        guard let loaded else { throw Refusal(reason: "input not loaded") }
        let request = request, root = admittedRoot
        let checked = try await Task.detached { try FrozenNativeReviewInput.load(request, admittedRoot: root) }.value
        try requireCurrent()
        try FrozenNativeReviewInput.check(checked.identity == loaded.identity, "bound input changed")
    }
    func admitStudy(deck: Int64, name: String?) throws {
        try requireCurrent()
        guard let input = loaded?.input, receipt?.phase == "activated", !reload, sessionOwner == nil,
              deck == input.chosen.deck_id, name == input.chosen.deck_name else { throw Refusal(reason: "study deck/phase") }
    }
    func selecting(deck: Int64, name: String?) throws {
        try admitStudy(deck: deck, name: name)
        guard let count = receipt?.selectionCalls, count < 8 else { throw Refusal(reason: "selection evidence bound") }
        receipt?.selectedDeck = deck; receipt?.selectedDeckName = name; receipt?.selectionCalls = count + 1
        enqueueReceipt()
    }
    func bindSession(coordinator: UUID, session: UInt64, generation: UInt64, deck: Int64) throws {
        try requireCurrent()
        guard let input = loaded?.input, receipt?.phase == "activated", !reload, sessionOwner == nil,
              input.chosen.deck_id == deck, receipt?.activation?.generation == generation else { throw Refusal(reason: "session owner/phase") }
        sessionOwner = (coordinator, session, generation)
    }
    // Fact admission is shared with model tests. Native taps enter through the
    // surface's renderer validation before calling this state transition.
    func admitFacts(coordinator: UUID, session: UInt64, generation: UInt64, deck: Int64, item: ReviewHeadItem,
               rating: ReviewRating, instant: Int64, eligible: Bool) throws -> AnswerUse {
        try requireCurrent()
        guard let input = loaded?.input, receipt?.phase == "activated", receipt?.admission == nil, !reload,
              let owner = sessionOwner, owner.coordinator == coordinator, owner.session == session, owner.generation == generation,
              eligible, deck == input.chosen.deck_id, item.cardId == input.chosen.card_id, rating == .easy,
              instant == input.wall.now_ms, item.stateToken.bytes.count == 32,
              item.homePreset.maximumAnswerSeconds == input.answer.preset_cap_answer_time_secs,
              input.answer.elapsed_ms <= UInt64(item.homePreset.maximumAnswerSeconds) * 1000 else { throw Refusal(reason: "answer owner/head/rating/cap/phase") }
        let use = AnswerUse(owner: identity, coordinator: coordinator, session: session, generation: generation, deck: deck,
            card: item.cardId, token: item.stateToken.bytes, rating: "easy", elapsed: input.answer.elapsed_ms,
            wall: instant, cap: item.homePreset.maximumAnswerSeconds)
        receipt?.admission = use; receipt?.phase = "answer-admitted"; enqueueReceipt()
        return use
    }
    func completed(_ use: AnswerUse, outcome: String) {
        guard Self.current[collection.directory] == identity, receipt?.admission == use, receipt?.phase == "answer-admitted" else { return }
        receipt?.completion = outcome; receipt?.phase = outcome == "accepted" ? "accepted" : "answer-failed"
        enqueueReceipt(snapshot: outcome == "accepted" ? "accepted" : nil)
    }
    func ended(coordinator: UUID, session: UInt64) {
        guard Self.current[collection.directory] == identity, sessionOwner?.coordinator == coordinator, sessionOwner?.session == session,
              receipt?.completion == "accepted" else { return }
        receipt?.ended = true; receipt?.phase = "ended"; enqueueReceipt()
    }
    /// The dev shell supplies the real collection close after normal session end.
    public func closeForEvidence(_ close: @MainActor () async -> Bool) async throws {
        try requireCurrent()
        try FrozenNativeReviewInput.check(receipt?.phase == "ended" && receipt?.ended == true &&
            receipt?.completion == "accepted", "close request before session end")
        guard await close() else { throw Refusal(reason: "collection close failed") }
        await flushEvidence()
        try requireCurrent()
        try FrozenNativeReviewInput.check(receipt?.phase == "closed" && receipt?.closedFiles != nil &&
            flushStatus == .saved, "closed receipt not saved")
    }

    /// Runs after the real core close finalized its DB. Never repairs/reset a
    /// collection. Only receipt files are written by this owner.
    public func closed() async throws {
        try requireCurrent()
        guard receipt?.ended == true, receipt?.completion == "accepted" else { throw Refusal(reason: "close without accepted/end receipt") }
        let directory = collection.directory
        let files = try await Task.detached { try Self.closedFiles(directory) }.value
        try requireCurrent()
        guard let input = loaded?.input else { throw Refusal(reason: "input not loaded") }
        for name in FrozenNativeReviewInput.media {
            try FrozenNativeReviewInput.check(files["collection.media/\(name)"] == input.prepared_files["collection.media/\(name)"], "closed media differs")
        }
        receipt?.closedFiles = files; receipt?.phase = "closed"; enqueueReceipt(snapshot: "closed")
        await flushEvidence()
        try requireCurrent()
    }
    nonisolated private static func closedFiles(_ root: URL) throws -> [String: NativeReviewIdentity] {
        var files: [String: NativeReviewIdentity] = [:]
        for name in ["collection.anki2"] + FrozenNativeReviewInput.media.map({ "collection.media/\($0)" }) {
            files[name] = .of(try FrozenNativeReviewInput.read(root, name))
        }
        for suffix in ["-wal", "-journal"] {
            let name = "collection.anki2" + suffix
            if FileManager.default.fileExists(atPath: root.appending(path: name).path) {
                try FrozenNativeReviewInput.check(try FrozenNativeReviewInput.read(root, name).isEmpty, "unfinalized closed database")
            }
        }
        let media = try FrozenNativeReviewInput.scoped(root, "collection.media")
        try FrozenNativeReviewInput.check(Set(FileManager.default.contentsOfDirectory(atPath: media.path)) == Set(FrozenNativeReviewInput.media), "closed media names")
        return files
    }
    /// Called on the real current renderer after reveal. The DOM identity is
    /// retained inside this evaluation across decode(), never a second page.
    func captureImage(pair: CardPagePair, coordinator: UUID, session: UInt64, generation: UInt64) async throws {
        try requireCurrent()
        guard let input = loaded?.input, let owner = sessionOwner,
              owner.coordinator == coordinator, owner.session == session, owner.generation == generation,
              let update = pair.current.update, update.card == input.chosen.card_id, update.side == .answer,
              pair.current.presented, pair.diagnostics.entries.isEmpty else { throw Refusal(reason: "media renderer binding") }
        let page = pair.current, terminations = page.terminations
        let documentURL = page.webView.url
        let binding = try mediaVisit(pair: pair, coordinator: coordinator, session: session, generation: generation)
        let result = try await page.webView.callAsyncJavaScript("""
            const originalDocument = document;
            const documentKey = Symbol.for('meguri.nativeReview.document');
            if (originalDocument[documentKey] !== undefined && originalDocument[documentKey] !== documentToken)
                throw new Error('native document changed');
            if (originalDocument[documentKey] === undefined)
                Object.defineProperty(originalDocument, documentKey, {value: documentToken});
            const matches = Array.from(document.images).filter(image => image.getAttribute('src') === 'meguri-review.png');
            if (matches.length !== 1) throw new Error('native image count');
            const image = matches[0];
            await image.decode();
            const rect = image.getBoundingClientRect();
            if (document !== originalDocument || originalDocument[documentKey] !== documentToken || !image.isConnected || !image.complete ||
                image.getAttribute('src') !== 'meguri-review.png' || image.naturalWidth !== 4 || image.naturalHeight !== 4 ||
                ![rect.x, rect.y, rect.width, rect.height].every(Number.isFinite) || rect.width <= 0 || rect.height <= 0 ||
                rect.bottom <= 0 || rect.right <= 0 || rect.top >= innerHeight || rect.left >= innerWidth ||
                getComputedStyle(image).visibility !== 'visible' || getComputedStyle(image).display === 'none')
                throw new Error('native image display');
            return {document: originalDocument[documentKey], width: image.naturalWidth, height: image.naturalHeight,
                    x: rect.x, y: rect.y, displayWidth: rect.width, displayHeight: rect.height};
            """, arguments: ["documentToken": binding.document.uuidString], in: nil, contentWorld: .page)
        try requireCurrent()
        guard pair.current === page, page.update == update, page.presented, !page.closed,
              page.terminations == terminations, page.webView.url == documentURL, pair.diagnostics.entries.isEmpty,
              mediaBinding == binding, let value = result as? [String: Any],
              value["document"] as? String == binding.document.uuidString,
              let width = value["width"] as? Int, let height = value["height"] as? Int,
              let x = value["x"] as? Double, let y = value["y"] as? Double,
              let w = value["displayWidth"] as? Double, let h = value["displayHeight"] as? Double,
              width == 4, height == 4, [x, y, w, h].allSatisfy({ $0.isFinite }), w > 0, h > 0 else {
            throw Refusal(reason: "stale/failed native image decode")
        }
        receipt?.image = ImageEvidence(binding: binding, name: "meguri-review.png", width: width, height: height,
            x: x, y: y, displayWidth: w, displayHeight: h)
        enqueueReceipt()
    }
    private func mediaVisit(pair: CardPagePair, coordinator: UUID, session: UInt64, generation: UInt64) throws -> RenderBinding {
        try requireCurrent()
        guard let input = loaded?.input, let owner = sessionOwner,
              owner.coordinator == coordinator, owner.session == session, owner.generation == generation,
              receipt?.phase == "activated", let update = pair.current.update,
              update.card == input.chosen.card_id, update.side == .answer, !pair.current.closed,
              pair.diagnostics.entries.isEmpty else { throw Refusal(reason: "media visit owner/update") }
        if let binding = mediaBinding {
            try FrozenNativeReviewInput.check(mediaPage == ObjectIdentifier(pair.current) &&
                mediaTerminations == pair.current.terminations && binding.update == update.generation &&
                binding.card == update.card && binding.coordinator == coordinator && binding.session == session &&
                binding.generation == generation, "media visit changed")
            return binding
        }
        let binding = RenderBinding(coordinator: coordinator, session: session, generation: generation,
            card: update.card, update: update.generation, side: update.sideName, document: UUID())
        mediaBinding = binding; mediaPage = ObjectIdentifier(pair.current); mediaTerminations = pair.current.terminations
        return binding
    }
    /// Installed before presentation/autoplay by the real coordinator. Keep a
    /// bounded summary of native backend samples and its natural onEnd result.
    func observePlayback(pair: CardPagePair, entry: CardAudioController.Entry,
                         observation: CardAudioController.PlaybackObservation,
                         coordinator: UUID, session: UInt64, generation: UInt64) {
        do {
            let binding = try mediaVisit(pair: pair, coordinator: coordinator, session: session, generation: generation)
            try FrozenNativeReviewInput.check(entry.update == pair.current.update && entry.side == .answer &&
                entry.name == "meguri-review.wav" && entry.index == 0, "actual WAV entry")
            var audio = receipt?.audio ?? AudioEvidence(binding: binding, name: entry.name, index: entry.index)
            try FrozenNativeReviewInput.check(audio.binding == binding && !audio.success, "repeated playback")
            switch observation {
            case .advanced(let position, let duration, let observedAt):
                try FrozenNativeReviewInput.check(position.isFinite && observedAt.isFinite &&
                    position > (audio.lastPosition ?? 0) && observedAt > (audio.lastObservedAt ?? 0) &&
                    duration == 2 && position <= 2, "native playback timeline")
                if audio.samples == 0 { audio.firstPosition = position; audio.firstObservedAt = observedAt }
                audio.samples += 1; audio.lastPosition = position; audio.lastObservedAt = observedAt; audio.duration = duration
            case .completed(let success, let observedAt):
                try FrozenNativeReviewInput.check(success && observedAt.isFinite && audio.samples >= 2 &&
                    observedAt > (audio.lastObservedAt ?? observedAt) && (audio.lastPosition ?? 0) > 1 &&
                    observedAt - (audio.firstObservedAt ?? observedAt) >= 1.5 &&
                    (audio.firstPosition ?? 2) < 0.25 && (audio.lastPosition ?? 0) > 1.75 &&
                    abs(((audio.lastPosition ?? 0) - (audio.firstPosition ?? 0)) -
                        ((audio.lastObservedAt ?? 0) - (audio.firstObservedAt ?? 0))) < 0.5, "natural WAV completion")
                audio.completedAt = observedAt; audio.success = true
            }
            receipt?.audio = audio
            // Sampling never queues hundreds of writes on the UI path.
            if audio.success { enqueueReceipt() }
        } catch { failed(error) }
    }
    func validateMediaCurrent(pair: CardPagePair, coordinator: UUID, session: UInt64, generation: UInt64) throws {
        let binding = try mediaVisit(pair: pair, coordinator: coordinator, session: session, generation: generation)
        try FrozenNativeReviewInput.check(pair.current.presented && mediaReady && receipt?.journal != nil && receipt?.image?.binding == binding,
            "media evidence no longer belongs to current presentation")
    }
    private var mediaReady: Bool {
        guard let image = receipt?.image, let audio = receipt?.audio else { return false }
        return image.binding == audio.binding && audio.success && audio.duration == 2 && audio.samples >= 2
    }
    public func evidenceStatus() async -> String {
        do {
            try requireCurrent()
            let step = receipt?.phase
            if (step == "activated" && receipt?.journal == nil) || (step == "reopened" && receipt?.reopenedJournal == nil) {
                let tracked = try await CoreMediaService().trackedNames()
                try requireCurrent()
                try FrozenNativeReviewInput.check(receipt?.phase == step && tracked.generation > 0 &&
                    tracked.root.path == collection.directory.appending(path: "collection.media").path &&
                    Set(tracked.names.map(\.name)) == Set(FrozenNativeReviewInput.media) && tracked.names.count == 3 &&
                    Set(tracked.names.map(\.caseKey)).count == 3 && tracked.names.allSatisfy({ !$0.caseKey.isEmpty }), "actual initialized media journal")
                let savedNames = JournalEvidence(generation: tracked.generation, root: tracked.root.path,
                    names: tracked.names.sorted { $0.name < $1.name }.map { JournalEvidence.Name(name: $0.name, caseKey: $0.caseKey) })
                if step == "activated", receipt?.journal == nil {
                    try FrozenNativeReviewInput.check(tracked.generation == receipt?.activation?.generation, "import journal generation")
                    receipt?.journal = savedNames; enqueueReceipt(snapshot: "import")
                } else if step == "reopened", receipt?.reopenedJournal == nil {
                    receipt?.reopenedJournal = savedNames; enqueueReceipt(snapshot: "reopened")
                }
            }
        } catch { failed(error) }
        await flushEvidence()
        if receipt?.failure != nil || persistenceFailure != nil { return "failed" }
        guard flushStatus == .saved else { return "failed" }
        if receipt?.phase == "activated" { return mediaReady ? "media-ready" : "imported" }
        return ["accepted", "ended", "reopened"].contains(receipt?.phase ?? "") ? receipt!.phase : "pending"
    }

    /// Bounded evidence/flush seam for later native capture; no product chrome.
    public func flushEvidence() async {
        var revision: UInt64
        repeat {
            revision = writeRevision
            let pending = persistence; await pending?.value
        } while revision != writeRevision
    }
    private func enqueueReceipt(snapshot: String? = nil) {
        guard let receipt, persistenceFailure == nil else { return }
        writeRevision &+= 1
        let revision = writeRevision, previous = persistence, url = receiptURL
        flushStatus = .pending
        persistence = Task { [weak self] in
            await previous?.value
            guard let self, Self.current[self.collection.directory] == self.identity, self.persistenceFailure == nil else { return }
            do {
                try await Task.detached {
                    let parent = url.deletingLastPathComponent(), files = FileManager.default
                    try FrozenNativeReviewInput.check(parent.path == parent.resolvingSymlinksInPath().path, "evidence path links")
                    try files.createDirectory(at: parent, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
                    let attrs = try files.attributesOfItem(atPath: parent.path)
                    try FrozenNativeReviewInput.check((attrs[.ownerAccountID] as? NSNumber)?.uint32Value == getuid(), "evidence owner")
                    if files.fileExists(atPath: url.path) { _ = try FrozenNativeReviewInput.scoped(parent, url.lastPathComponent) }
                    let encoder = JSONEncoder(); encoder.outputFormatting = [.sortedKeys]
                    let data = try encoder.encode(receipt)
                    try FrozenNativeReviewInput.check(data.count <= 128 * 1024, "receipt bound")
                    if let snapshot {
                        try FrozenNativeReviewInput.check(["import", "accepted", "closed", "reopened"].contains(snapshot), "snapshot name")
                        try data.write(to: parent.appending(path: "\(snapshot).json"), options: .withoutOverwriting)
                    }
                    try data.write(to: url, options: .atomic)
                }.value
                if self.writeRevision == revision { self.flushStatus = .saved }
            } catch {
                self.persistenceFailure = String(describing: error)
                self.flushStatus = .failed(String(describing: error))
            }
        }
    }
}
