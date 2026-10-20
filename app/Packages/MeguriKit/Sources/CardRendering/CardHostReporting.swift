import Foundation
import WebKit

/// Revision/serial admission is independent of WebKit transport, and exercised
/// directly by tests. Only an acknowledged native context can activate it.
final class CardHostAdmission {
    private(set) var context: CardHostContext?
    private(set) var hitMap: CardHostHitMap?
    private var inputSerial: UInt64 = 0
    private var mapSerial: UInt64 = 0
    private var layoutRevision: UInt64 = 0
    private var geometry: CardHostGeometry?

    func discardMap() { hitMap = nil }
    func clear() {
        context = nil; hitMap = nil; geometry = nil
        inputSerial = 0; mapSerial = 0; layoutRevision = 0
    }
    func activate(_ context: CardHostContext) {
        guard self.context != context else { return }
        clear(); self.context = context
    }

    func receive(_ report: CardHostReport) -> CardHostEvent? {
        guard let context, report.update == context.update, report.revision == context.viewRevision,
              context.visibility == .presented else { return nil }
        switch report.value {
        case .input(let serial, let input):
            guard serial > inputSerial else { return nil }
            if case .click(let observed, _, _, _) = input.detail {
                guard acceptGeometry(observed) else { return nil }
            }
            inputSerial = serial
            return .input(context, input)
        case .hitMap(let serial, let map):
            guard serial > mapSerial, acceptGeometry(map.geometry) else { return nil }
            mapSerial = serial; hitMap = map
            return .hitMap(context, map)
        case .probe: return nil
        }
    }
    private func acceptGeometry(_ next: CardHostGeometry) -> Bool {
        guard next.layoutRevision >= layoutRevision else { return false }
        if next.layoutRevision == layoutRevision, let geometry, geometry != next { return false }
        if next != geometry { hitMap = nil }
        layoutRevision = next.layoutRevision; geometry = next
        return true
    }
}

/// Per actual page. No production producer is installed here (H10).
public final class CardHostReporting {
    public private(set) var viewRevision: UInt64 = 1
    public var context: CardHostContext? {
        checkGeometry()
        return admission.context
    }
    public var hitMap: CardHostHitMap? {
        checkGeometry()
        return admission.hitMap
    }
    private func checkGeometry() {
        #if !os(macOS)
        page?.webView.checkHostScrollGeometry()
        #endif
        if let context = admission.context, !isCurrent(context) { geometryChanged() }
    }
    public private(set) var slotBackground: CardHostProbeValue?
    public private(set) var slotResources: CardHostProbeValue?
    var onEvent: ((CardHostEvent) -> Void)?
    private weak var page: CardPage?
    private let admission = CardHostAdmission()
    private var installed = false
    private var installation = UUID()
    private var publication: Task<Void, Never>?
    private var publishingContext: CardHostContext?
    private var requestSerial: UInt64 = 0
    private struct Pending {
        let id: UInt64
        let context: CardHostContext
        let purpose: CardHostResourcePurpose?
        let deadline: ContinuousClock.Instant
        let completion: (CardHostContext?, CardHostProbeOutcome) -> Void
        var timer: Task<Void, Never>?
        var work: Task<Void, Never>?
        var acknowledged = false
        var early: CardHostProbeValue?
    }
    private var probes: [CardHostProbeKind: Pending] = [:]
    private static let maxInteger: UInt64 = 9_007_199_254_740_991
    init(page: CardPage) { self.page = page }

    /// Invalidates synchronously, before any asynchronous JavaScript work.
    /// Only presentation may consume preparation observations for this slot.
    func invalidate(keepSlot: Bool = false) {
        admission.clear()
        publication?.cancel(); publication = nil; publishingContext = nil
        if !keepSlot { slotBackground = nil; slotResources = nil }
        if viewRevision < Self.maxInteger { viewRevision += 1 }
        else { installed = false }
        onEvent?(.invalidated(viewRevision: viewRevision))
        let pending = probes.values
        probes.removeAll()
        for probe in pending { probe.timer?.cancel(); probe.work?.cancel(); probe.completion(probe.context, .cancelled) }
        guard installed, let page, !page.closed else { return }
        let revision = viewRevision
        publication = Task { [weak self, weak page] in
            guard let self, let page else { return }
            let accepted = await self.send(["op": "clear", "viewRevision": revision], page: page)
            guard self.viewRevision == revision, !Task.isCancelled else { return }
            if !accepted { self.unavailable() }
        }
    }

    func disable() {
        installed = false
        installation = UUID()
        invalidate()
    }

    func install(_ source: String) async throws {
        disable()
        let installation = UUID()
        self.installation = installation
        guard let page, !page.closed else { throw CardPage.Failure.closed }
        do { try await page.run(source, world: CardPage.world) }
        catch {
            if self.installation == installation { unavailable() }
            throw error
        }
        guard self.installation == installation, !page.closed else { throw CardPage.Failure.superseded }
        let revision = viewRevision
        let accepted = await send(["op": "clear", "viewRevision": revision], page: page)
        guard self.installation == installation, viewRevision == revision, !page.closed else { return }
        guard accepted else { unavailable(); return }
        installed = true
        await publish()
    }

    func publish() async {
        guard let page, !page.closed, page.hostDOMKnown, let update = page.update else { return }
        let rect = page.webView.hostContentRect, zoom = Double(page.webView.pageZoom)
        guard installed, viewRevision < Self.maxInteger,
              rect.width.isFinite, rect.height.isFinite, rect.minX.isFinite, rect.minY.isFinite,
              rect.width > 0, rect.height > 0, rect.width <= 1_000_000, rect.height <= 1_000_000,
              zoom.isFinite, zoom > 0, zoom <= 1_000 else { unavailable(); return }
        let context = CardHostContext(update: update, viewRevision: viewRevision,
                                      visibility: page.presented ? .presented : .preparing,
                                      contentRect: rect, pageZoom: zoom)
        if admission.context == context { return }
        if publishingContext == context { await publication?.value; return }
        let previous = publication
        publishingContext = context
        // Immutable capture survives the asynchronous receive/ack round trip.
        let operation = Task { [weak self, weak page] in
            await previous?.value
            guard let self, let page, self.isCurrent(context), !Task.isCancelled else { return }
            let accepted = await self.send(["op": "context", "card": update.card, "generation": update.generation,
                                            "side": update.sideName, "viewRevision": context.viewRevision,
                                            "visibility": context.visibility.rawValue,
                                            "bounds": ["width": rect.width, "height": rect.height], "pageZoom": zoom], page: page)
            guard self.isCurrent(context), !Task.isCancelled else { return }
            if accepted { self.admission.activate(context) }
            else { self.unavailable() }
        }
        publication = operation
        await operation.value
    }

    /// Await once after a synchronous native bounds/zoom/scroll change. This
    /// acknowledges only this identity and epoch; it never proves DOM freshness.
    /// nil means absent/failed host, cancellation, or a superseding change.
    public func refreshContext(for update: CardUpdate) async -> CardHostContext? {
        checkGeometry()
        guard page?.update == update, !Task.isCancelled else { return nil }
        let revision = viewRevision
        await publish()
        guard !Task.isCancelled, page?.update == update, viewRevision == revision,
              let context, isCurrent(context) else { return nil }
        return context
    }

    func geometryChanged() {
        invalidate()
        Task { [weak self] in await self?.publish() }
    }

    private func isCurrent(_ context: CardHostContext) -> Bool {
        guard let page else { return false }
        return installed && !page.closed && page.hostDOMKnown && page.update == context.update && viewRevision == context.viewRevision
            && page.presented == (context.visibility == .presented)
            && page.webView.hostContentRect == context.contentRect && Double(page.webView.pageZoom) == context.pageZoom
    }
    private func unavailable() {
        installed = false
        admission.clear()
        let pending = probes.values
        probes.removeAll()
        for probe in pending { probe.timer?.cancel(); probe.work?.cancel(); probe.completion(probe.context, .unavailable) }
        onEvent?(.unavailable)
    }

    /// Fixed code and structured arguments only; a true JS boolean is the
    /// sole acknowledgement. Absence/throw/undefined/1/string all fail closed.
    private func send(_ message: [String: Any], page: CardPage) async -> Bool {
        do {
            return try await page.hostAcknowledgement(message)
        } catch { return false }
    }

    /// At most one request per kind. No retry, no readiness on expiry.
    /// The completion carries the request's immutable native context.
    @discardableResult
    public func probe(_ kind: CardHostProbeKind, purpose: CardHostResourcePurpose? = nil, timeoutMs: Int? = nil,
                      completion: @escaping (CardHostContext?, CardHostProbeOutcome) -> Void) -> UInt64? {
        let timeout = timeoutMs ?? (kind == .background ? 250 : 1000)
        guard (1...2000).contains(timeout), (kind == .resources) == (purpose != nil),
              let context, isCurrent(context), let page, requestSerial < Self.maxInteger else {
            completion(context, .unavailable)
            onEvent?(.unavailable)
            return nil
        }
        let previous = probes.removeValue(forKey: kind)
        previous?.timer?.cancel(); previous?.work?.cancel()
        requestSerial += 1
        let id = requestSerial
        let deadline = ContinuousClock.now.advanced(by: .milliseconds(timeout))
        probes[kind] = Pending(id: id, context: context, purpose: purpose, deadline: deadline, completion: completion)
        probes[kind]?.timer = Task { [weak self] in
            do { try await ContinuousClock().sleep(until: deadline) } catch { return }
            self?.finish(kind, id: id, outcome: .timeout)
        }
        probes[kind]?.work = Task { [weak self, weak page] in
            guard let self, let page, self.probes[kind]?.id == id, self.isCurrent(context) else { return }
            guard ContinuousClock.now < deadline else { self.finish(kind, id: id, outcome: .timeout); return }
            var message: [String: Any] = ["op": "probe", "kind": kind.rawValue, "request": id,
                                          "viewRevision": context.viewRevision, "timeoutMs": timeout]
            if let purpose { message["purpose"] = purpose.rawValue }
            let accepted = await self.send(message, page: page)
            guard self.probes[kind]?.id == id, self.isCurrent(context) else { return }
            guard ContinuousClock.now < deadline else { self.finish(kind, id: id, outcome: .timeout); return }
            guard accepted else { self.unavailable(); return }
            self.probes[kind]?.acknowledged = true
            if let early = self.probes[kind]?.early { self.finish(kind, id: id, outcome: .observed(early)) }
        }
        // Install the replacement before calling its predecessor's owner,
        // which can synchronously cancel, close or request another probe.
        if let previous { previous.completion(previous.context, .cancelled) }
        return id
    }
    public func cancelProbe(_ kind: CardHostProbeKind) {
        if let probe = probes[kind] { finish(kind, id: probe.id, outcome: .cancelled) }
    }
    private func finish(_ kind: CardHostProbeKind, id: UInt64, outcome: CardHostProbeOutcome) {
        guard let probe = probes[kind], probe.id == id else { return }
        probes[kind] = nil; probe.timer?.cancel(); probe.work?.cancel()
        if case .observed(let value) = outcome {
            if kind == .background { slotBackground = value } else { slotResources = value }
        }
        probe.completion(probe.context, outcome)
    }

    func receive(_ body: Any) {
        guard let report = CardHostReport(body) else {
            let hadMap = admission.hitMap != nil
            admission.discardMap()
            if hadMap { onEvent?(.hitMapUnavailable) }
            return
        }
        guard let context, isCurrent(context),
              report.update == context.update, report.revision == context.viewRevision else { return }
        if case .probe(let kind, let request, let value) = report.value {
            guard let probe = probes[kind], probe.id == request, probe.context == context else { return }
            guard ContinuousClock.now < probe.deadline else { finish(kind, id: request, outcome: .timeout); return }
            if case .resources(let purpose, _, _, _, _) = value, purpose != probe.purpose { return }
            if probe.acknowledged { finish(kind, id: request, outcome: .observed(value)) }
            else if probe.early == nil { probes[kind]?.early = value }
        } else if let event = admission.receive(report) { onEvent?(event) }
    }

    func close() {
        installed = false; installation = UUID()
        invalidate()
        onEvent = nil
    }
}

/// Shared registration does not grant opt-in: only pair installation on the
/// registered actual page can activate that page's reporting state.
public final class CardHostBridge: NSObject, WKScriptMessageHandlerWithReply {
    static let name = "meguriReviewHost"
    private struct WeakPage { weak var value: CardPage? }
    private var pages: [ObjectIdentifier: WeakPage] = [:]
    func register(_ page: CardPage) { pages[ObjectIdentifier(page.webView)] = WeakPage(value: page) }
    func unregister(_ page: CardPage) { pages[ObjectIdentifier(page.webView)] = nil }

    public func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage,
                                      replyHandler: @escaping @MainActor @Sendable (Any?, String?) -> Void) {
        let origin = message.frameInfo.securityOrigin
        receive(name: message.name, world: message.world.name, body: message.body,
                page: message.webView.flatMap { pages[ObjectIdentifier($0)]?.value }, mainFrame: message.frameInfo.isMainFrame,
                origin: message.frameInfo.request.url, scheme: origin.protocol, host: origin.host, port: origin.port, replyHandler: replyHandler)
    }

    /// Both metadata and reply semantics use this path in production and tests.
    func receive(name: String, world: String?, body: Any, page: CardPage?, mainFrame: Bool,
                 origin: URL?, scheme: String, host: String, port: Int,
                 replyHandler: @MainActor @Sendable (Any?, String?) -> Void) {
        defer { replyHandler(NSNull(), nil) }
        guard name == Self.name, world == CardRuntime.interactionWorldName, mainFrame,
              scheme == MediaOrigin.scheme, host == MediaOrigin.host, port == 0,
              let origin, MediaOrigin.path(origin) == "/", let page, !page.closed,
              pages[ObjectIdentifier(page.webView)]?.value === page else { return }
        page.hostReports.receive(body)
    }
}
