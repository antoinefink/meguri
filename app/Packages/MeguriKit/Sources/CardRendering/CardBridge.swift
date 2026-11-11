import CoreBridge
import CoreFoundation
import Foundation
#if DEBUG
import OSLog
#endif
import WebKit

public nonisolated enum CardBridgeAction: Equatable, Sendable {
    case reveal
    case play(CardRenderSide, Int)
    case again
    case good
}

/// The only route from WebKit messages to a page. The controller retains this
/// router keeps weak page references.
public final class CardBridge: NSObject, WKScriptMessageHandlerWithReply {
#if DEBUG
    private static let startupLogger = Logger(subsystem: "Meguri", category: "ReviewStartup")
#endif

    /// Fixed technical labels only; never derive a reason from message content.
    public nonisolated enum RejectionReason: String, CaseIterable, Sendable {
        case legacyPlatform, world, body, bodySize, legacyValue, card
        case confirmationToken, confirmationShape, confirmationValue
        case signalChannel, generation, olderGeneration, futureGeneration, side, signalType
        case signalFields, signalValue, unknownSignal
    }

    private struct WeakPage { weak var value: CardPage? }
    private var pages: [ObjectIdentifier: WeakPage] = [:]
    func register(_ page: CardPage) { pages[ObjectIdentifier(page.webView)] = WeakPage(value: page) }
    func unregister(_ page: CardPage) { pages[ObjectIdentifier(page.webView)] = nil }

    public func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage,
                                      replyHandler: @escaping @MainActor @Sendable (Any?, String?) -> Void) {
        defer { replyHandler(nil, nil) }
        guard let webView = message.webView, let page = pages[ObjectIdentifier(webView)]?.value else { return }
        let origin = message.frameInfo.securityOrigin
        guard message.frameInfo.isMainFrame, origin.protocol == MediaOrigin.scheme, origin.host == MediaOrigin.host,
              origin.port == 0, let url = message.frameInfo.request.url, MediaOrigin.path(url) == "/" else {
#if DEBUG
            Self.startupLogger.notice("bridge.originRejected")
            if origin.protocol.isEmpty && origin.host.isEmpty {
                Self.startupLogger.notice("bridge.originOpaque")
            }
            if message.frameInfo.isMainFrame {
                Self.startupLogger.notice("bridge.mainFrame")
            }
            if let url = message.frameInfo.request.url, MediaOrigin.path(url) == "/" {
                Self.startupLogger.notice("bridge.rootPath")
            }
#endif
            page.diagnostics.record("bridgeOrigin", update: page.update)
            return
        }
        receive(name: message.name, world: message.world.name, body: message.body, page: page)
    }

    /// Metadata admission is above. Keeping body validation separate permits
    /// tests with ordinary values instead of WKFrameInfo objects.
    func receive(name: String, world: String?, body: Any, page: CardPage) {
        guard let update = page.update, let payload = page.payload, !page.closed else { return }
        func reject(_ reason: RejectionReason) {
#if DEBUG
            Self.startupLogger.notice("bridge.rejected")
#endif
            page.diagnostics.record("bridgeRejected", update: update, bridgeRejectionReason: reason)
        }
        if name == "cb" || name == "meguri" {
            guard page.platform != .mac else { reject(.legacyPlatform); return }
            guard world == WKContentWorld.page.name else { reject(.world); return }
            guard let text = body as? String else { reject(.body); return }
            guard text.utf8.count <= 512 else { reject(.bodySize); return }
            if name == "meguri", let action = Self.command(text, payload: payload) { page.admit(action); return }
            if name == "cb", let data = text.data(using: .utf8),
               let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: String],
               Set(json.keys) == ["scheme", "msg"], json["scheme"] == "ankitap", let zone = json["msg"], Self.zones.contains(zone) {
                if update.side == .question { page.admit(.reveal) }
                else if zone.hasSuffix("Left") { page.admit(.again) }
                else if zone.hasSuffix("Right") { page.admit(.good) }
                return
            }
            reject(.legacyValue); return
        }
        guard world == CardRuntime.interactionWorldName else { reject(.world); return }
        guard let dictionary = body as? [String: Any] else { reject(.body); return }
        guard dictionary.count <= 16 else { reject(.bodySize); return }
        guard let card = Self.integer(dictionary["card"]) else { reject(.card); return }
        // Validate the whole supported message before classifying its age. A
        // delayed producer retains its identity, even after this page is reused.
        func isCurrent(_ generation: Int64, side: String) -> Bool {
            if UInt64(generation) < update.generation { return false }
            guard UInt64(generation) == update.generation else { reject(.futureGeneration); return false }
            guard card == update.card else { reject(.card); return false }
            guard side == update.sideName else { reject(.side); return false }
            return true
        }
        if name == CardRuntime.confirmationHandlerName {
            guard let token = Self.integer(dictionary["token"]), token >= 0 else { reject(.confirmationToken); return }
            guard let phase = dictionary["phase"] as? String,
                  Set(dictionary.keys) == ["card", "generation", "side", "token", "phase"] else { reject(.confirmationShape); return }
            guard phase == "settled" || phase == "visible" else { reject(.confirmationValue); return }
            guard let generation = Self.integer(dictionary["generation"]), generation >= 0 else { reject(.generation); return }
            guard let side = dictionary["side"] as? String, side == "question" || side == "answer" else { reject(.side); return }
            guard isCurrent(generation, side: side) else { return }
            guard (phase == "settled" && UInt64(token) == update.generation || phase == "visible" && UInt64(token) == page.visibleToken && page.acceptsVisibleConfirmation())
            else { reject(.confirmationValue); return }
            page.confirm(dictionary)
            return
        }
        guard name == CardRuntime.signalHandlerName else { reject(.signalChannel); return }
        guard let generation = Self.integer(dictionary["generation"]), generation >= 0 else { reject(.generation); return }
        guard let side = dictionary["side"] as? String, side == "question" || side == "answer" else { reject(.side); return }
        guard let signal = dictionary["signal"] as? String else { reject(.signalType); return }
        let base: Set<String> = ["signal", "card", "generation", "side"]
        func keys(_ extra: Set<String>) -> Bool { Set(dictionary.keys).isSubset(of: base.union(extra)) }
        func string(_ key: String, max: Int) -> String? {
            guard let value = dictionary[key] as? String, value.utf8.count <= max else { return nil }
            return value
        }
        func boolean(_ key: String) -> Bool { Self.bool(dictionary[key]) != nil }
        switch signal {
        case "settled", "shown":
            guard keys([]) else { reject(.signalFields); return }
        case "visible":
            guard keys(["token"]) else { reject(.signalFields); return }
            guard let token = Self.integer(dictionary["token"]), token >= 0 else { reject(.signalValue); return }
        case "updateFailed":
            guard keys(["error"]) else { reject(.signalFields); return }
            guard string("error", max: 8_000) != nil else { reject(.signalValue); return }
        case "command":
            guard keys(["command", "reveal"]) else { reject(.signalFields); return }
            if dictionary["reveal"] != nil {
                guard Self.bool(dictionary["reveal"]) == true, dictionary["command"] == nil else { reject(.signalValue); return }
            } else {
                guard let command = string("command", max: 128), command == "ans" || Self.playCommand(command) != nil
                else { reject(.signalValue); return }
            }
        case "error":
            guard keys(["kind", "message", "line", "column", "source", "phase", "index"]) else { reject(.signalFields); return }
            guard let kind = string("kind", max: 16), ["exception", "rejection", "hook"].contains(kind),
                  string("message", max: 8_000) != nil, let phase = string("phase", max: 16),
                  ["script", "later", "shown-hook", "update-hook"].contains(phase),
                  dictionary["source"] == nil || string("source", max: 4096) != nil,
                  ["line", "column"].allSatisfy({ dictionary[$0] == nil || dictionary[$0] is NSNull || (Self.integer(dictionary[$0]) ?? -1) >= 0 }),
                  (phase == "script" ? (Self.integer(dictionary["index"]) ?? 0) > 0 : dictionary["index"] == nil || dictionary["index"] is NSNull)
            else { reject(.signalValue); return }
        case "mediaError":
            guard keys(["tag", "url", "family"]) else { reject(.signalFields); return }
            guard let tag = string("tag", max: 16),
                  ["img", "audio", "video", "source", "link", "object", "embed", "script", "font"].contains(tag),
                  string("url", max: 4096) != nil, dictionary["family"] == nil || string("family", max: 512) != nil
            else { reject(.signalValue); return }
        case "selection", "composition", "focus", "typedText", "listeners", "mediaPlayback":
            let fields: Set<String>
            let valid: Bool
            switch signal {
            case "selection": fields = ["selected"]
            case "composition": fields = ["active"]
            case "focus": fields = ["editable", "typeans"]
            case "typedText": fields = ["text"]
            case "mediaPlayback": fields = ["playing"]
            default: fields = ["tap", "key"]
            }
            guard keys(fields) else { reject(.signalFields); return }
            switch signal {
            case "selection": valid = boolean("selected")
            case "composition": valid = boolean("active")
            case "focus": valid = boolean("editable") && boolean("typeans")
            case "typedText": valid = string("text", max: 65_536) != nil
            case "mediaPlayback": valid = boolean("playing")
            default: valid = boolean("tap") && boolean("key")
            }
            guard valid else { reject(.signalValue); return }
        default: reject(.unknownSignal); return
        }
        guard isCurrent(generation, side: side) else { return }
        switch signal {
        case "settled", "shown", "updateFailed":
#if DEBUG
            switch signal {
            case "settled": Self.startupLogger.notice("bridge.settled")
            case "shown": Self.startupLogger.notice("bridge.shown")
            case "updateFailed": Self.startupLogger.notice("bridge.updateFailed")
            default: break
            }
#endif
            page.signal(signal, body: dictionary)
        case "visible":
            guard let token = Self.integer(dictionary["token"]), UInt64(token) == page.visibleToken else { reject(.signalValue); return }
            page.signal(signal, body: dictionary)
        case "command":
            if Self.bool(dictionary["reveal"]) == true, update.side == .question { page.admit(.reveal) }
            else if page.platform == .mac, let command = dictionary["command"] as? String {
                if command == "ans", update.side == .question { page.admit(.reveal) }
                else if let action = Self.command(command, payload: payload) { page.admit(action) }
                else { reject(.signalValue) }
            } else { reject(.signalValue) }
        case "error":
            let kind = dictionary["kind"] as? String ?? ""
            let phase = dictionary["phase"] as? String ?? ""
            page.diagnostics.captureTemplateError(update: update, payload: payload, kind: kind, phase: phase,
                                                  message: dictionary["message"] as? String ?? "",
                                                  line: Self.integer(dictionary["line"]).map(Int.init),
                                                  column: Self.integer(dictionary["column"]).map(Int.init),
                                                  index: Self.integer(dictionary["index"]).map(Int.init))
            let location = page.diagnostics.scriptLocation(message: dictionary["message"] as? String ?? "", line: Self.integer(dictionary["line"]).map(Int.init),
                                                           column: Self.integer(dictionary["column"]).map(Int.init), source: dictionary["source"] as? String)
            page.diagnostics.record("script.\(kind).\(phase)", update: update, index: Self.integer(dictionary["index"]).map(Int.init), script: location)
        case "mediaError":
            page.diagnostics.record("mediaError.\(dictionary["tag"] as? String ?? "")", update: update, name: dictionary["url"] as? String ?? "")
        default:
            if page.presented { page.signal(signal, body: dictionary) }
        }
    }

    static let zones: Set<String> = ["topLeft", "topCenter", "topRight", "midLeft", "midCenter", "midRight", "bottomLeft", "bottomCenter", "bottomRight"]
    static func bool(_ value: Any?) -> Bool? {
        guard let value = value as? NSNumber, CFGetTypeID(value) == CFBooleanGetTypeID() else { return nil }
        return value.boolValue
    }
    static func integer(_ value: Any?) -> Int64? {
        guard let value = value as? NSNumber, CFGetTypeID(value) != CFBooleanGetTypeID(),
              value.doubleValue.isFinite, abs(value.doubleValue) <= 9_007_199_254_740_991 else { return nil }
        return Int64(exactly: value.doubleValue)
    }
    static func command(_ text: String, payload: CardPayload) -> CardBridgeAction? {
        guard let action = playCommand(text), case let .play(side, index) = action,
              index < payload.audio(on: side).count else { return nil }
        return action
    }
    private static func playCommand(_ text: String) -> CardBridgeAction? {
        let parts = text.split(separator: ":", omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "play", parts[1] == "q" || parts[1] == "a", !parts[2].isEmpty,
              parts[2].utf8.allSatisfy({ $0 >= 48 && $0 <= 57 }) else { return nil }
        var index = 0
        for byte in parts[2].utf8 {
            let (times, overflow1) = index.multipliedReportingOverflow(by: 10)
            let (next, overflow2) = times.addingReportingOverflow(Int(byte - 48))
            guard !overflow1 && !overflow2 else { return nil }
            index = next
        }
        let side: CardRenderSide = parts[1] == "q" ? .question : .answer
        return .play(side, index)
    }
}
