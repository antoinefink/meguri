#if os(macOS)
import CoreBridge
import Foundation
import Observation

/// Device-local Settings owner. Construction never listens; the collection
/// entry path calls firstOpenAttemptEnded after launch has produced its UI.
@Observable public final class SentenceMining {
    public private(set) var enabled: Bool
    public private(set) var status = "Off"
    public private(set) var available = false
    public private(set) var decks: [DeckNameEntry] = []
    public private(set) var types: [NotetypeName] = []
    public private(set) var selectedDeck: Int64
    public private(set) var selectedType: Int64
    public private(set) var fieldMapping: [String] = []
    public private(set) var error: String?
    public let port: UInt16 = 44432
    @ObservationIgnored private let defaults: UserDefaults?
    @ObservationIgnored private let prefix: String
    @ObservationIgnored private let receiver: MiningReceiver
    @ObservationIgnored private var listener: MiningListener?
    @ObservationIgnored private var inert = false
    @ObservationIgnored private var began = false
    @ObservationIgnored private var generation: UInt64?
    @ObservationIgnored private var control: Task<Void, Never>?
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var refreshToken = UUID()
    @ObservationIgnored private var state: MiningListener.State = .off
    @ObservationIgnored public var enabledChanged: ((Bool) -> Void)?
    public init(defaults: UserDefaults?, collectionKey: String) {
        self.defaults = defaults
        prefix = "mining.destination." + MiningPayload.hash(Data(collectionKey.utf8))
        receiver = MiningReceiver()
        enabled = defaults?.bool(forKey: "mining.enabled") ?? false
        selectedDeck = Int64(defaults?.string(forKey: prefix + ".deck") ?? "") ?? 0
        selectedType = Int64(defaults?.string(forKey: prefix + ".type") ?? "") ?? 0
        if enabled { status = "Waiting for collection" }
    }
    public func setEnabled(_ value: Bool) {
        guard value != enabled else { return }
        enabled = value; defaults?.set(value, forKey: "mining.enabled"); enabledChanged?(value)
        updateStatus(); apply()
    }
    public func selectDeck(_ id: Int64) {
        selectedDeck = id; defaults?.set(String(id), forKey: prefix + ".deck"); apply()
    }
    public func selectType(_ id: Int64) {
        selectedType = id; defaults?.set(String(id), forKey: prefix + ".type"); apply(); refresh()
    }
    func firstOpenAttemptEnded() {
        guard !began else { refresh(); return }; began = true
        enabledChanged?(enabled); refresh(); apply()
    }
    func collectionChanged(replaced: Bool = false) {
        if replaced {
            generation = nil; available = false
            // A replacement may reuse deck and model ids for unrelated data.
            selectedDeck = 0; selectedType = 0
            defaults?.removeObject(forKey: prefix + ".deck"); defaults?.removeObject(forKey: prefix + ".type")
            apply()
        }
        if began { refresh() }
    }
    public func refresh() {
        guard !inert else { return }
        refreshTask?.cancel(); let token = UUID(); refreshToken = token
        refreshTask = Task { [weak self] in
            guard let self else { return }
            do {
                let catalog = try await receiver.collection.catalog()
                guard !Task.isCancelled, refreshToken == token else { return }
                generation = catalog.generation; decks = catalog.decks; types = catalog.types; available = true; error = nil
                // Persisted selections are kept only if still present. Do not
                // silently redirect an incoming card to a different destination.
                if !decks.contains(where: { $0.id == selectedDeck }) { selectedDeck = 0 }
                if !types.contains(where: { $0.id == selectedType }) { selectedType = 0 }
                if selectedType != 0 {
                    let type = try await receiver.collection.notetype(selectedType)
                    guard !Task.isCancelled, refreshToken == token else { return }
                    let map = MiningFieldMap.infer(type.fields.map(\.name))
                    let defaults = MiningFieldMap.defaults(typeName: type.name, fields: type.fields.map(\.name))
                    fieldMapping = type.fields.indices.compactMap { index in
                        if let source = map[index] { return "\(type.fields[index].name) ← \(source)" }
                        if !defaults[index].isEmpty { return "\(type.fields[index].name) ← Default “\(defaults[index])”" }
                        return nil
                    }
                    if type.kind != 0 || type.originalStockKind == 6 { error = "Choose a standard note type. Cloze and image occlusion need the editor." }
                    else if fieldMapping.isEmpty { error = "This note type has no recognized fields. Choose one with a Sentence or Front field." }
                } else { fieldMapping = [] }
                apply()
            } catch {
                guard !Task.isCancelled, refreshToken == token else { return }
                available = false; self.error = "Collection unavailable. Open a collection to choose a destination."
                updateStatus()
            }
        }
    }
    private func apply() {
        guard !inert else { updateStatus(); return }
        let previous = control
        let value = enabled && began
        let destination = generation.flatMap { generation in
            selectedDeck != 0 && selectedType != 0 ? MiningDestination(generation: generation, deck: selectedDeck, type: selectedType) : nil
        }
        control = Task { [weak self] in
            _ = await previous?.value
            guard let self else { return }
            await receiver.configure(enabled: value, destination: destination)
            if value {
                if listener == nil {
                    let receiver = self.receiver
                    listener = MiningListener(handler: { await receiver.handle($0) }, changed: { [weak self] state in
                        Task { @MainActor [weak self] in self?.state = state; self?.updateStatus() }
                    })
                }
                listener?.start()
            } else { await listener?.stop() }
            updateStatus()
        }
    }
    private func updateStatus() {
        guard enabled else { status = "Off"; return }
        switch state {
        case .off: status = began ? "Starting…" : "Waiting for collection"
        case .starting: status = "Starting…"
        case .listening: status = available ? "Listening on 127.0.0.1:\(port)" : "Listening · Collection unavailable"
        case .portInUse: status = "Port \(port) is in use. Quit Anki or the other app using this port. Meguri retries automatically."
        }
    }
    func resumeAfterRefusedQuit() { apply(); refresh() }
    func shutdown() async {
        refreshTask?.cancel()
        _ = await control?.value
        await receiver.configure(enabled: false, destination: nil)
        await listener?.stop()
        await receiver.drain()
    }
    /// No listener, defaults or collection is accessed by this synthetic
    /// production-pane fixture. The coordinator captures it in the test VM.
    public static func renderFixture(portConflict: Bool = false) -> SentenceMining {
        let model = SentenceMining(defaults: nil, collectionKey: "synthetic")
        model.inert = true
        model.enabled = true; model.began = true; model.available = true
        model.decks = [.init(id: 1, fullName: "Japanese::Sentences", filtered: false)]
        model.types = [.init(id: 2, name: "Imported Sentence Cards")]
        model.selectedDeck = 1; model.selectedType = 2
        model.fieldMapping = ["Sentence ← sentence", "Translation ← translation", "Sentence Audio ← sentenceAudio", "Screenshot ← firstImage"]
        model.state = portConflict ? .portInUse : .listening(44432); model.updateStatus()
        return model
    }
}
#endif
