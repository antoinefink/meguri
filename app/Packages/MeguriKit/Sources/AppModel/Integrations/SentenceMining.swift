#if os(macOS)
import CoreBridge
import Foundation
import Observation

/// Device-local Settings owner. Construction never listens; the collection
/// entry path calls firstOpenAttemptEnded after launch has produced its UI.
@Observable public final class SentenceMining {
    public enum EndpointID: CaseIterable { case migaku, ankiConnect }

    @Observable public final class Endpoint {
        public fileprivate(set) var enabled: Bool
        public fileprivate(set) var status = ""
        public fileprivate(set) var port: UInt16
        fileprivate let configuredPort: UInt16
        @ObservationIgnored fileprivate let admission = MiningAdmission()
        @ObservationIgnored fileprivate var listener: MiningListener?
        @ObservationIgnored fileprivate var listenerStatus: Task<Void, Never>?
        @ObservationIgnored fileprivate var listenerToken = UUID()
        @ObservationIgnored fileprivate var state: MiningListener.State = .off
        fileprivate init(enabled: Bool, port: UInt16) {
            self.enabled = enabled
            self.port = port
            configuredPort = port
        }
    }
    public let migaku: Endpoint
    public let ankiConnect: Endpoint
    public var enabled: Bool { migaku.enabled || ankiConnect.enabled }
    public private(set) var available = false
    public private(set) var decks: [DeckNameEntry] = []
    public private(set) var types: [NotetypeName] = []
    public private(set) var selectedDeck: Int64
    public private(set) var selectedType: Int64
    public private(set) var fieldMapping: [String] = []
    public private(set) var error: String?
    @ObservationIgnored private let defaults: UserDefaults?
    @ObservationIgnored private let prefix: String
    @ObservationIgnored private let receiver = MiningReceiver()
    @ObservationIgnored private var inert = false
    @ObservationIgnored private var began = false
    @ObservationIgnored private var suspended = false
    @ObservationIgnored private var shutdownTask: Task<Void, Never>?
    @ObservationIgnored private var generation: UInt64?
    @ObservationIgnored private var appliedEnabled: Bool?
    @ObservationIgnored private var appliedDestination: MiningDestination?
    @ObservationIgnored private var controlToken = UUID()
    @ObservationIgnored private var control: Task<Void, Never>?
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var refreshToken = UUID()
    @ObservationIgnored public var enabledChanged: ((Bool) -> Void)?
    // Separate from the app delegate's last-window policy callback.
    @ObservationIgnored var collectionActivityChanged: (() -> Void)?

    /// The override pair is only for isolated synthetic wire tests.
    public init(defaults: UserDefaults?, collectionKey: String,
                ports: (migaku: UInt16, ankiConnect: UInt16)? = nil) {
        self.defaults = defaults
        // Migrate once. Missing legacy ports meant 44432; unknown ports must
        // never silently enable a different service. New bits take precedence.
        if defaults?.object(forKey: "mining.migaku.enabled") == nil,
           defaults?.object(forKey: "mining.ankiConnect.enabled") == nil {
            let legacyPort = defaults?.object(forKey: "mining.port") == nil
                ? "44432" : defaults?.string(forKey: "mining.port")
            let legacyEnabled = defaults?.bool(forKey: "mining.enabled") == true
            defaults?.set(legacyEnabled && legacyPort == "44432", forKey: "mining.migaku.enabled")
            defaults?.set(legacyEnabled && legacyPort == "8765", forKey: "mining.ankiConnect.enabled")
        }
        defaults?.removeObject(forKey: "mining.enabled")
        defaults?.removeObject(forKey: "mining.port")
        migaku = Endpoint(enabled: defaults?.bool(forKey: "mining.migaku.enabled") ?? false,
            port: ports?.migaku ?? 44432)
        ankiConnect = Endpoint(enabled: defaults?.bool(forKey: "mining.ankiConnect.enabled") ?? false,
            port: ports?.ankiConnect ?? 8765)
        prefix = "mining.destination." + MiningPayload.hash(Data(collectionKey.utf8))
        selectedDeck = Int64(defaults?.string(forKey: prefix + ".deck") ?? "") ?? 0
        selectedType = Int64(defaults?.string(forKey: prefix + ".type") ?? "") ?? 0
        updateStatus()
    }

    public func endpoint(_ id: EndpointID) -> Endpoint {
        switch id { case .migaku: migaku; case .ankiConnect: ankiConnect }
    }

    public func setEnabled(_ value: Bool, for id: EndpointID) {
        let endpoint = self.endpoint(id)
        guard value != endpoint.enabled else { return }
        endpoint.enabled = value
        defaults?.set(value, forKey: id == .migaku ? "mining.migaku.enabled" : "mining.ankiConnect.enabled")
        enabledChanged?(enabled)
        apply()
        collectionActivityChanged?()
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

    private func retire(_ endpoint: Endpoint) -> MiningListener? {
        _ = endpoint.admission.update(enabled: false)
        let retired = endpoint.listener
        endpoint.listener = nil
        endpoint.listenerToken = UUID()
        endpoint.listenerStatus?.cancel()
        endpoint.listenerStatus = nil
        endpoint.state = .off
        retired?.requestStop()
        return retired
    }

    private func apply() {
        guard !inert, !suspended else { updateStatus(); return }
        let value = enabled && began
        let destination = generation.flatMap { generation in
            selectedDeck != 0 && selectedType != 0
                ? MiningDestination(generation: generation, deck: selectedDeck, type: selectedType) : nil
        }
        let configure = appliedEnabled != value || appliedDestination != destination
        appliedEnabled = value
        appliedDestination = destination
        // Only destination/aggregate changes fence the shared receiver. A
        // sibling toggle must leave this endpoint's queued work and peers live.
        let admissionToken = configure
            ? receiver.admission.update(enabled: value, retiringListener: false)
            : receiver.admission.current()
        let retired = EndpointID.allCases.compactMap { id -> MiningListener? in
            let endpoint = self.endpoint(id)
            return !endpoint.enabled || !began ? retire(endpoint) : nil
        }
        let token = UUID()
        controlToken = token
        let previous = control
        updateStatus()
        control = Task { [weak self] in
            _ = await previous?.value
            guard let self else { return }
            if controlToken == token {
                let changed = await receiver.configure(enabled: value, destination: destination, admissionToken: admissionToken)
                // A sibling toggle must not wait for unrelated ongoing writes.
                if changed { await receiver.drain() }
            }
            // Superseded controls still join every listener they retired.
            for listener in retired { await listener.stop() }
            guard controlToken == token else { return }
            for id in EndpointID.allCases {
                let endpoint = self.endpoint(id)
                guard endpoint.enabled, began, endpoint.listener == nil else { continue }
                _ = endpoint.admission.update(enabled: true)
                let receiver = self.receiver
                let admission = endpoint.admission
                let listenerID = UUID()
                endpoint.listenerToken = listenerID
                let events = AsyncStream<MiningListener.State>.makeStream()
                endpoint.listenerStatus = Task { @MainActor [weak self, weak endpoint] in
                    for await state in events.stream {
                        guard let self, let endpoint, endpoint.listenerToken == listenerID, endpoint.enabled else { return }
                        endpoint.state = state
                        if case .listening(let port) = state { endpoint.port = port }
                        updateStatus()
                    }
                }
                endpoint.listener = MiningListener(port: endpoint.configuredPort, admission: admission,
                    launch: { request, token in
                        receiver.launch(request, admissionToken: receiver.admission.current(),
                            isCurrent: { token.map { admission.matches($0) } == true })
                    }, changed: { events.continuation.yield($0) })
                endpoint.listener?.start()
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
        for id in EndpointID.allCases {
            let endpoint = self.endpoint(id)
            let port = endpoint.port
            guard endpoint.enabled, !suspended else {
                endpoint.status = "Off · Port \(port)"
                continue
            }
            switch endpoint.state {
            case .off: endpoint.status = began ? "Starting on 127.0.0.1:\(port)…" : "Waiting for collection · Port \(port)"
            case .starting: endpoint.status = "Starting on 127.0.0.1:\(port)…"
            case .listening: endpoint.status = "Listening on 127.0.0.1:\(port)" + (available ? "" : " · Collection unavailable")
            case .portInUse: endpoint.status = "Port \(port) is in use. Quit the app using it. Meguri retries automatically."
            }
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
        let retired = EndpointID.allCases.compactMap { retire(endpoint($0)) }
        let previous = control
        let task = Task {
            _ = await previous?.value
            await receiver.configure(enabled: false, destination: nil)
            for listener in retired { await listener.stop() }
            await receiver.drain()
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
        model.migaku.enabled = true
        model.ankiConnect.enabled = true
        model.began = true
        model.available = true
        model.decks = [.init(id: 1, fullName: "Japanese::Sentences", filtered: false)]
        model.types = [.init(id: 2, name: "Imported Sentence Cards")]
        model.selectedDeck = 1
        model.selectedType = 2
        model.fieldMapping = ["Sentence ← sentence", "Translation ← translation", "Sentence Audio ← sentenceAudio", "Screenshot ← firstImage"]
        model.migaku.state = .listening(44432)
        model.ankiConnect.state = portConflict ? .portInUse : .listening(8765)
        model.updateStatus()
        return model
    }
}
#endif
