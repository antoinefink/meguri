import CoreBridge
import CoreFoundation
import Foundation
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
            page.diagnostics.record("bridgeOrigin", update: page.update)
            return
        }
        receive(name: message.name, world: message.world.name, body: message.body, page: page)
    }

    /// Metadata admission is above. Keeping body validation separate permits
    /// tests with ordinary values instead of WKFrameInfo objects.
    func receive(name: String, world: String?, body: Any, page: CardPage) {
        guard let update = page.update, let payload = page.payload, !page.closed else { return }
        func reject() { page.diagnostics.record("bridgeRejected", update: update) }
        if name == "cb" || name == "meguri" {
            guard page.platform != .mac, world == WKContentWorld.page.name, let text = body as? String, text.utf8.count <= 512 else { reject(); return }
            if name == "meguri", let action = Self.command(text, payload: payload) { page.admit(action); return }
            if name == "cb", let data = text.data(using: .utf8),
               let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: String],
               Set(json.keys) == ["scheme", "msg"], json["scheme"] == "ankitap", let zone = json["msg"], Self.zones.contains(zone) {
                if update.side == .question { page.admit(.reveal) }
                else if zone.hasSuffix("Left") { page.admit(.again) }
                else if zone.hasSuffix("Right") { page.admit(.good) }
                return
            }
            reject(); return
        }
        guard world == CardRuntime.interactionWorldName, let dictionary = body as? [String: Any], dictionary.count <= 16,
              let card = Self.integer(dictionary["card"]), card == update.card else { reject(); return }
        if name == CardRuntime.confirmationHandlerName {
            guard let token = Self.integer(dictionary["token"]), token >= 0, let phase = dictionary["phase"] as? String,
                  Set(dictionary.keys) == ["card", "token", "phase"],
                  (phase == "settled" && UInt64(token) == update.generation || phase == "visible" && UInt64(token) == page.visibleToken && page.acceptsVisibleConfirmation())
            else { reject(); return }
            page.confirm(dictionary)
            return
        }
        guard name == CardRuntime.signalHandlerName, let generation = Self.integer(dictionary["generation"]), generation >= 0,
              UInt64(generation) == update.generation, dictionary["side"] as? String == update.sideName,
              let signal = dictionary["signal"] as? String else { reject(); return }
        let base: Set<String> = ["signal", "card", "generation", "side"]
        func keys(_ extra: Set<String>) -> Bool { Set(dictionary.keys).isSubset(of: base.union(extra)) }
        func string(_ key: String, max: Int) -> String? {
            guard let value = dictionary[key] as? String, value.utf8.count <= max else { return nil }
            return value
        }
        func boolean(_ key: String) -> Bool { Self.bool(dictionary[key]) != nil }
        switch signal {
        case "settled", "shown":
            guard keys([]) else { reject(); return }
            page.signal(signal, body: dictionary)
        case "visible":
            guard keys(["token"]), let token = Self.integer(dictionary["token"]), token >= 0, UInt64(token) == page.visibleToken else { reject(); return }
            page.signal(signal, body: dictionary)
        case "updateFailed":
            guard keys(["error"]), string("error", max: 8_000) != nil else { reject(); return }
            page.signal(signal, body: dictionary)
        case "command":
            guard keys(["command", "reveal"]) else { reject(); return }
            if Self.bool(dictionary["reveal"]) == true, update.side == .question, dictionary["command"] == nil { page.admit(.reveal) }
            else if page.platform == .mac, let command = string("command", max: 128), dictionary["reveal"] == nil {
                if command == "ans", update.side == .question { page.admit(.reveal) }
                else if let action = Self.command(command, payload: payload) { page.admit(action) }
                else { reject() }
            } else { reject() }
        case "error":
            guard keys(["kind", "message", "line", "column", "source", "phase", "index"]),
                  let kind = string("kind", max: 16), ["exception", "rejection", "hook"].contains(kind),
                  string("message", max: 8_000) != nil, let phase = string("phase", max: 16),
                  ["script", "later", "shown-hook", "update-hook"].contains(phase),
                  dictionary["source"] == nil || string("source", max: 4096) != nil,
                  ["line", "column"].allSatisfy({ dictionary[$0] == nil || dictionary[$0] is NSNull || (Self.integer(dictionary[$0]) ?? -1) >= 0 }),
                  (phase == "script" ? (Self.integer(dictionary["index"]) ?? 0) > 0 : dictionary["index"] == nil || dictionary["index"] is NSNull)
            else { reject(); return }
            page.diagnostics.captureTemplateError(update: update, payload: payload, kind: kind, phase: phase,
                                                  message: dictionary["message"] as? String ?? "",
                                                  line: Self.integer(dictionary["line"]).map(Int.init),
                                                  column: Self.integer(dictionary["column"]).map(Int.init),
                                                  index: Self.integer(dictionary["index"]).map(Int.init))
            let location = page.diagnostics.scriptLocation(message: dictionary["message"] as? String ?? "", line: Self.integer(dictionary["line"]).map(Int.init),
                                                           column: Self.integer(dictionary["column"]).map(Int.init), source: dictionary["source"] as? String)
            page.diagnostics.record("script.\(kind).\(phase)", update: update, index: Self.integer(dictionary["index"]).map(Int.init), script: location)
        case "mediaError":
            guard keys(["tag", "url", "family"]), let tag = string("tag", max: 16),
                  ["img", "audio", "video", "source", "link", "object", "embed", "script", "font"].contains(tag),
                  let url = string("url", max: 4096), dictionary["family"] == nil || string("family", max: 512) != nil
            else { reject(); return }
            page.diagnostics.record("mediaError.\(tag)", update: update, name: url)
        case "selection", "composition", "focus", "typedText", "listeners", "mediaPlayback":
            let valid: Bool
            switch signal {
            case "selection": valid = keys(["selected"]) && boolean("selected")
            case "composition": valid = keys(["active"]) && boolean("active")
            case "focus": valid = keys(["editable", "typeans"]) && boolean("editable") && boolean("typeans")
            case "typedText": valid = keys(["text"]) && string("text", max: 65_536) != nil
            case "mediaPlayback": valid = keys(["playing"]) && boolean("playing")
            default: valid = keys(["tap", "key"]) && boolean("tap") && boolean("key")
            }
            guard valid else { reject(); return }
            if page.presented { page.signal(signal, body: dictionary) }
        default: reject()
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
        return index < payload.audio(on: side).count ? .play(side, index) : nil
    }
}
