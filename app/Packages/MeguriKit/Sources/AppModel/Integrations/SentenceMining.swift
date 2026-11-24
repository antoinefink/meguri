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
    public private(set) var port: UInt16
    public private(set) var configuredPort: UInt16
    @ObservationIgnored private let defaults: UserDefaults?
    @ObservationIgnored private let prefix: String
    @ObservationIgnored private let receiver: MiningReceiver
    @ObservationIgnored private var listener: MiningListener?
    @ObservationIgnored private var listenerStatus: Task<Void, Never>?
    @ObservationIgnored private var inert = false
    @ObservationIgnored private var began = false
    @ObservationIgnored private var suspended = false
    @ObservationIgnored private var shutdownTask: Task<Void, Never>?
    @ObservationIgnored private var generation: UInt64?
    @ObservationIgnored private var appliedEnabled: Bool?
    @ObservationIgnored private var appliedPort: UInt16?
    @ObservationIgnored private var appliedDestination: MiningDestination?
    @ObservationIgnored private var controlToken = UUID()
    @ObservationIgnored private var listenerToken = UUID()
    @ObservationIgnored private var control: Task<Void, Never>?
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var refreshToken = UUID()
    @ObservationIgnored private var state: MiningListener.State = .off
    @ObservationIgnored public var enabledChanged: ((Bool) -> Void)?
    /// An explicit port is a launch/test override; only tests use zero.
    public init(defaults: UserDefaults?, collectionKey: String, port: UInt16? = nil) {
        self.defaults = defaults
        let selectedPort = port ?? Self.validPort(defaults?.string(forKey: "mining.port") ?? "") ?? 44432
        self.port = selectedPort
        configuredPort = selectedPort
        prefix = "mining.destination." + MiningPayload.hash(Data(collectionKey.utf8))
        receiver = MiningReceiver()
        enabled = defaults?.bool(forKey: "mining.enabled") ?? false
        selectedDeck = Int64(defaults?.string(forKey: prefix + ".deck") ?? "") ?? 0
        selectedType = Int64(defaults?.string(forKey: prefix + ".type") ?? "") ?? 0
        updateStatus()
    }

    private static func validPort(_ text: String) -> UInt16? {
        guard !text.isEmpty, text.utf8.allSatisfy({ (48...57).contains($0) }),
              let value = UInt16(text), value >= 1024 else { return nil }
        return value
    }

    /// Drafts never affect admission. A committed change uses the same
    /// synchronous fence and serialized control path as the receiving toggle.
    @discardableResult public func applyPort(_ text: String) -> Bool {
        guard let value = Self.validPort(text) else { return false }
        guard value != configuredPort else { return true }
        configuredPort = value
        port = value
        defaults?.set(Int(value), forKey: "mining.port")
        apply()
        return true
    }

    public func setEnabled(_ value: Bool) {
        guard value != enabled else {
            return
        }
        enabled = value
        defaults?.set(value, forKey: "mining.enabled")
        enabledChanged?(value)
        updateStatus()
        apply()
    }

    public func selectDeck(_ id: Int64) {
        selectedDeck = id
        defaults?.set(String(id), forKey: prefix + ".deck")
        apply()
    }

    public func selectType(_ id: Int64) {
        selectedType = id
        defaults?.set(String(id), forKey: prefix + ".type")
        apply()
        refresh()
    }

    func firstOpenAttemptEnded() {
        guard !began else {
            refresh()
            return
        }
        began = true
        enabledChanged?(enabled)
        refresh()
        apply()
    }

    func collectionChanged(replaced: Bool = false) {
        if replaced {
            generation = nil
            available = false
            // A replacement may reuse deck and model ids for unrelated data.
            selectedDeck = 0
            selectedType = 0
            defaults?.removeObject(forKey: prefix + ".deck")
            defaults?.removeObject(forKey: prefix + ".type")
            apply()
        }
        if began { refresh() }
    }

    func collectionAvailable() {
        guard began, !suspended else { return }
        refresh()
    }

    public func refresh() {
        guard !inert, !suspended else {
            return
        }
        refreshTask?.cancel()
        let token = UUID()
        refreshToken = token
        refreshTask = Task { [weak self] in
            guard let self else {
                return
            }
            do {
                let catalog = try await receiver.collection.catalog()
                guard !Task.isCancelled, refreshToken == token else {
                    return
                }
                generation = catalog.generation
                decks = catalog.decks
                types = catalog.types
                available = true
                error = nil
                // Persisted selections are kept only if still present. Do not
                // silently redirect an incoming card to a different destination.
                if !decks.contains(where: { $0.id == selectedDeck }) { selectedDeck = 0 }
                if !types.contains(where: { $0.id == selectedType }) { selectedType = 0 }
                if selectedType != 0 {
                    let type = try await receiver.collection.notetype(selectedType)
                    guard !Task.isCancelled, refreshToken == token else {
                        return
                    }
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
                guard !Task.isCancelled, refreshToken == token else {
                    return
                }
                available = false
                self.error = "Collection unavailable. Open a collection to choose a destination."
                updateStatus()
            }
        }
    }

    private func apply() {
        guard !inert, !suspended else {
            updateStatus()
            return
        }
        let value = enabled && began
        let requestedPort = configuredPort
        let portChanged = appliedPort != requestedPort
        let destination = generation.flatMap { generation in
            selectedDeck != 0 && selectedType != 0
                ? MiningDestination(generation: generation, deck: selectedDeck, type: selectedType) : nil
        }
        guard appliedEnabled != value || appliedDestination != destination || portChanged else {
            updateStatus()
            return
        }
        appliedEnabled = value
        appliedPort = requestedPort
        appliedDestination = destination
        // Fence the old settings snapshot before any actor hop. Destination
        // edits retain the socket; port/off changes retire its fixed epoch.
        let retiring = !value || portChanged
        let admissionToken = receiver.admission.update(enabled: value,
            retiringListener: retiring || listener == nil)
        let token = UUID()
        controlToken = token
        let previous = control
        let retired: MiningListener?
        if retiring {
            retired = listener
            listener = nil
            listenerToken = UUID()
            listenerStatus?.cancel()
            state = .off
            retired?.requestStop()
        } else {
            retired = nil
        }
        updateStatus()
        control = Task { [weak self] in
            _ = await previous?.value
            guard let self else {
                return
            }
            // Even a superseded control owns the listener it retired. Join its
            // replies before allowing the next control to bind a replacement.
            if controlToken == token {
                await receiver.configure(enabled: value, destination: destination, admissionToken: admissionToken)
            }
            await receiver.drain()
            await retired?.stop()
            guard controlToken == token else {
                return
            }
            if value {
                if listener == nil {
                    let receiver = self.receiver
                    let listenerID = UUID()
                    listenerToken = listenerID
                    let events = AsyncStream<MiningListener.State>.makeStream()
                    listenerStatus = Task { @MainActor [weak self] in
                        for await state in events.stream {
                            guard let self, listenerToken == listenerID, enabled else { return }
                            self.state = state
                            if case .listening(let bound) = state { port = bound }
                            updateStatus()
                        }
                    }
                    listener = MiningListener(port: requestedPort, admission: receiver.admission,
                        launch: { receiver.launch($0, admissionToken: $1) },
                        changed: { events.continuation.yield($0) })
                }
                listener?.start()
            }
            updateStatus()
        }
    }

    /// Joins Settings changes and catalog refreshes without launching an app.
    func settle() async {
        while true {
            let refresh = refreshToken
            let control = controlToken
            await refreshTask?.value
            await self.control?.value
            if refreshToken == refresh, controlToken == control { return }
        }
    }

    private func updateStatus() {
        guard enabled, !suspended else {
            status = "Off · Port \(port)"
            return
        }
        switch state {
        case .off: status = began ? "Starting on 127.0.0.1:\(port)…" : "Waiting for collection · Port \(port)"
        case .starting: status = "Starting on 127.0.0.1:\(port)…"
        case .listening: status = "Listening on 127.0.0.1:\(port)" + (available ? "" : " · Collection unavailable")
        case .portInUse: status = "Port \(port) is in use. Quit Anki or the other app using this port. Meguri retries automatically."
        }
    }

    func resumeAfterRefusedQuit() {
        guard shutdownTask == nil else { return }
        suspended = false
        apply()
        refresh()
    }

    func shutdown() async {
        if let shutdownTask { await shutdownTask.value; return }
        suspended = true
        refreshTask?.cancel()
        refreshToken = UUID()
        appliedEnabled = nil
        _ = receiver.admission.update(enabled: false)
        controlToken = UUID()
        listenerToken = UUID()
        listenerStatus?.cancel()
        let retired = listener
        listener = nil
        retired?.requestStop()
        let previous = control
        let task = Task {
            _ = await previous?.value
            await receiver.configure(enabled: false, destination: nil)
            await retired?.stop()
            await receiver.drain()
            state = .off
            shutdownTask = nil
            updateStatus()
        }
        shutdownTask = task
        await task.value
    }
    /// No listener, defaults or collection is accessed by this synthetic
    /// production-pane fixture. The coordinator captures it in the test VM.
    public static func renderFixture(portConflict: Bool = false) -> SentenceMining {
        let model = SentenceMining(defaults: nil, collectionKey: "synthetic")
        model.inert = true
        model.enabled = true
        model.began = true
        model.available = true
        model.decks = [.init(id: 1, fullName: "Japanese::Sentences", filtered: false)]
        model.types = [.init(id: 2, name: "Imported Sentence Cards")]
        model.selectedDeck = 1
        model.selectedType = 2
        model.fieldMapping = ["Sentence ← sentence", "Translation ← translation", "Sentence Audio ← sentenceAudio", "Screenshot ← firstImage"]
        model.state = portConflict ? .portInUse : .listening(44432)
        model.updateStatus()
        return model
    }
}
#endif
