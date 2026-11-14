@testable import CardRendering
import Foundation
import Testing
import WebKit
#if os(macOS)
import AppKit
#endif

private enum HostReports {
    static let geometry: [String: Any] = ["width": 200, "height": 300, "offsetX": 40, "offsetY": 70, "scale": 2, "layoutRevision": 1]
    static let rect: [String: Any] = ["x": 10, "y": 20, "width": 30, "height": 40]
    static var click: [String: Any] {
        ["kind": "click", "timeMs": 12.5, "defaultPrevented": false, "exempt": false, "selection": false,
         "editable": false, "typeans": false, "composing": false, "geometry": geometry,
         "x": 20, "y": 30, "button": 0, "pointer": "touch"]
    }
    static var key: [String: Any] {
        ["kind": "key", "timeMs": 12.5, "defaultPrevented": true, "exempt": false, "selection": true,
         "editable": true, "typeans": true, "composing": false, "event": "keyup", "key": "", "code": "",
         "repeat": false, "shift": false, "control": true, "alt": false, "meta": false]
    }
    static var map: [String: Any] { ["geometry": geometry, "complete": true, "rects": [rect]] }
    static func body(_ signal: String = "hostInput", payload: [String: Any]? = nil, revision: UInt64 = 2, serial: UInt64 = 1) -> [String: Any] {
        var result: [String: Any] = ["signal": signal, "version": 1, "card": 7, "generation": 1, "side": "question", "viewRevision": revision,
                                   "payload": payload ?? click]
        if signal != "hostProbe" { result["serial"] = serial }
        return result
    }
    static func replacing(_ original: [String: Any], _ key: String, _ value: Any) -> [String: Any] {
        original.merging([key: value], uniquingKeysWith: { _, new in new })
    }
    static var context: CardHostContext {
        .init(update: CardUpdate(card: 7, generation: 1, side: .question), viewRevision: 2, visibility: .presented,
              contentRect: CGRect(x: 10, y: 20, width: 400, height: 600), pageZoom: 2)
    }
    static let syntheticHost = """
        window.hostMessages = [];
        window.meguriReviewHost = { receive(message) {
          hostMessages.push(message);
          if (message.op === 'clear') window.hostContext = null;
          if (message.op === 'context') window.hostContext = message;
          return true;
        }};
        """
}

// Extend existing selected suites: no additional selector is required.
extension WebKitSuites.BridgeAllowlistTests {
    @Test func hostReportSchemaStrictVariantsAndBoundaries() throws {
        #expect(CardHostReport(HostReports.body()) != nil)
        for key in HostReports.body().keys {
            var missing = HostReports.body()
            missing[key] = nil
            #expect(CardHostReport(missing) == nil)
        }
        for key in HostReports.click.keys {
            var missing = HostReports.click
            missing[key] = nil
            #expect(CardHostReport(HostReports.body(payload: missing)) == nil)
        }
        #expect(CardHostReport(HostReports.body(payload: HostReports.key)) != nil)
        #expect(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.map)) != nil)
        let background: [String: Any] = ["kind": "background", "request": 1, "rgba": [0.0, 0.5, 1.0, 0.0]]
        #expect(CardHostReport(HostReports.body("hostProbe", payload: background)) != nil)
        for purpose in ["warmup", "preload"] {
            for state in ["ready", "budgetExpired", "failed"] {
                let p: [String: Any] = ["kind": "resources", "request": 1, "purpose": purpose, "state": state,
                                       "imagesPending": 0, "fontsPending": 1_000_000, "stylesPending": 1]
                #expect(CardHostReport(HostReports.body("hostProbe", payload: p)) != nil)
            }
        }
        for field in ["version", "card", "generation", "viewRevision", "serial"] {
            for invalid: Any in [true, 1.5, Double.nan, Double.infinity, 9_007_199_254_740_992.0, "1", NSNull()] {
                #expect(CardHostReport(HostReports.replacing(HostReports.body(), field, invalid)) == nil)
            }
        }
        for (field, invalid): (String, Any) in [("version", 2), ("viewRevision", 0), ("serial", 0), ("generation", -1), ("side", "front"),
                                               ("signal", "nativeOpen"), ("page", 1), ("world", "meguri.card"), ("op", "reveal")] {
            #expect(CardHostReport(HostReports.replacing(HostReports.body(), field, invalid)) == nil)
        }
        #expect(CardHostReport(HostReports.replacing(HostReports.body(), "card", -9_007_199_254_740_991)) != nil)
        for field in ["defaultPrevented", "exempt", "selection", "editable", "typeans", "composing"] {
            #expect(CardHostReport(HostReports.body(payload: HostReports.replacing(HostReports.click, field, 1))) == nil)
        }
        for (field, invalid): (String, Any) in [("typeans", true), ("trusted", true), ("selector", "#qa"), ("button", 5), ("pointer", "trackpad"),
                                               ("x", -1), ("y", 301), ("timeMs", -1), ("timeMs", Double.infinity)] {
            #expect(CardHostReport(HostReports.body(payload: HostReports.replacing(HostReports.click, field, invalid))) == nil)
        }
        for field in ["repeat", "shift", "control", "alt", "meta"] {
            #expect(CardHostReport(HostReports.body(payload: HostReports.replacing(HostReports.key, field, "false"))) == nil)
        }
        let boundaryKey = HostReports.replacing(HostReports.key, "key", String(repeating: "é", count: 64))
        #expect(CardHostReport(HostReports.body(payload: boundaryKey)) != nil)
        for field in ["key", "code"] {
            #expect(CardHostReport(HostReports.body(payload: HostReports.replacing(HostReports.key, field, String(repeating: "é", count: 65)))) == nil)
        }
        #expect(CardHostReport(HostReports.body(payload: HostReports.replacing(HostReports.key, "event", "keypress"))) == nil)
        #expect(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.replacing(HostReports.map, "rects", Array(repeating: HostReports.rect, count: 256)))) != nil)
        #expect(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.replacing(HostReports.map, "rects", Array(repeating: HostReports.rect, count: 257)))) == nil)
        for field in ["width", "height", "offsetX", "offsetY", "scale", "layoutRevision"] {
            for invalid: Any in [true, Double.nan, Double.infinity, "1"] {
                let geometry = HostReports.replacing(HostReports.geometry, field, invalid)
                #expect(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.replacing(HostReports.map, "geometry", geometry))) == nil)
            }
        }
        for (field, invalid): (String, Any) in [("width", 0), ("height", 1_000_001), ("offsetX", 1_000_000_001), ("offsetY", -1_000_000_001),
                                               ("scale", 0), ("scale", 1001), ("layoutRevision", 1.5), ("extra", 1)] {
            let geometry = HostReports.replacing(HostReports.geometry, field, invalid)
            #expect(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.replacing(HostReports.map, "geometry", geometry))) == nil)
        }
        for (field, invalid): (String, Any) in [("x", -1), ("y", 301), ("width", 0), ("height", 281), ("width", true), ("id", "secret")] {
            let rect = HostReports.replacing(HostReports.rect, field, invalid)
            #expect(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.replacing(HostReports.map, "rects", [rect]))) == nil)
        }
        #expect(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.replacing(HostReports.map, "complete", 1))) == nil)
        for rgba: [Any] in [[0, 0, 0], [0, 0, 0, 0, 0], [0, 0, 0, true], [0, 0, 0, 1.01], [0, 0, 0, Double.nan]] {
            #expect(CardHostReport(HostReports.body("hostProbe", payload: HostReports.replacing(background, "rgba", rgba))) == nil)
        }
        let resource: [String: Any] = ["kind": "resources", "request": 1, "purpose": "warmup", "state": "ready", "imagesPending": 0, "fontsPending": 0, "stylesPending": 0]
        for field in ["imagesPending", "fontsPending", "stylesPending"] {
            for invalid: Any in [true, -1, 1.5, 1_000_001, Double.infinity] {
                #expect(CardHostReport(HostReports.body("hostProbe", payload: HostReports.replacing(resource, field, invalid))) == nil)
            }
        }
        for (field, invalid): (String, Any) in [("purpose", "sync"), ("state", "success"), ("request", 0), ("request", true), ("url", "file:///secret")] {
            #expect(CardHostReport(HostReports.body("hostProbe", payload: HostReports.replacing(resource, field, invalid))) == nil)
        }
        #expect(CardHostReport(HostReports.replacing(HostReports.body(), "payload", ["nested": ["nested": ["nested": ["nested": 1]]]])) == nil)
        #expect(CardHostReport(HostReports.body(payload: HostReports.replacing(HostReports.key, "key", String(repeating: "x", count: 65_537)))) == nil)
        #expect(CardHostReport("{\"signal\":\"hostInput\"}") == nil)
        #expect(CardHostReport([HostReports.body()]) == nil)
    }

    @Test func nativeHostAdmissionSerialVisibilityGeometryAndConservativeMaps() throws {
        let admission = CardHostAdmission()
        let input = try #require(CardHostReport(HostReports.body()))
        #expect(admission.receive(input) == nil)
        admission.activate(HostReports.context)
        #expect(admission.receive(input) != nil)
        #expect(admission.receive(input) == nil)
        // Republish of the same native epoch must not reset serial admission.
        admission.activate(HostReports.context)
        #expect(admission.receive(input) == nil)
        for (field, invalid): (String, Any) in [("card", 8), ("generation", 2), ("side", "answer"), ("viewRevision", 3)] {
            let stale = try #require(CardHostReport(HostReports.replacing(HostReports.body(serial: 2), field, invalid)))
            #expect(admission.receive(stale) == nil)
        }
        let map = try #require(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.map)))
        #expect(admission.receive(map) != nil)
        let cached = try #require(admission.hitMap)
        // Native points include content insets and a 2:1 points/CSS ratio.
        // Scroll offsets and visualViewport scale must not be applied twice.
        #expect(cached.isExempt(CGPoint(x: 50, y: 80), context: HostReports.context) == true)
        #expect(cached.isExempt(CGPoint(x: 390, y: 600), context: HostReports.context) == false)
        #expect(cached.isExempt(CGPoint(x: 0, y: 0), context: HostReports.context) == nil)
        let inconsistent = HostReports.replacing(HostReports.geometry, "scale", 3)
        let inconsistentMap = try #require(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.replacing(HostReports.map, "geometry", inconsistent), serial: 2)))
        #expect(admission.receive(inconsistentMap) == nil)
        let nextGeometry = HostReports.replacing(HostReports.geometry, "layoutRevision", 2)
        let newer = try #require(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.replacing(HostReports.map, "geometry", nextGeometry), serial: 2)))
        #expect(admission.receive(newer) != nil)
        let older = try #require(CardHostReport(HostReports.body("hostHitMap", payload: HostReports.map, serial: 3)))
        #expect(admission.receive(older) == nil)
        let uncertain = try #require(CardHostReport(HostReports.body("hostHitMap", payload: ["geometry": nextGeometry, "complete": false, "rects": []], serial: 4)))
        #expect(admission.receive(uncertain) != nil)
        #expect(admission.hitMap?.isExempt(CGPoint(x: 100, y: 100), context: HostReports.context) == nil)
        let whole = try #require(CardHostReport(HostReports.body("hostHitMap", payload: ["geometry": nextGeometry, "complete": true,
            "rects": [["x": 0, "y": 0, "width": 200, "height": 300]]], serial: 5)))
        #expect(admission.receive(whole) != nil)
        #expect(admission.hitMap?.isExempt(CGPoint(x: 390, y: 600), context: HostReports.context) == true)
        admission.clear()
        #expect(admission.hitMap == nil && admission.receive(input) == nil)
        let idle = CardHostContext(update: HostReports.context.update, viewRevision: 2, visibility: .preparing,
                                   contentRect: HostReports.context.contentRect, pageZoom: 2)
        admission.activate(idle)
        #expect(admission.receive(input) == nil && admission.receive(map) == nil && admission.hitMap == nil)
    }
}

extension WebKitSuites.CardPagePairTests {
    private func hostPair() async throws -> (CardPagePair, CardWebConfiguration, MediaFixture, SpikeWindow) {
        let fixture = MediaFixture()
        let configuration = CardWebConfiguration(store: .nonPersistent())
        let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved())
        let window = SpikeWindow(pair.container)
        try await pair.load()
        _ = try await pair.prepare(CardPayload(cardID: 7, question: "<p>synthetic</p>", answer: "answer"))
        return (pair, configuration, fixture, window)
    }
    private func hostWait(_ condition: () -> Bool) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(1))
        while !condition(), ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(2)) }
        #expect(condition())
    }

    @Test func isolatedHostInstallationAbsentThrowReturnTypeAndReinstall() async throws {
        let (pair, configuration, fixture, window) = try await hostPair()
        let other = CardPagePair(configuration: configuration, resolver: try await fixture.resolved())
        let otherWindow = SpikeWindow(other.container)
        defer { pair.close(); other.close(); window.close(); otherWindow.close() }
        let page = pair.idle
        var unavailable = 0
        pair.onHostEvent = { _, event in if event == .unavailable { unavailable += 1 } }
        #expect(page.hostReports.context == nil)
        var outcomes: [CardHostProbeOutcome] = []
        #expect(page.hostReports.probe(.background) { _, outcome in outcomes.append(outcome) } == nil)
        #expect(outcomes == [.unavailable])
        try await pair.installHostScript("delete window.meguriReviewHost;")
        #expect(page.hostReports.context == nil && unavailable > 0)
        for source in ["window.meguriReviewHost={receive(){throw new Error('synthetic')}};",
                       "window.meguriReviewHost={receive(){return 1}};",
                       "window.meguriReviewHost={receive(){return 'true'}};",
                       "window.meguriReviewHost={receive(){}};",
                       "window.meguriReviewHost={receive(){return false}};",
                       "window.meguriReviewHost={receive(){return null}};",
                       "window.meguriReviewHost={receive(){return {ready:true}}};",
                       "window.meguriReviewHost={receive:1};",
                       "window.meguriReviewHost={receive(){return Promise.resolve(true)}};"] {
            try await pair.installHostScript(source)
            #expect(page.hostReports.context == nil)
        }
        try await pair.installHostScript(HostReports.syntheticHost)
        let context = try #require(page.hostReports.context)
        #expect(context.visibility == .preparing && context.update == page.update)
        try await other.load()
        #expect(try await other.current.string("return typeof window.meguriReviewHost;", world: CardPage.world) == "undefined")
        #expect(try await page.string("return typeof window.meguriReviewHost;") == "undefined")
        #expect(try await page.string("return typeof window.webkit?.messageHandlers?.meguriReviewHost;") == "undefined")
        // Reinstallation issues a fresh native epoch, clearing the old host.
        try await pair.installHostScript(HostReports.syntheticHost)
        #expect(try #require(page.hostReports.context).viewRevision > context.viewRevision)
        let messages = try await page.string("return JSON.stringify(hostMessages);", world: CardPage.world)
        #expect(messages.contains("\"op\":\"clear\"") && messages.contains("\"op\":\"context\""))
    }

    @Test func immediatePreparedHostProbesHaveAcknowledgedContext() async throws {
        let fixture = MediaFixture()
        let configuration = CardWebConfiguration(store: .nonPersistent())
        let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved())
        let window = SpikeWindow(pair.container)
        defer { pair.close(); window.close() }
        // Transport fixture only: accept the fixed ABI synchronously and echo
        // bounded observations using the immutable native context at start.
        try await pair.installHostScript("""
            window.immediateProbeHost = {context:null, messages:[], replies:[]};
            window.meguriReviewHost = { receive(message) {
              const host = immediateProbeHost;
              host.messages.push(message);
              if(message.op==='clear') { host.context=null; return true; }
              if(message.op==='context') { host.context=Object.freeze({...message}); return true; }
              const start = host.context;
              if(message.op!=='probe' || !start || message.viewRevision!==start.viewRevision) return false;
              let payload;
              if(message.kind==='background') {
                payload = {kind:'background',request:message.request,rgba:[0,0.5,1,0]};
              } else if(message.kind==='resources' && message.purpose==='preload') {
                payload = {kind:'resources',request:message.request,purpose:'preload',state:'ready',
                           imagesPending:0,fontsPending:0,stylesPending:0};
              } else { return false; }
              host.replies.push(webkit.messageHandlers.meguriReviewHost.postMessage({
                signal:'hostProbe',version:1,card:start.card,generation:start.generation,side:start.side,
                viewRevision:start.viewRevision,payload
              }));
              return true;
            }};
            """)
        let page = pair.idle
        let otherPage = pair.current
        typealias Completion = (kind: CardHostProbeKind, context: CardHostContext?, outcome: CardHostProbeOutcome)
        let callbacks = AsyncStream<Completion>.makeStream(bufferingPolicy: .bufferingOldest(2))
        defer { callbacks.continuation.finish() }

        let prepared = try await pair.prepare(CardPayload(cardID: 73, question: "<p>synthetic prepared host</p>", answer: "answer"))
        // No suspension, publication call or readiness polling between prepare
        // and either request: this is the public owner's first opportunity.
        let backgroundRequest = page.hostReports.probe(.background, timeoutMs: 2000) { context, outcome in
            callbacks.continuation.yield((.background, context, outcome))
        }
        let resourceRequest = page.hostReports.probe(.resources, purpose: .preload, timeoutMs: 2000) { context, outcome in
            callbacks.continuation.yield((.resources, context, outcome))
        }
        let contextAtRequest = page.hostReports.context
        #expect(backgroundRequest != nil, "prepare must return with an acknowledged host context for immediate background probes")
        #expect(resourceRequest != nil, "prepare must return with an acknowledged host context for immediate resource probes")

        let completions = try await Spike.deadline("immediate prepared host probe callbacks", timeout: .seconds(3)) {
            var results: [Completion] = []
            for await result in callbacks.stream {
                results.append(result)
                if results.count == 2 { return results }
            }
            throw Spike.Failure("the prepared host did not complete both probes")
        }
        let backgrounds = completions.filter { $0.kind == .background }
        let resources = completions.filter { $0.kind == .resources }
        #expect(backgrounds.map { $0.outcome } == [.observed(.background(red: 0, green: 0.5, blue: 1, alpha: 0))])
        #expect(resources.map { $0.outcome } == [.observed(.resources(purpose: .preload, state: .ready, imagesPending: 0, fontsPending: 0, stylesPending: 0))])
        let context = try #require(contextAtRequest)
        #expect(context.update == prepared && context.visibility == .preparing)
        #expect(completions.allSatisfy { $0.context == context })
        #expect(page.hostReports.context == context && page.update == prepared && pair.idle === page)
        #expect(page.hostReports.slotBackground == .background(red: 0, green: 0.5, blue: 1, alpha: 0))
        #expect(page.hostReports.slotResources == .resources(purpose: .preload, state: .ready, imagesPending: 0, fontsPending: 0, stylesPending: 0))
        let backgroundID = try #require(backgroundRequest)
        let resourceID = try #require(resourceRequest)
        #expect(backgroundID != resourceID)
        // Inspect only after the immediate requests and callbacks. The real
        // isolated handler must have received both reports and replied JS null.
        let delivered = try await Spike.deadline("prepared host transport replies", timeout: .seconds(3)) {
            try await page.string("""
                const host = immediateProbeHost, context = host.context;
                const probes = host.messages.filter(message=>message.op==='probe');
                const replies = await Promise.all(host.replies);
                return String(context.card===card && context.generation===generation && context.side==='question' &&
                  context.viewRevision===revision && context.visibility==='preparing' &&
                  probes.length===2 && probes.every(message=>message.viewRevision===revision) &&
                  probes.some(message=>message.kind==='background' && message.request===background) &&
                  probes.some(message=>message.kind==='resources' && message.request===resources && message.purpose==='preload') &&
                  replies.length===2 && replies.every(reply=>reply===null));
                """, arguments: ["card": prepared.card, "generation": prepared.generation, "revision": context.viewRevision,
                                  "background": backgroundID, "resources": resourceID], world: CardPage.world)
        }
        #expect(delivered == "true")
        #expect(try await otherPage.string("return String(immediateProbeHost.messages.filter(message=>message.op==='probe').length);", world: CardPage.world) == "0")
        #expect(otherPage.hostReports.slotBackground == nil && otherPage.hostReports.slotResources == nil)
    }

    @Test func hostProbesMatchImmutableIdleSlotDeadlineCancellationAndPurpose() async throws {
        let (pair, _, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(HostReports.syntheticHost)
        let page = pair.idle
        let original = try #require(page.hostReports.context)
        var observed: [(CardHostContext?, CardHostProbeOutcome)] = []
        func completion(_ context: CardHostContext?, _ outcome: CardHostProbeOutcome) { observed.append((context, outcome)) }
        let first = try #require(page.hostReports.probe(.background, completion: completion))
        let second = try #require(page.hostReports.probe(.background, completion: completion))
        #expect(observed.count == 1 && observed[0].0 == original && observed[0].1 == .cancelled)
        let resource = try #require(page.hostReports.probe(.resources, purpose: .preload, completion: completion))
        // A fixed isolated round trip lets pending receive acknowledgements land.
        try await hostWait { observed.count == 1 }
        try await Task.sleep(for: .milliseconds(20))
        func background(_ request: UInt64) -> [String: Any] {
            HostReports.body("hostProbe", payload: ["kind": "background", "request": request, "rgba": [0.1, 0.2, 0.3, 1]], revision: original.viewRevision)
        }
        page.hostReports.receive(background(first)) // cancelled
        #expect(observed.count == 1)
        for (field, invalid): (String, Any) in [("card", 8), ("generation", 2), ("side", "answer"), ("viewRevision", original.viewRevision + 1)] {
            page.hostReports.receive(HostReports.replacing(background(second), field, invalid))
        }
        #expect(observed.count == 1)
        page.hostReports.receive(background(second))
        #expect(observed.count == 2 && observed[1].0 == original)
        #expect(observed[1].1 == .observed(.background(red: 0.1, green: 0.2, blue: 0.3, alpha: 1)))
        #expect(page.hostReports.slotBackground != nil && pair.current.hostReports.slotBackground == nil)
        page.hostReports.receive(background(second)) // duplicate
        #expect(observed.count == 2)
        var resources: [String: Any] = ["kind": "resources", "request": resource, "purpose": "warmup", "state": "ready",
                                        "imagesPending": 0, "fontsPending": 0, "stylesPending": 0]
        page.hostReports.receive(HostReports.body("hostProbe", payload: resources, revision: original.viewRevision))
        #expect(observed.count == 2) // purpose must match native request
        resources["purpose"] = "preload"
        page.hostReports.receive(HostReports.body("hostProbe", payload: resources, revision: original.viewRevision))
        #expect(observed.count == 3 && page.hostReports.slotResources != nil)
        var expired: [CardHostProbeOutcome] = []
        let timeout = try #require(page.hostReports.probe(.background, timeoutMs: 40) { _, outcome in expired.append(outcome) })
        try await hostWait { expired.count == 1 }
        #expect(expired == [.timeout])
        page.hostReports.receive(background(timeout))
        #expect(expired == [.timeout])
        #expect(try await page.string("return String(hostMessages.filter(x=>x.op==='probe'&&x.request===request).length);", arguments: ["request": timeout], world: CardPage.world) == "1")
        let pending = try #require(page.hostReports.probe(.background, completion: completion))
        page.hostReports.cancelProbe(.background)
        #expect(observed.last?.1 == .cancelled)
        page.hostReports.receive(background(pending))
        #expect(observed.count == 4)
        #expect(page.hostReports.probe(.background, timeoutMs: 0, completion: completion) == nil)
        #expect(page.hostReports.probe(.resources, timeoutMs: 2001, completion: completion) == nil)
        _ = page.hostReports.probe(.resources, purpose: .warmup, completion: completion)
        await pair.suspend()
        #expect(observed.last?.1 == .cancelled)
        #expect(page.hostReports.context == nil && page.hostReports.hitMap == nil && page.hostReports.slotBackground == nil)
        page.hostReports.receive(background(second))
        #expect(page.hostReports.slotBackground == nil)
    }

    @Test func supersededAsyncHostAcknowledgementsCannotAdmitStaleContextOrProbe() async throws {
        let (pair, _, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(HostReports.syntheticHost)
        let page = pair.idle
        let before = try #require(page.hostReports.context)
        // The function returns an actual boolean, while WebKit delivers the
        // native completion asynchronously. Delay that process round trip.
        try await page.run("""
            meguriReviewHost.receive = function(message) {
              hostMessages.push(message);
              if(message.op === 'context') {
                const end = performance.now() + 100;
                while(performance.now() < end) {}
              }
              return true;
            };
            """, world: CardPage.world)
        page.webView.contentRectangle = CGRect(x: 10, y: 20, width: 200, height: 300)
        let pendingPublication = Task { await page.hostReports.publish() }
        try await Task.sleep(for: .milliseconds(20))
        #expect(page.hostReports.context == nil)
        page.webView.contentRectangle = CGRect(x: 10, y: 20, width: 210, height: 310)
        await pendingPublication.value
        try await pair.installHostScript(HostReports.syntheticHost)
        let current = try #require(page.hostReports.context)
        #expect(current.viewRevision > before.viewRevision && current.contentRect.width == 210)
        try await page.run("""
            meguriReviewHost.receive = function(message) {
              hostMessages.push(message);
              if(message.op==='probe') {
                const end = performance.now() + 100;
                while(performance.now() < end) {}
              }
              return true;
            };
            """, world: CardPage.world)
        var results: [CardHostProbeOutcome] = []
        let request = try #require(page.hostReports.probe(.background) { _, outcome in results.append(outcome) })
        try await Task.sleep(for: .milliseconds(20))
        page.hostReports.receive(HostReports.body("hostProbe", payload: ["kind": "background", "request": request, "rgba": [0, 0, 0, 1]], revision: current.viewRevision))
        #expect(results.isEmpty) // even a result must wait for a true ack
        page.hostReports.invalidate()
        try await Task.sleep(for: .milliseconds(120))
        #expect(results == [.cancelled] && page.hostReports.slotBackground == nil)
    }

    @Test func hostMetadataReplyAndGenericRelayNeverGrantAdmission() async throws {
        let (pair, configuration, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(HostReports.syntheticHost)
        let page = pair.idle
        let prepared = try #require(page.update)
        _ = try pair.present(prepared)
        try await page.waitForShown(prepared)
        try await hostWait { page.hostReports.context != nil }
        let context = try #require(page.hostReports.context)
        var events: [CardHostEvent] = []
        pair.onHostEvent = { _, event in events.append(event) }
        final class Replies { var count = 0; var nulls = 0 }
        let replies = Replies()
        let input = HostReports.body(revision: context.viewRevision)
        func deliver(_ body: Any, name: String = "meguriReviewHost", world: String? = CardRuntime.interactionWorldName,
                     sender: CardPage? = nil, mainFrame: Bool = true, origin: URL? = MediaOrigin.url,
                     scheme: String = MediaOrigin.scheme, host: String = MediaOrigin.host, port: Int = 0) {
            let count = replies.count
            configuration.hostBridge.receive(name: name, world: world, body: body, page: sender ?? page,
                                             mainFrame: mainFrame, origin: origin, scheme: scheme, host: host, port: port) { value, error in
                replies.count += 1
                if value is NSNull && error == nil { replies.nulls += 1 }
            }
            #expect(replies.count == count + 1 && replies.count == replies.nulls)
        }
        deliver(input, world: WKContentWorld.page.name)
        deliver(input, world: "other.world")
        deliver(input, mainFrame: false)
        deliver(input, origin: URL(string: "meguri-media://media/_anki/page"))
        deliver(input, origin: URL(string: "meguri-media://user:secret@media/"))
        deliver(input, scheme: "https")
        deliver(input, host: "elsewhere")
        deliver(input, port: 1)
        deliver(input, name: "meguriCard")
        deliver(input, sender: pair.current === page ? pair.idle : pair.current)
        deliver(HostReports.replacing(input, "card", 8))
        deliver(HostReports.replacing(input, "payload", String(repeating: "x", count: 65_537)))
        deliver(["signal": "unknown"])
        #expect(events.isEmpty)
        for signal in ["hostInput", "hostHitMap", "hostProbe"] {
            configuration.bridge.receive(name: "meguriCard", world: CardRuntime.interactionWorldName,
                                         body: HostReports.replacing(input, "signal", signal), page: page)
        }
        #expect(events.isEmpty && page.hostReports.hitMap == nil)
        deliver(input)
        #expect(events.count == 1)
        deliver(input) // duplicate serial, still one actual null reply
        #expect(events.count == 1)
        // Actual WebKit transport: accepted/rejected reports all return JS null.
        let nulls = try await page.string("""
            const messages = [body, {signal:'unknown'}, 'bad', {payload:'x'.repeat(65537)}];
            const replies = await Promise.all(messages.map(m=>webkit.messageHandlers.meguriReviewHost.postMessage(m)));
            return String(replies.length===4 && replies.every(x=>x===null));
            """, arguments: ["body": HostReports.replacing(input, "serial", 2)], world: CardPage.world)
        #expect(nulls == "true")
        #expect(try await page.string("return typeof window.meguriReviewHost;") == "undefined")
        // A synthetic DOM event can reach the generic relay, never host admission.
        let count = events.count
        try await page.run("document.dispatchEvent(new CustomEvent('meguri:signal',{detail:JSON.stringify(body)}));", arguments: ["body": HostReports.replacing(input, "serial", 3)])
        try await Task.sleep(for: .milliseconds(20))
        #expect(events.count == count)
        page.hostReports.invalidate()
        #expect(events.last == .invalidated(viewRevision: page.hostReports.viewRevision))
        deliver(HostReports.replacing(input, "serial", 3))
        #expect(events.count == count + 1)
    }

    @Test func nativeGeometryZoomSwapAndSideChangeRevokeMapsSynchronously() async throws {
        let (pair, _, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(HostReports.syntheticHost)
        let page = pair.idle
        let update = try #require(page.update)
        // Idle maps/input cannot be cached for presentation.
        let idle = try #require(page.hostReports.context)
        page.hostReports.receive(HostReports.body("hostHitMap", payload: HostReports.map, revision: idle.viewRevision))
        #expect(page.hostReports.hitMap == nil)
        _ = try pair.present(update)
        #expect(page.hostReports.context == nil)
        try await page.waitForShown(update)
        try await hostWait { page.hostReports.context != nil }
        var context = try #require(page.hostReports.context)
        func map(_ context: CardHostContext) {
            page.hostReports.receive(HostReports.body("hostHitMap", payload: HostReports.map, revision: context.viewRevision))
        }
        map(context)
        #expect(page.hostReports.hitMap != nil)
        page.webView.frame.size.width += 10
        #expect(page.hostReports.hitMap == nil && page.hostReports.context == nil)
        await page.hostReports.publish()
        let resized = try #require(page.hostReports.context)
        #expect(resized.viewRevision > context.viewRevision)
        context = resized; map(context)
        page.webView.pageZoom = 1.5
        #expect(page.hostReports.hitMap == nil && page.hostReports.context == nil)
        await page.hostReports.publish()
        context = try #require(page.hostReports.context)
        #expect(context.pageZoom == 1.5)
        map(context)
        page.webView.contentRectangle = CGRect(x: 20, y: 30, width: 300, height: 400)
        #expect(page.hostReports.hitMap == nil && page.hostReports.context == nil)
        await page.hostReports.publish()
        context = try #require(page.hostReports.context); map(context)
        // Over-limit maps revoke a previously complete map conservatively.
        page.hostReports.receive(HostReports.body("hostHitMap", payload: HostReports.replacing(HostReports.map, "rects", Array(repeating: HostReports.rect, count: 257)), revision: context.viewRevision, serial: 2))
        #expect(page.hostReports.hitMap == nil)
        _ = try await pair.showAnswer()
        #expect(page.hostReports.hitMap == nil)
        let next = try await pair.prepare(CardPayload(cardID: 8, question: "next", answer: "answer"))
        _ = try pair.present(next)
        #expect(page.hostReports.context == nil && page.hostReports.hitMap == nil)
        #expect(pair.current.hostReports.hitMap == nil)
    }
}

extension WebKitSuites.BridgeAllowlistTests {
    @Test func templateErrorsRetainDistinctPrivateTextAndOnlyNativeTemplateIdentity() async throws {
        let fixture = MediaFixture()
        let configuration = CardWebConfiguration(store: .nonPersistent())
        let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved())
        defer { pair.close() }
        let page = pair.idle
        let payload = CardPayload(cardID: 7, ordinal: 4, templateDescriptor: "Native template", question: "q", answer: "a")
        let update = CardUpdate(card: 7, generation: 1, side: .answer)
        page.begin(payload, update: update)
        func send(_ message: String, kind: String = "exception", extra: [String: Any] = [:]) {
            let body: [String: Any] = ["signal": "error", "card": 7, "generation": 1, "side": "answer", "kind": kind,
                                       "phase": "later", "message": message, "line": 3, "source": "file:///private/secret.js"]
            configuration.bridge.receive(name: "meguriCard", world: CardRuntime.interactionWorldName,
                                         body: body.merging(extra, uniquingKeysWith: { _, new in new }), page: page)
        }
        let first = "TypeError: first exact private message", second = "TypeError: second exact private message"
        send(first); send(second); send("exact rejection reason", kind: "rejection")
        let errors = pair.diagnostics.templateErrors
        #expect(errors.map(\.message) == [first, second, "exact rejection reason"])
        #expect(errors.allSatisfy { $0.update == update && $0.templateOrdinal == 4 && $0.templateDescriptor == "Native template" && $0.line == 3 })
        send("page error", extra: ["templateOrdinal": 99])
        send("page error", extra: ["templateDescriptor": "Page-chosen template"])
        send("page error", extra: ["card": 8])
        #expect(pair.diagnostics.templateErrors.count == 3)
        send(String(repeating: "é", count: 3_000))
        #expect(pair.diagnostics.templateErrors.last?.message == String(repeating: "é", count: 2_000))
        for index in 0..<70 { send("rejection \(index)", kind: "rejection") }
        #expect(pair.diagnostics.templateErrors.count == 64)
        #expect(pair.diagnostics.entries.allSatisfy { $0.name.isEmpty })
        #expect(pair.diagnostics.entries.compactMap(\.script).allSatisfy { $0.source == "external-resource" })
        #expect(pair.diagnostics.entries.compactMap(\.script).allSatisfy { $0.errorClass == "TypeError" || $0.errorClass == "Error" })
    }
}

extension WebKitSuites.CardPagePairTests {
    @Test func realRuntimeErrorsKeepMessagesAndNativeTemplateOnBothSides() async throws {
        let fixture = MediaFixture()
        let pair = try await make(fixture)
        let window = SpikeWindow(pair.container)
        defer { pair.close(); window.close() }
        let html = "<script>throw new TypeError('first synthetic message')</script><script>throw new TypeError('second synthetic message')</script><script>Promise.reject('synthetic rejection')</script>"
        let update = try await pair.prepare(CardPayload(cardID: 7, ordinal: 3, templateDescriptor: "Synthetic native template", question: html, answer: html))
        try await hostWait { pair.diagnostics.templateErrors.count >= 3 }
        let questionErrors = pair.diagnostics.templateErrors
        #expect(questionErrors.contains { $0.message == "TypeError: first synthetic message" })
        #expect(questionErrors.contains { $0.message == "TypeError: second synthetic message" })
        #expect(questionErrors.contains { $0.kind == "rejection" && $0.message == "synthetic rejection" })
        #expect(questionErrors.allSatisfy { $0.update == update && $0.templateOrdinal == 3 && $0.templateDescriptor == "Synthetic native template" })
        _ = try pair.present(update)
        try await pair.current.waitForShown(update)
        _ = try await pair.showAnswer()
        try await hostWait { pair.diagnostics.templateErrors.contains { $0.update.side == .answer && $0.kind == "rejection" } }
        #expect(pair.diagnostics.templateErrors.filter { $0.update.side == .answer }.allSatisfy { $0.update.card == 7 && $0.templateOrdinal == 3 && $0.templateDescriptor == "Synthetic native template" })
    }

    @Test func terminatedSettledPreparedPageCannotPresentOrRegainReadiness() async throws {
        let (pair, configuration, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(HostReports.syntheticHost)
        let page = pair.idle
        let prepared = try #require(page.update)
        #expect(page.settled && page.hostReports.context != nil)
        page.visibleToken = 5
        page.webViewWebContentProcessDidTerminate(page.webView)
        #expect(!page.settled && page.visibleToken == nil && page.hostReports.context == nil)
        #expect(throws: CardPage.Failure.self) { try pair.present(prepared) }
        for signal in ["settled", "shown", "visible"] {
            var body: [String: Any] = ["signal": signal, "card": prepared.card, "generation": prepared.generation, "side": prepared.sideName]
            if signal == "visible" { body["token"] = 5 }
            configuration.bridge.receive(name: "meguriCard", world: CardRuntime.interactionWorldName, body: body, page: page)
        }
        #expect(!page.settled && page.visibleToken == nil && page.hostReports.context == nil)
        #expect(throws: CardPage.Failure.self) { try pair.present(prepared) }
        await #expect(throws: CardPage.Failure.self) { try await page.waitForSettled(prepared) }
    }
}

extension WebKitSuites.CardPagePairTests {
    @Test func synchronousProbeResultStillRequiresTrueNativeAcknowledgement() async throws {
        let (pair, _, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        // A synthetic isolated producer for transport testing only. Reports
        // use the exact context supplied by native, never a page DOM relay.
        let source = """
            window.hostContext = null;
            window.hostAck = true;
            window.meguriReviewHost = { receive(message) {
              if(message.op==='clear') { hostContext=null; return true; }
              if(message.op==='context') { hostContext=message; return true; }
              const start = hostContext;
              webkit.messageHandlers.meguriReviewHost.postMessage({
                signal:'hostProbe',version:1,card:start.card,generation:start.generation,side:start.side,
                viewRevision:start.viewRevision,
                payload:{kind:'background',request:message.request,rgba:[0,0.5,1,0]}
              });
              return hostAck;
            }};
            """
        try await pair.installHostScript(source)
        let page = pair.idle
        let original = try #require(page.hostReports.context)
        var results: [CardHostProbeOutcome] = []
        _ = page.hostReports.probe(.background) { context, outcome in
            #expect(context == original)
            results.append(outcome)
        }
        try await hostWait { results.count == 1 }
        #expect(results == [.observed(.background(red: 0, green: 0.5, blue: 1, alpha: 0))])
        try await page.run("hostAck = false;", world: CardPage.world)
        _ = page.hostReports.probe(.background) { _, outcome in results.append(outcome) }
        try await hostWait { results.count == 2 }
        #expect(results.last == .unavailable && page.hostReports.context == nil)
    }

    @Test func nativeInvalidGeometryAndKnownScrollInvalidateHostEpoch() async throws {
        let (pair, _, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(HostReports.syntheticHost)
        let page = pair.idle
        let original = try #require(page.hostReports.context)
        page.webView.contentRectangle = .zero
        #expect(page.hostReports.context == nil)
        await page.hostReports.publish()
        #expect(page.hostReports.context == nil)
        page.webView.contentRectangle = CGRect(x: 0, y: 0, width: 1_000_001, height: 600)
        try await pair.installHostScript(HostReports.syntheticHost)
        #expect(page.hostReports.context == nil)
        page.webView.contentRectangle = nil
        try await pair.installHostScript(HostReports.syntheticHost)
        #expect(try #require(page.hostReports.context).viewRevision > original.viewRevision)
        let next = try await pair.prepare(CardPayload(cardID: 7, question: "<div style='height:4000px'>scroll content</div>", answer: "a"))
        _ = try pair.present(next)
        try await page.waitForShown(next)
        try await hostWait { page.hostReports.context != nil }
        let before = try #require(page.hostReports.context)
        // This card's generation is not the fixture's default generation.
        // Use its actual binding for a complete map before native invalidation.
        var map = HostReports.body("hostHitMap", payload: HostReports.map, revision: before.viewRevision)
        map["card"] = next.card; map["generation"] = next.generation
        page.hostReports.receive(map)
        #expect(page.hostReports.hitMap != nil)
        // H5/H10: U67 observes native scroll input. Programmatic DOM scroll
        // freshness belongs to the future U176/U180 producer. Never post or
        // drive workstation input: deliver an offscreen scroll event directly.
        #if os(macOS)
        let event = try #require(CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: -100, wheel2: 0, wheel3: 0))
        let scroll = try #require(NSEvent(cgEvent: event))
        page.webView.scrollWheel(with: scroll)
        #expect(page.hostReports.context == nil)
        #expect(page.hostReports.viewRevision > before.viewRevision)
        #else
        page.webView.scrollView.contentOffset.y += 100
        #expect(page.hostReports.context == nil)
        #expect(page.hostReports.viewRevision > before.viewRevision)
        #endif
        #expect(page.hostReports.hitMap == nil)
        let refreshed = try #require(await page.hostReports.refreshContext(for: next))
        #expect(refreshed.update == next && refreshed.viewRevision > before.viewRevision)
        #expect(page.hostReports.hitMap == nil) // acknowledgement is not map freshness
        #expect(await page.hostReports.refreshContext(for: original.update) == nil)
    }
}

extension WebKitSuites.CardPagePairTests {
    @Test func replacingProbeRemainsBoundedWhenCancellationOwnerReenters() async throws {
        let (pair, _, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(HostReports.syntheticHost)
        let page = pair.idle
        let context = try #require(page.hostReports.context)
        var first: [CardHostProbeOutcome] = [], second: [CardHostProbeOutcome] = []
        _ = page.hostReports.probe(.background) { _, outcome in
            first.append(outcome)
            if outcome == .cancelled { page.hostReports.cancelProbe(.background) }
        }
        let replacement = try #require(page.hostReports.probe(.background) { _, outcome in second.append(outcome) })
        #expect(first == [.cancelled] && second == [.cancelled])
        page.hostReports.receive(HostReports.body("hostProbe", payload: ["kind": "background", "request": replacement, "rgba": [0, 0, 0, 1]], revision: context.viewRevision))
        #expect(first == [.cancelled] && second == [.cancelled] && page.hostReports.slotBackground == nil)
    }
}

extension WebKitSuites.CardPagePairTests {
    @Test func publicMutationAndCommitBoundariesHaveAcknowledgedHostContext() async throws {
        let (pair, _, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(HostReports.syntheticHost)
        let page = pair.idle
        let update = try #require(page.update)
        let committed = AsyncStream<CardHostContext?>.makeStream(bufferingPolicy: .bufferingOldest(1))
        defer { committed.continuation.finish() }
        pair.onCommitted = { identity, _ in
            #expect(identity == update)
            committed.continuation.yield(page.hostReports.context)
        }
        _ = try pair.present(update)
        let presented = try await Spike.deadline("acknowledged post-commit context", timeout: .seconds(3)) {
            for await context in committed.stream { return context }
            throw Spike.Failure("missing commit callback")
        }
        let context = try #require(presented)
        #expect(context.update == update && context.visibility == .presented)
        try await pair.setAppearance(.dark)
        var previous = try #require(page.hostReports.context)
        #expect(previous.viewRevision > context.viewRevision)
        try await pair.setCardState(flag: 1, marked: true)
        var next = try #require(page.hostReports.context)
        #expect(next.viewRevision > previous.viewRevision)
        previous = next
        _ = try await pair.mediaArrived(["absent.png"])
        next = try #require(page.hostReports.context)
        #expect(next.viewRevision > previous.viewRevision)
        _ = try await pair.showAnswer()
        let answer = try #require(page.hostReports.context)
        #expect(answer.update.side == .answer && answer.visibility == .presented && answer.viewRevision > next.viewRevision)
    }

    @Test func cancellationDuringPreparedContextAcknowledgementCannotReturnReadiness() async throws {
        let fixture = MediaFixture()
        let pair = try await make(fixture)
        let window = SpikeWindow(pair.container)
        defer { pair.close(); window.close() }
        var preparation: Task<CardUpdate, any Error>?
        // Reject the actual context request and cancel its owner synchronously
        // during publication, before the awaiting prepare can return readiness.
        pair.onHostEvent = { _, event in
            if case .unavailable = event { preparation?.cancel() }
        }
        try await pair.installHostScript("""
            window.meguriReviewHost={receive(message){
              if(message.op==='context') return false;
              return true;
            }};
            """)
        let task = Task { try await pair.prepare(CardPayload(cardID: 80, question: "q", answer: "a")) }
        preparation = task
        await #expect(throws: (any Error).self) { try await task.value }
        #expect(pair.idle.hostReports.context == nil)
    }
}

extension WebKitSuites.CardPagePairTests {
    @Test func unavailableHostDuringPrepareNeverStopsCardRendering() async throws {
        for result in ["false", "1", "undefined", "throw new Error('synthetic')"] {
            let fixture = MediaFixture()
            let pair = try await make(fixture)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            // Clear succeeds at installation; the real context call fails.
            let body = result.hasPrefix("throw") ? result : "return " + result
            try await pair.installHostScript("window.meguriReviewHost={receive(message){if(message.op==='clear')return true;" + body + ";}};")
            let update = try await pair.prepare(CardPayload(cardID: 90, question: "<p>still rendered</p>", answer: "a"))
            #expect(pair.idle.update == update && pair.idle.settled)
            #expect(pair.idle.hostReports.context == nil)
            #expect(try await pair.idle.string("return document.getElementById('qa').textContent;") == "still rendered")
            var results: [CardHostProbeOutcome] = []
            #expect(pair.idle.hostReports.probe(.background) { _, result in results.append(result) } == nil)
            #expect(results == [.unavailable])
        }
    }
}

// U176 producer continuation, review R3. These exercise the bundled producer;
// the earlier synthetic receiver cases continue to test transport separately.
extension WebKitSuites.CardPagePairTests {
    private func productionProbe(_ page: CardPage, _ kind: CardHostProbeKind,
                                 purpose: CardHostResourcePurpose? = nil, timeoutMs: Int? = nil) async throws -> CardHostProbeOutcome {
        var outcomes: [CardHostProbeOutcome] = []
        let context = page.hostReports.context
        page.hostReports.probe(kind, purpose: purpose, timeoutMs: timeoutMs) { received, outcome in
            #expect(received == context)
            outcomes.append(outcome)
        }
        try await hostWait { outcomes.count == 1 }
        return try #require(outcomes.first)
    }

    private func recordProductionReports(_ page: CardPage) async throws {
        try await page.run("""
            window.producerReports = [];
            window.producerHandler = webkit.messageHandlers.meguriReviewHost;
            const handler = producerHandler, post = handler.postMessage;
            handler.postMessage = function(message) {
              producerReports.push(message);
              return post.call(handler, message);
            };
            """, world: CardPage.world)
    }

    private nonisolated static let bodyColorSamples: [(String, [Double])] = [
        ("rgb(24 128 240)", [24.0 / 255, 128.0 / 255, 240.0 / 255, 1.0]),
        ("transparent", [0.0, 0.0, 0.0, 0.0]),
        ("color(srgb 0.2 0.4 0.6 / 0.5)", [0.2, 0.4, 0.6, 0.5]),
        ("oklch(62.7955% 0.257683 29.2339)", [1.0, 0.0, 0.0, 1.0]),
        ("color(display-p3 1 0 0)", [1.0, 0.0, 0.0, 1.0]),
    ]
    @Test(arguments: bodyColorSamples)
    func productionHostObservesComputedBodySRGB(color: String, expected: [Double]) async throws {
        let (pair, _, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        let page = pair.idle
        try await page.run("document.body.style.backgroundColor = color;", arguments: ["color": color])
        let outcome = try await productionProbe(page, .background)
        guard case let .observed(.background(red, green, blue, alpha)) = outcome else {
            Issue.record("Missing actual computed BODY background: \(outcome)"); return
        }
        for (actual, wanted) in zip([red, green, blue, alpha], expected) {
            // Allow two byte steps for WebKit's pixel quantization and alpha
            // conversion when comparing the returned sRGB components.
            #expect(abs(actual - wanted) <= 2.0 / 255)
        }
        #expect(page.hostReports.slotBackground != nil)
        #expect(!CardRuntime.assetPaths.contains("review-host.js"))
        #expect(CardRuntime.asset(at: "review-host.js") == nil)
        #expect(try await page.string("return typeof meguriReviewHost;") == "undefined")
    }

    @Test func productionHostReadyResourcesAndBothPurposesUseRealContent() async throws {
        let fixture = MediaFixture()
        fixture.add("/host.png", TestMedia.png(), "image/png")
        fixture.add("/host.ttf", TestMedia.font(), "font/ttf")
        fixture.add("/import.css", text: "#qa { font-family: HostFixture; }", "text/css")
        fixture.add("/host.css", text: "@import url('import.css');", "text/css")
        let pair = try await make(fixture)
        let window = SpikeWindow(pair.container)
        defer { pair.close(); window.close() }
        try await pair.load()
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        let update = try await pair.prepare(CardPayload(cardID: -9_007_199_254_740_991, question: """
            <style>@font-face { font-family: HostFixture; src: url('host.ttf'); }</style>
            <link rel=stylesheet href=host.css><img src=host.png><p>Local resources</p>
            <script>
              // Page-world replacements must not affect isolated observations.
              window.getComputedStyle = function(){ throw new Error('page replacement'); };
              HTMLImageElement.prototype.decode = function(){ return Promise.reject('page replacement'); };
            </script>
            """, answer: "answer"))
        let page = pair.idle
        for purpose: CardHostResourcePurpose in [.warmup, .preload] {
            #expect(try await productionProbe(page, .resources, purpose: purpose) ==
                .observed(.resources(purpose: purpose, state: .ready, imagesPending: 0, fontsPending: 0, stylesPending: 0)))
        }
        #expect(page.hostReports.context?.update == update)
        #expect(page.hostReports.context?.visibility == .preparing)
        #expect(try await page.string("return String(document.querySelector('img').naturalWidth);") == "4")
        #expect(try await page.string("return document.fonts.status;") == "loaded")
    }

    @Test func productionReceiverRejectsMalformedObsoleteAndDuplicateABI() async throws {
        let (pair, _, _, window) = try await hostPair()
        defer { pair.close(); window.close() }
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        let page = pair.idle
        let context = try #require(page.hostReports.context)
        let result = try await page.string("""
            const receive = meguriReviewHost.receive;
            const revision = currentRevision;
            const context = {op:'context',card:7,generation:1,side:'question',viewRevision:revision,
              visibility:'preparing',bounds:{width:400,height:600},pageZoom:1};
            const probe = {op:'probe',kind:'resources',request:50,viewRevision:revision,timeoutMs:100,purpose:'warmup'};
            const invalid = [null, [], true, {op:'unknown'}, context,
              {op:'clear',viewRevision:revision}, {op:'clear',viewRevision:revision-1},
              {...probe,extra:true}, {...probe,timeoutMs:true}, {...probe,timeoutMs:0},
              {...probe,timeoutMs:2001}, {...probe,timeoutMs:1.5}, {...probe,kind:'input'},
              {...probe,purpose:'answer'}, {...probe,viewRevision:revision-1},
              {...probe,request:0}, {...probe,request:9007199254740992},
              {...probe,kind:'background'}, {...probe,purpose:undefined}];
            if (!invalid.every(message => receive(message) === false)) return 'invalid accepted';
            if (!receive(probe) || receive(probe)) return 'duplicate accepted';
            if (!receive({op:'clear',viewRevision:revision+1})) return 'clear rejected';
            const next = {...context,viewRevision:revision+1};
            const badContexts = [{...next,card:'7'}, {...next,card:9007199254740992},
              {...next,generation:-1}, {...next,generation:true}, {...next,side:'front'},
              {...next,visibility:'idle'}, {...next,bounds:{width:0,height:1}},
              {...next,bounds:{width:1000001,height:1}}, {...next,bounds:{width:1,height:1,extra:0}},
              {...next,pageZoom:0}, {...next,pageZoom:1001}, {...next,pageZoom:Infinity},
              {...next,viewRevision:revision}, {...next,extra:1}];
            if (!badContexts.every(message => receive(message) === false)) return 'context accepted';
            if (receive({...probe,request:51,viewRevision:revision+1})) return 'missing context accepted';
            if (!receive(next) || receive(next)) return 'duplicate context accepted';
            return Object.keys(meguriReviewHost).join(',');
            """, arguments: ["currentRevision": context.viewRevision], world: CardPage.world)
        #expect(result == "receive")
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        #expect(page.hostReports.context != nil)
    }

    @Test(arguments: ["image", "font", "style", "preload"])
    func productionPendingResourcesTimeoutWithoutSyntheticReadiness(kind: String) async throws {
        let fixture = MediaFixture()
        let server = TestMediaServer()
        server.holdsEverything = true
        let configuration = CardWebConfiguration(store: .nonPersistent())
        configuration.configuration.setURLSchemeHandler(server, forURLScheme: WebTestHarness.remoteScheme)
        let pair = try await make(fixture, configuration: configuration)
        let window = SpikeWindow(pair.container)
        defer { pair.close(); window.close() }
        try await pair.load()
        for page in [pair.current, pair.idle] { try await recordProductionReports(page) }
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        _ = try await pair.prepare(CardPayload(cardID: 7, question: "<p>Pending fixture</p>", answer: "a"))
        let page = pair.idle
        try await page.run("""
            if (kind === 'image') {
              const image = document.createElement('img'); image.id = 'pending';
              image.src = 'meguri-remote://media/held.png'; document.body.append(image);
            } else if (kind === 'style') {
              const link = document.createElement('link'); link.id = 'pending';
              link.rel = 'stylesheet'; link.href = 'meguri-remote://media/held.css'; document.head.append(link);
            } else if (kind === 'preload') {
              const stylesheetPreload = document.createElement('link');
              stylesheetPreload.id = 'pending';
              stylesheetPreload.rel = 'preload';
              stylesheetPreload.as = 'style';
              stylesheetPreload.href = 'meguri-remote://media/held-preload.css';
              document.head.append(stylesheetPreload);
            } else {
              window.pendingFace = new FontFace('HostPending','url(meguri-remote://media/held.ttf)');
              document.fonts.add(pendingFace); pendingFace.load().catch(function(){});
            }
            """, arguments: ["kind": kind])
        var outcomes: [CardHostProbeOutcome] = []
        page.hostReports.probe(.resources, purpose: .preload, timeoutMs: 300) { _, outcome in outcomes.append(outcome) }
        try await page.run("""
            const pending = document.getElementById('pending');
            if (pending) { pending.dispatchEvent(new Event('load')); pending.dispatchEvent(new Event('error')); }
            document.fonts.dispatchEvent(new Event('loadingdone'));
            document.fonts.dispatchEvent(new Event('loadingerror'));
            """)
        #expect(outcomes.isEmpty)
        try await hostWait { outcomes.count == 1 }
        #expect(outcomes == [.timeout])
        #expect(page.hostReports.slotResources == nil)
        // Native time starts before script delivery. Also cross the script's
        // own bounded deadline before making the DOM ready.
        try await page.run("await new Promise(resolve => setTimeout(resolve, 300));", world: CardPage.world)
        // Make the observed DOM ready after expiry: the old observer cannot
        // send a late success or create native budgetExpired/paint evidence.
        try await page.run("""
            document.getElementById('pending')?.remove();
            if (window.pendingFace) document.fonts.delete(pendingFace);
            """)
        #expect(try await page.string("return String(producerReports.length);", world: CardPage.world) == "0")
        #expect(try await productionProbe(page, .resources, purpose: .warmup) ==
            .observed(.resources(purpose: .warmup, state: .ready, imagesPending: 0, fontsPending: 0, stylesPending: 0)))
        #expect(outcomes == [.timeout])
    }

    @Test func productionFailureReplacementClearAndReinstallReleaseOldWork() async throws {
        let fixture = MediaFixture()
        let server = TestMediaServer()
        server.hold("/held.png")
        let configuration = CardWebConfiguration(store: .nonPersistent())
        configuration.configuration.setURLSchemeHandler(server, forURLScheme: WebTestHarness.remoteScheme)
        let pair = try await make(fixture, configuration: configuration)
        let window = SpikeWindow(pair.container)
        defer { pair.close(); window.close() }
        try await pair.load()
        let page = pair.idle
        try await recordProductionReports(page)
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        _ = try await pair.prepare(CardPayload(cardID: 7, question: "fixture", answer: "answer"))
        // Native WebKit properties establish failure even when the error
        // happened before the resource request was installed.
        try await page.run("""
            const image = document.createElement('img'); image.id='broken';
            image.src='missing-host-fixture.png'; document.body.append(image);
            await new Promise(resolve => { if(image.complete) resolve(); else image.onerror=resolve; });
            """)
        #expect(try await productionProbe(page, .resources, purpose: .warmup) ==
            .observed(.resources(purpose: .warmup, state: .failed, imagesPending: 0, fontsPending: 0, stylesPending: 0)))
        try await page.run("""
            document.getElementById('broken').remove();
            window.failedFace=new FontFace('HostFailed','url(meguri-remote://media/missing.ttf)');
            document.fonts.add(failedFace); await failedFace.load().catch(function(){});
            """)
        #expect(try await productionProbe(page, .resources, purpose: .warmup) ==
            .observed(.resources(purpose: .warmup, state: .failed, imagesPending: 0, fontsPending: 0, stylesPending: 0)))
        try await page.run("""
            document.fonts.delete(failedFace);
            const image=document.createElement('img'); image.id='pending';
            image.src='meguri-remote://media/held.png'; document.body.append(image);
            """)
        var first: [CardHostProbeOutcome] = [], second: [CardHostProbeOutcome] = []
        page.hostReports.probe(.resources, purpose: .warmup) { _, outcome in first.append(outcome) }
        // Fixed WebKit round trips allow the actual request/observer to start;
        // the controlled resource cannot finish during them.
        try await page.run("await new Promise(resolve => setTimeout(resolve, 0));", world: CardPage.world)
        page.hostReports.probe(.resources, purpose: .preload) { _, outcome in second.append(outcome) }
        try await page.run("await new Promise(resolve => setTimeout(resolve, 0));", world: CardPage.world)
        #expect(first == [.cancelled] && second.isEmpty)
        page.hostReports.invalidate()
        #expect(second == [.cancelled])
        await page.hostReports.publish() // includes acknowledgement after clear
        try await page.run("document.getElementById('pending').remove(); window.previousHost=meguriReviewHost;", world: CardPage.world)
        #expect(try await page.string("return String(producerReports.length);", world: CardPage.world) == "2")
        try await page.run("""
            const image=document.createElement('img'); image.id='pending';
            image.src='meguri-remote://media/held.png'; document.body.append(image);
            """)
        var reinstalled: [CardHostProbeOutcome] = []
        page.hostReports.probe(.resources, purpose: .preload) { _, outcome in reinstalled.append(outcome) }
        try await page.run("await new Promise(resolve => setTimeout(resolve, 0));", world: CardPage.world)
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        #expect(reinstalled == [.cancelled])
        #expect(try await page.string("return String(previousHost.receive({op:'clear',viewRevision:9007199254740991}));", world: CardPage.world) == "false")
        try await page.run("document.getElementById('pending').remove();")
        #expect(try await productionProbe(page, .resources, purpose: .preload) ==
            .observed(.resources(purpose: .preload, state: .ready, imagesPending: 0, fontsPending: 0, stylesPending: 0)))
        #expect(try await page.string("return String(producerReports.length);", world: CardPage.world) == "3")
        let next = try await pair.prepare(CardPayload(cardID: 8, question: "next", answer: "answer"))
        #expect(page.hostReports.slotResources == nil && page.hostReports.context?.update == next)
        #expect(first == [.cancelled] && second == [.cancelled] && reinstalled == [.cancelled])
    }

    @Test func productionStylesheetReportsOnlyObservedFailure() async throws {
        let fixture = MediaFixture()
        let server = TestMediaServer()
        server.delay("/missing.css", by: .milliseconds(400))
        let configuration = CardWebConfiguration(store: .nonPersistent())
        configuration.configuration.setURLSchemeHandler(server, forURLScheme: WebTestHarness.remoteScheme)
        let pair = try await make(fixture, configuration: configuration)
        let window = SpikeWindow(pair.container)
        defer { pair.close(); window.close() }
        try await pair.load()
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        _ = try await pair.prepare(CardPayload(cardID: 7, question: "fixture", answer: "answer"))
        let page = pair.idle
        try await page.run("""
            const link=document.createElement('link'); link.rel='stylesheet';
            link.href='meguri-remote://media/missing.css'; document.head.append(link);
            """)
        #expect(try await productionProbe(page, .resources, purpose: .preload) ==
            .observed(.resources(purpose: .preload, state: .failed, imagesPending: 0, fontsPending: 0, stylesPending: 1)))
    }

    @Test func productionLoadedOpaqueStylesheetRemainsUnavailable() async throws {
        let fixture = MediaFixture()
        let server = TestMediaServer()
        server.add("/opaque.css", text: "body { --host-cssom-fixture: loaded; }", "text/css")
        let configuration = CardWebConfiguration(store: .nonPersistent())
        configuration.configuration.setURLSchemeHandler(server, forURLScheme: WebTestHarness.remoteScheme)
        let pair = try await make(fixture, configuration: configuration)
        let window = SpikeWindow(pair.container)
        defer { pair.close(); window.close() }
        try await pair.load()
        let page = pair.idle
        try await recordProductionReports(page)
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        _ = try await pair.prepare(CardPayload(cardID: 7, question: "fixture", answer: "answer"))
        try await page.run("""
            const sheet = document.createElement('link');
            sheet.id = 'opaque'; sheet.rel = 'stylesheet';
            sheet.href = 'meguri-remote://media/opaque.css';
            await new Promise((resolve, reject) => {
              sheet.onload = resolve; sheet.onerror = reject; document.head.appendChild(sheet);
            });
            """)
        #expect(try await page.string("return getComputedStyle(document.body).getPropertyValue('--host-cssom-fixture').trim();") == "loaded")
        #expect(try await page.string("""
            try { document.getElementById('opaque').sheet.cssRules; return 'readable'; }
            catch (error) { return error.name; }
            """) == "SecurityError")
        #expect(try await productionProbe(page, .resources, purpose: .warmup, timeoutMs: 300) == .timeout)
        #expect(page.hostReports.slotResources == nil)
        #expect(try await page.string("return String(producerReports.length);", world: CardPage.world) == "0")
    }

    @Test(arguments: ["srcset", "media", "type", "replace"])
    func productionPictureSelectionDecodesEveryNativeCurrentSource(change: String) async throws {
        let fixture = MediaFixture()
        fixture.add("/first.png", TestMedia.png(), "image/png")
        fixture.add("/second.png", TestMedia.png(width: 8), "image/png")
        let server = TestMediaServer()
        server.hold("/held.css")
        let configuration = CardWebConfiguration(store: .nonPersistent())
        configuration.configuration.setURLSchemeHandler(server, forURLScheme: WebTestHarness.remoteScheme)
        let pair = try await make(fixture, configuration: configuration)
        let window = SpikeWindow(pair.container)
        defer { pair.close(); window.close() }
        try await pair.load()
        let page = pair.idle
        // Observe calls while returning WebKit's original decode promise.
        // This neither resolves nor replaces native decode readiness.
        try await page.run("""
            window.nativeDecodeSelections = [];
            const decode = HTMLImageElement.prototype.decode;
            const selected = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'currentSrc').get;
            HTMLImageElement.prototype.decode = function () {
              nativeDecodeSelections.push(selected.call(this));
              return decode.call(this);
            };
            """, world: CardPage.world)
        try await recordProductionReports(page)
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        _ = try await pair.prepare(CardPayload(cardID: 7, question: """
            <picture><source id=choice srcset=first.png type=image/png media=all>
              <source srcset=second.png><img id=responsive src=first.png></picture>
            """, answer: "answer"))
        try await page.run("await document.getElementById('responsive').decode();")
        try await page.run("""
            const blocker = document.createElement('link'); blocker.id = 'blocker';
            blocker.rel = 'stylesheet'; blocker.href = 'meguri-remote://media/held.css';
            document.head.appendChild(blocker);
            """)
        var outcomes: [CardHostProbeOutcome] = []
        page.hostReports.probe(.resources, purpose: .preload, timeoutMs: 2_000) { _, outcome in outcomes.append(outcome) }
        try await CardBehaviorFixture.until("first native image decode") {
            try await page.string("return String(nativeDecodeSelections.length);", world: CardPage.world) == "1"
        }
        #expect(outcomes.isEmpty)
        try await page.run("""
            const image = document.getElementById('responsive');
            const source = document.getElementById('choice');
            await new Promise((resolve, reject) => {
              image.onload = resolve; image.onerror = reject;
              if (change === 'replace') {
                const replacement = document.createElement('source');
                replacement.setAttribute('srcset', 'second.png'); source.replaceWith(replacement);
              } else {
                const values = {srcset: 'second.png', media: 'not all', type: 'image/unsupported-host-fixture'};
                source.setAttribute(change, values[change]);
              }
            });
            await image.decode();
            """, arguments: ["change": change])
        #expect(try await page.string("return String(document.getElementById('responsive').naturalWidth);") == "8")
        try await CardBehaviorFixture.until("new engine-selected resource decoded by the producer") {
            try await page.string("""
                const selected = document.getElementById('responsive').currentSrc;
                return String(nativeDecodeSelections.length >= 2 && nativeDecodeSelections.at(-1) === selected);
                """, world: CardPage.world) == "true"
        }
        #expect(outcomes.isEmpty)
        #expect(try await page.string("return document.getElementById('responsive').getAttribute('src');") == "first.png")
        #expect(try await page.string("return String(producerReports.length);", world: CardPage.world) == "0")
        try await page.run("document.getElementById('blocker').remove();")
        try await hostWait { outcomes.count == 1 }
        #expect(outcomes == [.observed(.resources(purpose: .preload, state: .ready,
            imagesPending: 0, fontsPending: 0, stylesPending: 0))])
    }

    @Test func productionHostIsPairOnlyAndPreservesExactSafeIdentity() async throws {
        let (pair, configuration, fixture, window) = try await hostPair()
        let other = CardPagePair(configuration: configuration, resolver: try await fixture.resolved())
        let otherWindow = SpikeWindow(other.container)
        defer { pair.close(); other.close(); window.close(); otherWindow.close() }
        let page = pair.idle
        try await recordProductionReports(page)
        try await pair.installHostScript(CardRuntime.reviewHostScript)
        try await other.load()
        for otherPage in [other.current, other.idle] {
            #expect(try await otherPage.string("return typeof meguriReviewHost;", world: CardPage.world) == "undefined")
        }
        for own in [pair.current, pair.idle] {
            #expect(try await own.string("return typeof meguriReviewHost.receive;", world: CardPage.world) == "function")
            #expect(try await own.string("return typeof meguriReviewHost;") == "undefined")
        }
        let revision = try #require(page.hostReports.context).viewRevision
        try await page.run("""
            document.dispatchEvent(new CustomEvent('meguri:signal', {detail:JSON.stringify({
              signal:'hostProbe',version:1,card:7,generation:1,side:'question',viewRevision:revision,
              payload:{kind:'background',request:1,rgba:[1,0,0,1]}
            })}));
            """, arguments: ["revision": revision])
        #expect(try await page.string("return String(producerReports.length);", world: CardPage.world) == "0")
        #expect(try await page.string("""
            const receive=meguriReviewHost.receive;
            const viewRevision=revision+1;
            if(!receive({op:'clear',viewRevision})) return 'clear failed';
            const context={op:'context',card:-9007199254740991,generation:9007199254740991,
              side:'answer',viewRevision,visibility:'preparing',bounds:{width:400,height:600},pageZoom:1};
            if(!receive(context)) return 'context failed';
            // The receiver must copy context, not retain the mutable argument.
            context.card=1; context.generation=1; context.side='question';
            if(!receive({op:'probe',kind:'background',request:1,viewRevision,timeoutMs:250})) return 'probe failed';
            const message=producerReports[0];
            return [message.card,message.generation,message.side,message.viewRevision].join('/');
            """, arguments: ["revision": revision], world: CardPage.world) ==
            "-9007199254740991/9007199254740991/answer/\(revision + 1)")
        #expect(page.hostReports.slotBackground == nil, "Native must drop the deliberately unmatched test context")
    }
}
