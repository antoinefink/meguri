#if os(macOS)
import CoreBridge
import CoreFoundation
import Foundation

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
actor MiningCoreCollection {
    func available() async -> UInt64? { try? await CoreCollectionService().waitAvailable(timeoutMilliseconds: 0) }
    func catalog() async throws -> MiningCatalog {
        guard let generation = await available() else { throw MiningFailure("Collection unavailable. Open a collection and try again.") }
        let decks = try await CoreDeckService().allDeckNames()
        let types = try await CoreNotetypeService().notetypeNames()
        guard decks.generation == generation, await available() == generation else { throw MiningFailure("The collection changed. Choose the destination again.") }
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
    private var epoch = UUID()
    private var destination: MiningDestination?
    private var tail: Task<MiningHTTPResponse, Never>?
    private var queued = 0
    private var queuedBytes = 0
    init(collection: MiningCoreCollection = MiningCoreCollection()) { self.collection = collection }
    func configure(enabled: Bool, destination: MiningDestination?) {
        if self.enabled != enabled || self.destination != destination { epoch = UUID() }
        self.enabled = enabled; self.destination = destination
    }
    func drain() async { _ = await tail?.value }
    func handle(_ request: MiningHTTPRequest) async -> MiningHTTPResponse {
        guard enabled else { return .error(503, "Sentence mining is off.") }
        switch request.path {
        case "/srs-check": return .json(200, ["ok": await collection.available() != nil, "version": "4"])
        case "/anki-hello": return .json(200, ["status": "connect", "port": 44432])
        case "/srs-import", "/srs-import-info", "/srs-sample-card": return .error(501, "Importing into Migaku Memory is not supported by Meguri.")
        case "/receive", "/":
            // Actor reentrancy does not serialize an awaited collection write.
            // Chain requests explicitly; keep read-only routes off this FIFO.
            guard queued < 32, queuedBytes + request.body.count <= 268_435_456 else { return .error(503, "Too many pending mining requests. Try again.") }
            queued += 1; queuedBytes += request.body.count
            let previous = tail, token = epoch, selected = destination
            let work = Task { [self] in
                _ = await previous?.value
                return await process(request, token: token, selected: selected)
            }
            tail = work
            let response = await work.value
            queued -= 1; queuedBytes -= request.body.count
            if queued == 0 { tail = nil }
            return response
        default: return .error(404, "Not found.")
        }
    }
    private func process(_ request: MiningHTTPRequest, token: UUID, selected: MiningDestination?) async -> MiningHTTPResponse {
        let root = request.path == "/"
        var version = 6
        do {
            guard enabled, epoch == token, !Task.isCancelled else { throw MiningFailure("Mining settings changed. Send the card again.") }
            if root {
                guard let object = try JSONSerialization.jsonObject(with: request.body) as? [String: Any], let action = object["action"] as? String else { throw MiningFailure("invalid request: action must be a string") }
                if let number = object["version"] {
                    guard let integer = number as? NSNumber, CFGetTypeID(integer) != CFBooleanGetTypeID(), integer.doubleValue.rounded() == integer.doubleValue else { throw MiningFailure("invalid request: version must be an integer") }
                    version = integer.intValue
                } else { version = 4 }
                guard object["params"] == nil || object["params"] is [String: Any] else { throw MiningFailure("invalid request: params must be an object") }
                let params = object["params"] as? [String: Any] ?? [:]
                switch action {
                case "version": return envelope(6, version: version)
                case "deckNames": return envelope(try await collection.catalog().decks.map(\.fullName), version: version)
                case "modelNames": return envelope(try await collection.catalog().types.map(\.name), version: version)
                case "modelFieldNames":
                    let catalog = try await collection.catalog()
                    guard let name = params["modelName"] as? String, let model = catalog.types.first(where: { $0.name == name }) else { throw MiningFailure("model was not found") }
                    return envelope(try await collection.notetype(model.id).fields.map(\.name), version: version)
                case "addNote":
                    guard let note = params["note"] as? [String: Any] else { throw MiningFailure("note must be an object") }
                    let result = try await addRoot(note, token: token, selected: selected)
                    return envelope(result.id, version: version)
                default: throw MiningFailure("unsupported action")
                }
            }
            let payload = try MiningPayload(request.body)
            let (destination, type) = try await resolve(selected)
            let (fields, media) = try payload.fields(for: type)
            let result = try await commit(destination, type: type, fields: fields, media: media,
                key: payload.retryKey, digest: payload.digest, token: token)
            return .json(200, ["success": true, "created": true, "id": result.id])
        } catch {
            let message = (error as? MiningFailure)?.message ?? error.localizedDescription
            if root { return .json(200, ["result": NSNull(), "error": message]) }
            return .error(400, message)
        }
    }
    private func envelope(_ result: Any, version: Int) -> MiningHTTPResponse {
        version <= 4 ? .json(200, result) : .json(200, ["result": result, "error": NSNull()])
    }
    private func resolve(_ selected: MiningDestination?) async throws -> (MiningDestination, NotetypeRecord) {
        guard let selected else { throw MiningFailure("Choose a destination deck and note type in Settings › Integrations.") }
        guard await collection.available() == selected.generation else { throw MiningFailure("Collection unavailable or changed. Check Settings › Integrations.") }
        let catalog = try await collection.catalog()
        guard catalog.generation == selected.generation, catalog.decks.contains(where: { $0.id == selected.deck }),
              catalog.types.contains(where: { $0.id == selected.type }) else { throw MiningFailure("The selected deck or note type is missing. Choose a destination again.") }
        let type = try await collection.notetype(selected.type)
        guard await collection.available() == selected.generation else { throw MiningFailure("The collection changed. Send the card again.") }
        return (selected, type)
    }
    private func commit(_ selected: MiningDestination, type: NotetypeRecord, fields: [String], media: [MiningAttachment],
                        key: String, digest: String, token: UUID, tags: [String] = []) async throws -> NoteMutation {
        guard enabled, epoch == token, destination == selected, !Task.isCancelled else { throw MiningFailure("Mining settings changed. Send the card again.") }
        return try await collection.add(.init(expectedGeneration: selected.generation, notetype: type, fields: fields,
            tags: tags, deckId: selected.deck), key: key, digest: digest, attachments: media)
    }
    private func addRoot(_ note: [String: Any], token: UUID, selected: MiningDestination?) async throws -> NoteMutation {
        let (selected, type) = try await resolve(selected)
        let catalog = try await collection.catalog()
        guard note["deckName"] as? String == catalog.decks.first(where: { $0.id == selected.deck })?.fullName,
              note["modelName"] as? String == type.name else { throw MiningFailure("addNote can only write to the destination selected in Settings › Integrations.") }
        guard let supplied = note["fields"] as? [String: String], !supplied.isEmpty,
              Set(supplied.keys).isSubset(of: Set(type.fields.map(\.name))),
              !supplied.values.contains(where: { MiningPayload.hasRemoteResource($0) || $0.contains("__meguri_mining_attachment_") }) else { throw MiningFailure("invalid or unsupported note fields") }
        for key in ["audio", "picture", "video"] where note[key] != nil {
            guard let items = note[key] as? [Any], items.isEmpty else { throw MiningFailure("Root addNote media is not supported. Use /receive with embedded media.") }
        }
        guard note["tags"] == nil || note["tags"] is [String] else { throw MiningFailure("tags must be strings") }
        let fields = type.fields.map { supplied[$0.name, default: ""] }
        guard fields.contains(where: { !$0.isEmpty }), type.kind == 0, type.originalStockKind != 6 else { throw MiningFailure("Choose a standard note type and supply nonempty fields.") }
        let digest = MiningPayload.hash(try JSONSerialization.data(withJSONObject: note, options: [.sortedKeys]))
        let identity = try JSONEncoder().encode([String(selected.type), String(selected.deck), fields.first ?? ""])
        return try await commit(selected, type: type, fields: fields, media: [], key: MiningPayload.hash(identity), digest: digest,
            token: token, tags: note["tags"] as? [String] ?? [])
    }
}
#endif
