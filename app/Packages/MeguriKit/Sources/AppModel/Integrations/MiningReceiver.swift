#if os(macOS)
import CoreBridge
import CoreFoundation
import Foundation
import Synchronization

nonisolated struct MiningCatalog: Sendable {
    var generation: UInt64
    var decks: [DeckNameEntry]
    var types: [NotetypeName]
}
nonisolated struct MiningDestination: Sendable, Equatable {
    var generation: UInt64
    var deck: Int64
    var type: Int64
}
/// Shared with the Settings owner so admission changes before any actor hop.
nonisolated final class MiningAdmission: Sendable {
    private nonisolated struct State: Sendable {
        var token = UUID()
        var listenerEpoch = UUID()
        var enabled = false
    }
    private let state = Mutex(State())

    func update(enabled: Bool, retiringListener: Bool = true) -> UUID {
        state.withLock {
            $0.token = UUID()
            if retiringListener { $0.listenerEpoch = UUID() }
            $0.enabled = enabled
            return $0.token
        }
    }

    func matches(_ token: UUID, requiringEnabled: Bool = true) -> Bool {
        state.withLock { $0.token == token && (!requiringEnabled || $0.enabled) }
    }

    func current() -> UUID? {
        state.withLock { $0.enabled ? $0.token : nil }
    }

    var listenerEpoch: UUID { state.withLock { $0.listenerEpoch } }

    func current(for listenerEpoch: UUID) -> UUID? {
        state.withLock { $0.enabled && $0.listenerEpoch == listenerEpoch ? $0.token : nil }
    }
}

actor MiningCoreCollection {
    func available(timeoutMilliseconds: UInt32 = 0) async -> UInt64? {
        try? await CoreCollectionService().waitAvailable(timeoutMilliseconds: timeoutMilliseconds)
    }

    func catalog() async throws -> MiningCatalog {
        guard let generation = await available() else {
            throw MiningFailure("Collection unavailable. Open a collection and try again.")
        }
        let decks = try await CoreDeckService().allDeckNames()
        let types = try await CoreNotetypeService().notetypeNames()
        guard decks.generation == generation, await available() == generation else {
            throw MiningFailure("The collection changed. Choose the destination again.")
        }
        return .init(generation: generation, decks: decks.decks.filter { !$0.filtered }, types: types)
    }

    func notetype(_ id: Int64) async throws -> NotetypeRecord { try await CoreNotetypeService().readNotetype(id: id) }
    func add(_ request: AddScreenNoteRequest, key: String, digest: String, attachments: [MiningAttachment]) async throws -> NoteMutation {
        try await CoreMiningService().add(request, retryKey: key, payloadDigest: digest, attachments: attachments)
    }
}

actor MiningReceiver {
    let collection: MiningCoreCollection
    private var enabled = false
    private var destination: MiningDestination?
    nonisolated let admission = MiningAdmission()
    private nonisolated struct Pending: Sendable {
        let request: MiningHTTPRequest
        let admissionToken: UUID
        let selected: MiningDestination?
        let root: Result<RootAction, MiningFailure>?
        let isCurrent: @Sendable () -> Bool
        let reply: @Sendable (MiningHTTPResponse) -> Void
    }
    // JSONSerialization supplies immutable values; the parsed envelope is
    // retained once and only read by the receiver's sequential worker.
    private nonisolated struct RootAction: @unchecked Sendable {
        let name: String
        let version: Int
        let params: [String: Any]

        init(_ data: Data) throws {
            guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let action = object["action"] as? String else {
                throw MiningFailure("invalid request: action must be a string")
            }
            name = action
            if let number = object["version"] {
                guard let integer = number as? NSNumber, CFGetTypeID(integer) != CFBooleanGetTypeID(),
                      integer.doubleValue.rounded() == integer.doubleValue else {
                    throw MiningFailure("invalid request: version must be an integer")
                }
                version = integer.intValue
            } else { version = 4 }
            guard object["params"] == nil || object["params"] is [String: Any] else {
                throw MiningFailure("invalid request: params must be an object")
            }
            params = object["params"] as? [String: Any] ?? [:]
        }
    }
    private nonisolated struct Incoming: Sendable {
        let request: MiningHTTPRequest
        let token: UUID?
        let isCurrent: @Sendable () -> Bool
        let reply: @Sendable (MiningHTTPResponse) -> Void
    }
    private nonisolated let incoming: AsyncStream<Incoming>.Continuation
    private var pending: [Pending] = []
    private let pendingLimit: Int // Zero capacity exercises saturation without a Core write.
    private var executing: Task<Void, Never>?
    private var queuedBytes = 0
    private var configuredAdmissionToken: UUID?
    private var writing = false
    init(collection: MiningCoreCollection = MiningCoreCollection(), pendingLimit: Int = 32) {
        self.collection = collection
        self.pendingLimit = pendingLimit
        let channel = AsyncStream<Incoming>.makeStream()
        incoming = channel.continuation
        Task { [weak self] in
            for await work in channel.stream {
                guard let self else { work.reply(.error(503, "Sentence mining is off.")); continue }
                await self.enqueue(work)
            }
        }
    }
    deinit { incoming.finish() }

    /// Synchronous registration preserves the listener's body-completion order.
    /// The consumer only admits work; it never waits for a collection write.
    nonisolated func launch(_ request: MiningHTTPRequest, admissionToken: UUID? = nil,
        isCurrent: @escaping @Sendable () -> Bool = { true }) -> Task<MiningHTTPResponse, Never> {
        let response = AsyncStream<MiningHTTPResponse>.makeStream()
        incoming.yield(Incoming(request: request, token: admissionToken, isCurrent: isCurrent, reply: {
            response.continuation.yield($0)
            response.continuation.finish()
        }))
        return Task {
            for await value in response.stream { return value }
            return .error(503, "Sentence mining is off.")
        }
    }
    @discardableResult
    func configure(enabled: Bool, destination: MiningDestination?, admissionToken: UUID? = nil) -> Bool {
        let currentToken: UUID
        if let admissionToken {
            guard admission.matches(admissionToken, requiringEnabled: false) else {
                return false
            }
            currentToken = admissionToken
        } else {
            currentToken = admission.update(enabled: enabled)
        }
        let changed = configuredAdmissionToken != currentToken
        if changed {
            retirePending()
            if !writing { executing?.cancel() }
        }
        configuredAdmissionToken = currentToken
        self.enabled = enabled
        self.destination = destination
        return changed
    }

    private func retirePending() {
        let retired = pending
        pending.removeAll()
        queuedBytes = 0
        for work in retired {
            work.reply(.error(503, "Mining settings changed. Send the card again."))
        }
    }

    func drain() async {
        while let executing { await executing.value }
    }

    private func startNext() {
        guard executing == nil, !pending.isEmpty else {
            return
        }
        let work = pending.removeFirst()
        queuedBytes -= work.request.body.count
        executing = Task {
            let response = await process(work.request, root: work.root, admissionToken: work.admissionToken, selected: work.selected, isCurrent: work.isCurrent)
            work.reply(response)
            executing = nil
            startNext()
        }
    }

    func handle(_ request: MiningHTTPRequest, admissionToken: UUID? = nil) async -> MiningHTTPResponse {
        await launch(request, admissionToken: admissionToken).value
    }

    private func enqueue(_ work: Incoming) {
        let request = work.request
        guard enabled, work.isCurrent(), let admissionToken = work.token ?? admission.current(),
              admission.matches(admissionToken), configuredAdmissionToken == admissionToken else {
            work.reply(.error(503, "Sentence mining is off."))
            return
        }
        switch request.path {
        case "/srs-check":
            Task { work.reply(.json(200, ["ok": await collection.available() != nil, "version": "4"])) }
        case "/anki-hello": work.reply(.json(200, ["status": "connect", "port": Int(request.localPort)]))
        case "/srs-import", "/srs-import-info", "/srs-sample-card": work.reply(.error(501, "Importing into Migaku Memory is not supported by Meguri."))
        case "/receive", "/":
            let root: Result<RootAction, MiningFailure>? = request.path == "/" ? Result { try RootAction(request.body) }.mapError {
                ($0 as? MiningFailure) ?? MiningFailure($0.localizedDescription)
            } : nil
            if case .success(let action)? = root, action.name == "version" {
                work.reply(envelope(6, version: action.version))
                return
            }
            guard pending.count < pendingLimit, queuedBytes + request.body.count <= 268_435_456 else {
                work.reply(.error(503, "Too many pending mining requests. Try again."))
                return
            }
            pending.append(Pending(request: request, admissionToken: admissionToken,
                selected: destination, root: root, isCurrent: work.isCurrent, reply: work.reply))
            queuedBytes += request.body.count
            startNext()
        default: work.reply(.error(404, "Not found."))
        }
    }

    private func process(_ request: MiningHTTPRequest, root parsedRoot: Result<RootAction, MiningFailure>?, admissionToken: UUID, selected: MiningDestination?, isCurrent: @Sendable () -> Bool) async -> MiningHTTPResponse {
        let root = request.path == "/"
        do {
            guard enabled, isCurrent(), admission.matches(admissionToken), !Task.isCancelled else {
                throw MiningFailure("Mining settings changed. Send the card again.")
            }
            if let parsedRoot {
                let action = try parsedRoot.get()
                let version = action.version
                let params = action.params
                switch action.name {
                case "deckNames": return envelope(try await collection.catalog().decks.map(\.fullName), version: version)
                case "modelNames": return envelope(try await collection.catalog().types.map(\.name), version: version)
                case "modelFieldNames":
                    let catalog = try await collection.catalog()
                    guard let name = params["modelName"] as? String, let model = catalog.types.first(where: { $0.name == name }) else {
                        throw MiningFailure("model was not found")
                    }
                    let type = try await collection.notetype(model.id)
                    guard await collection.available() == catalog.generation else {
                        throw MiningFailure("The collection changed. Request model fields again.")
                    }
                    return envelope(type.fields.map(\.name), version: version)
                case "addNote":
                    guard let note = params["note"] as? [String: Any] else {
                        throw MiningFailure("note must be an object")
                    }
                    let result = try await addRoot(note, admissionToken: admissionToken, selected: selected, isCurrent: isCurrent)
                    return envelope(result.id, version: version)
                default: throw MiningFailure("unsupported action")
                }
            }
            let payload = try MiningPayload(request.body)
            let (destination, type) = try await resolve(selected)
            let (fields, media) = try payload.fields(for: type)
            let result = try await commit(destination, type: type, fields: fields, media: media,
                key: payload.retryKey, digest: payload.digest, admissionToken: admissionToken, isCurrent: isCurrent)
            return .json(200, ["success": true, "created": true, "id": result.id])
        } catch {
            let message: String
            if case BridgeError.invalidArgument(let detail) = error {
                message = detail
            } else {
                // Deliberate public errors (including MiningFailure) expose
                // their message through LocalizedError. Core internal errors
                // and arbitrary NSError details must stay private.
                message = (error as? any LocalizedError)?.errorDescription ?? "Meguri could not add the card."
            }
            if root { return .json(200, ["result": NSNull(), "error": message]) }
            return .error(400, message)
        }
    }

    private func envelope(_ result: Any, version: Int) -> MiningHTTPResponse {
        version <= 4 ? .json(200, result) : .json(200, ["result": result, "error": NSNull()])
    }

    private func resolve(_ selected: MiningDestination?) async throws -> (MiningDestination, NotetypeRecord) {
        guard let selected else {
            throw MiningFailure("Choose a destination deck and note type in Settings › Integrations.")
        }
        guard await collection.available(timeoutMilliseconds: 5_000) == selected.generation else {
            throw MiningFailure("Collection unavailable or changed. Check Settings › Integrations.")
        }
        let catalog = try await collection.catalog()
        guard catalog.generation == selected.generation, catalog.decks.contains(where: { $0.id == selected.deck }),
              catalog.types.contains(where: { $0.id == selected.type }) else {
            throw MiningFailure("The selected deck or note type is missing. Choose a destination again.")
        }
        let type = try await collection.notetype(selected.type)
        guard await collection.available() == selected.generation else {
            throw MiningFailure("The collection changed. Send the card again.")
        }
        return (selected, type)
    }

    private func commit(_ selected: MiningDestination, type: NotetypeRecord, fields: [String], media: [MiningAttachment],
                        key: String, digest: String, admissionToken: UUID, isCurrent: @Sendable () -> Bool, tags: [String] = []) async throws -> NoteMutation {
        guard enabled, isCurrent(), admission.matches(admissionToken), destination == selected, !Task.isCancelled else {
            throw MiningFailure("Mining settings changed. Send the card again.")
        }
        writing = true
        defer { writing = false }
        return try await collection.add(.init(expectedGeneration: selected.generation, notetype: type, fields: fields,
            tags: tags, deckId: selected.deck), key: key, digest: digest, attachments: media)
    }

    private func addRoot(_ note: [String: Any], admissionToken: UUID, selected: MiningDestination?, isCurrent: @Sendable () -> Bool) async throws -> NoteMutation {
        let (selected, type) = try await resolve(selected)
        let catalog = try await collection.catalog()
        guard note["deckName"] as? String == catalog.decks.first(where: { $0.id == selected.deck })?.fullName,
              note["modelName"] as? String == type.name else {
            throw MiningFailure("addNote can only write to the destination selected in Settings › Integrations.")
        }
        guard let supplied = note["fields"] as? [String: String], !supplied.isEmpty,
              Set(supplied.keys).isSubset(of: Set(type.fields.map(\.name))),
              !supplied.values.contains(where: { $0.contains("__meguri_mining_attachment_") }) else {
            throw MiningFailure("invalid or unsupported note fields")
        }
        for key in ["audio", "picture", "video"] where note[key] != nil {
            guard let items = note[key] as? [Any], items.isEmpty else {
                throw MiningFailure("Root addNote media is not supported. Use /receive with embedded media.")
            }
        }
        guard note["tags"] == nil || note["tags"] is [String] else {
            throw MiningFailure("tags must be strings")
        }
        let fields = try type.fields.map { field in
            guard let html = MiningPayload.studyHTML(supplied[field.name, default: ""]) else {
                throw MiningFailure("invalid or unsupported note fields")
            }
            return html
        }
        guard fields.contains(where: { !$0.isEmpty }), type.kind == 0, type.originalStockKind != 6 else {
            throw MiningFailure("Choose a standard note type and supply nonempty fields.")
        }
        let digest = MiningPayload.hash(try JSONSerialization.data(withJSONObject: note, options: [.sortedKeys]))
        let identity = try JSONEncoder().encode([String(selected.type), String(selected.deck), fields.first ?? ""])
        return try await commit(selected, type: type, fields: fields, media: [], key: MiningPayload.hash(identity), digest: digest,
            admissionToken: admissionToken, isCurrent: isCurrent, tags: note["tags"] as? [String] ?? [])
    }
}
#endif
