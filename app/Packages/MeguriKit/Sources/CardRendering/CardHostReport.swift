import CoreFoundation
import Foundation

/// Observations only. Review's owner retains the input-start/phase gate.
public nonisolated struct CardHostContext: Sendable, Equatable {
    public enum Visibility: String, Sendable { case preparing, presented }
    public let update: CardUpdate
    public let viewRevision: UInt64
    public let visibility: Visibility
    public let contentRect: CGRect
    public let pageZoom: Double
}

public nonisolated struct CardHostGeometry: Sendable, Equatable {
    public let width: Double
    public let height: Double
    public let offsetX: Double
    public let offsetY: Double
    public let scale: Double
    public let layoutRevision: UInt64

    /// Input has already been converted from its host/window to page points.
    /// Offsets describe the layout viewport; rects/points are already local
    /// to the visual viewport, so scroll offsets must not be added again.
    public func point(_ point: CGPoint, in contentRect: CGRect) -> CGPoint? {
        guard contentRect.width.isFinite, contentRect.height.isFinite, contentRect.minX.isFinite, contentRect.minY.isFinite,
              contentRect.width > 0, contentRect.height > 0,
              point.x.isFinite, point.y.isFinite,
              point.x >= contentRect.minX, point.x <= contentRect.maxX,
              point.y >= contentRect.minY, point.y <= contentRect.maxY else { return nil }
        return CGPoint(x: (point.x - contentRect.minX) * width / contentRect.width,
                       y: (point.y - contentRect.minY) * height / contentRect.height)
    }
}

public nonisolated struct CardHostInput: Sendable, Equatable {
    public struct State: Sendable, Equatable {
        public let timeMs: Double
        public let defaultPrevented: Bool
        public let exempt: Bool
        public let selection: Bool
        public let editable: Bool
        public let typeans: Bool
        public let composing: Bool
    }
    public enum Pointer: String, Sendable { case touch, mouse, pen, unknown }
    public enum KeyEvent: String, Sendable { case keydown, keyup }
    public enum Detail: Sendable, Equatable {
        case click(geometry: CardHostGeometry, point: CGPoint, button: Int, pointer: Pointer)
        case key(event: KeyEvent, key: String, code: String, repeatKey: Bool, shift: Bool, control: Bool, alt: Bool, meta: Bool)
    }
    public let state: State
    public let detail: Detail
}

public nonisolated struct CardHostHitMap: Sendable, Equatable {
    public let geometry: CardHostGeometry
    public let complete: Bool
    public let rects: [CGRect]

    /// Incomplete maps require the web path.
    public func isExempt(_ pagePoint: CGPoint, context: CardHostContext) -> Bool? {
        guard complete, let point = geometry.point(pagePoint, in: context.contentRect) else { return nil }
        return rects.contains { point.x >= $0.minX && point.x <= $0.maxX && point.y >= $0.minY && point.y <= $0.maxY }
    }
}

public nonisolated enum CardHostProbeKind: String, Sendable { case background, resources }
public nonisolated enum CardHostResourcePurpose: String, Sendable { case warmup, preload }
public nonisolated enum CardHostResourceState: String, Sendable { case ready, budgetExpired, failed }
public nonisolated enum CardHostProbeValue: Sendable, Equatable {
    case background(red: Double, green: Double, blue: Double, alpha: Double)
    case resources(purpose: CardHostResourcePurpose, state: CardHostResourceState, imagesPending: Int, fontsPending: Int, stylesPending: Int)
}
public nonisolated enum CardHostProbeOutcome: Sendable, Equatable {
    case observed(CardHostProbeValue), unavailable, timeout, cancelled
}
public nonisolated enum CardHostEvent: Sendable, Equatable {
    case unavailable
    case invalidated(viewRevision: UInt64)
    case hitMapUnavailable
    case input(CardHostContext, CardHostInput)
    case hitMap(CardHostContext, CardHostHitMap)
}

/// Fixed schema; no untyped value escapes this decoder. Validate the bounded
/// tree before serializing, so excess depth/width cannot reach JSON recursion.
struct CardHostReport {
    enum Value {
        case input(UInt64, CardHostInput)
        case hitMap(UInt64, CardHostHitMap)
        case probe(CardHostProbeKind, UInt64, CardHostProbeValue)
    }
    let update: CardUpdate
    let revision: UInt64
    let value: Value

    init?(_ body: Any) {
        guard Self.bounded(body), let d = body as? [String: Any],
              let signal = d["signal"] as? String,
              Self.exact(d, Set(["signal", "version", "card", "generation", "side", "viewRevision", "payload"]).union(signal == "hostProbe" ? [] : ["serial"])),
              CardBridge.integer(d["version"]) == 1,
              let card = CardBridge.integer(d["card"]), let generation = Self.unsigned(d["generation"], positive: false),
              let side = d["side"] as? String, side == "question" || side == "answer",
              let revision = Self.unsigned(d["viewRevision"]), let p = d["payload"] as? [String: Any] else { return nil }
        update = CardUpdate(card: card, generation: generation, side: side == "question" ? .question : .answer)
        self.revision = revision
        switch signal {
        case "hostInput":
            guard let serial = Self.unsigned(d["serial"]), let input = Self.input(p) else { return nil }
            value = .input(serial, input)
        case "hostHitMap":
            guard let serial = Self.unsigned(d["serial"]), Self.exact(p, ["geometry", "complete", "rects"]),
                  let geometry = Self.geometry(p["geometry"]), let complete = CardBridge.bool(p["complete"]),
                  let items = p["rects"] as? [Any], items.count <= 256 else { return nil }
            var rects: [CGRect] = []
            for item in items {
                guard let r = item as? [String: Any], Self.exact(r, ["x", "y", "width", "height"]),
                      let x = Self.number(r["x"], 0...geometry.width), let y = Self.number(r["y"], 0...geometry.height),
                      let w = Self.number(r["width"], 0...geometry.width), w > 0,
                      let h = Self.number(r["height"], 0...geometry.height), h > 0,
                      x + w <= geometry.width, y + h <= geometry.height else { return nil }
                rects.append(CGRect(x: x, y: y, width: w, height: h))
            }
            value = .hitMap(serial, CardHostHitMap(geometry: geometry, complete: complete, rects: rects))
        case "hostProbe":
            guard let kind = (p["kind"] as? String).flatMap(CardHostProbeKind.init(rawValue:)), let request = Self.unsigned(p["request"]) else { return nil }
            switch kind {
            case .background:
                guard Self.exact(p, ["kind", "request", "rgba"]), let rgba = p["rgba"] as? [Any], rgba.count == 4 else { return nil }
                let numbers = rgba.compactMap { Self.number($0, 0...1) }
                guard numbers.count == 4 else { return nil }
                value = .probe(kind, request, .background(red: numbers[0], green: numbers[1], blue: numbers[2], alpha: numbers[3]))
            case .resources:
                guard Self.exact(p, ["kind", "request", "purpose", "state", "imagesPending", "fontsPending", "stylesPending"]),
                      let purpose = (p["purpose"] as? String).flatMap(CardHostResourcePurpose.init(rawValue:)),
                      let state = (p["state"] as? String).flatMap(CardHostResourceState.init(rawValue:)),
                      let images = Self.count(p["imagesPending"]), let fonts = Self.count(p["fontsPending"]), let styles = Self.count(p["stylesPending"]) else { return nil }
                value = .probe(kind, request, .resources(purpose: purpose, state: state, imagesPending: images, fontsPending: fonts, stylesPending: styles))
            }
        default: return nil
        }
    }

    static func exact(_ d: [String: Any], _ keys: Set<String>) -> Bool { Set(d.keys) == keys }
    static func unsigned(_ value: Any?, positive: Bool = true) -> UInt64? {
        guard let n = CardBridge.integer(value), n >= (positive ? 1 : 0) else { return nil }
        return UInt64(n)
    }
    static func number(_ value: Any?, _ range: ClosedRange<Double>) -> Double? {
        guard let n = value as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID(), n.doubleValue.isFinite, range.contains(n.doubleValue) else { return nil }
        return n.doubleValue
    }
    private static func count(_ value: Any?) -> Int? {
        guard let n = CardBridge.integer(value), (0...1_000_000).contains(n) else { return nil }
        return Int(n)
    }
    private static func geometry(_ value: Any?) -> CardHostGeometry? {
        guard let d = value as? [String: Any], exact(d, ["width", "height", "offsetX", "offsetY", "scale", "layoutRevision"]),
              let w = number(d["width"], 0...1_000_000), w > 0, let h = number(d["height"], 0...1_000_000), h > 0,
              let x = number(d["offsetX"], -1_000_000_000...1_000_000_000), let y = number(d["offsetY"], -1_000_000_000...1_000_000_000),
              let scale = number(d["scale"], 0...1_000), scale > 0, let revision = unsigned(d["layoutRevision"]) else { return nil }
        return CardHostGeometry(width: w, height: h, offsetX: x, offsetY: y, scale: scale, layoutRevision: revision)
    }
    private static func input(_ p: [String: Any]) -> CardHostInput? {
        let common: Set<String> = ["kind", "timeMs", "defaultPrevented", "exempt", "selection", "editable", "typeans", "composing"]
        guard let kind = p["kind"] as? String,
              let time = number(p["timeMs"], 0...9_007_199_254_740_991),
              let prevented = CardBridge.bool(p["defaultPrevented"]), let exempt = CardBridge.bool(p["exempt"]),
              let selection = CardBridge.bool(p["selection"]), let editable = CardBridge.bool(p["editable"]),
              let typeans = CardBridge.bool(p["typeans"]), let composing = CardBridge.bool(p["composing"]), !typeans || editable else { return nil }
        let state = CardHostInput.State(timeMs: time, defaultPrevented: prevented, exempt: exempt, selection: selection, editable: editable, typeans: typeans, composing: composing)
        if kind == "click" {
            guard exact(p, common.union(["geometry", "x", "y", "button", "pointer"])), let geometry = geometry(p["geometry"]),
                  let x = number(p["x"], 0...geometry.width), let y = number(p["y"], 0...geometry.height),
                  let button = CardBridge.integer(p["button"]), (0...4).contains(button),
                  let pointer = (p["pointer"] as? String).flatMap(CardHostInput.Pointer.init(rawValue:)) else { return nil }
            return CardHostInput(state: state, detail: .click(geometry: geometry, point: CGPoint(x: x, y: y), button: Int(button), pointer: pointer))
        }
        guard kind == "key", exact(p, common.union(["event", "key", "code", "repeat", "shift", "control", "alt", "meta"])),
              let event = (p["event"] as? String).flatMap(CardHostInput.KeyEvent.init(rawValue:)),
              let key = p["key"] as? String, key.utf8.count <= 128, let code = p["code"] as? String, code.utf8.count <= 128,
              let repeated = CardBridge.bool(p["repeat"]), let shift = CardBridge.bool(p["shift"]), let control = CardBridge.bool(p["control"]),
              let alt = CardBridge.bool(p["alt"]), let meta = CardBridge.bool(p["meta"]) else { return nil }
        return CardHostInput(state: state, detail: .key(event: event, key: key, code: code, repeatKey: repeated, shift: shift, control: control, alt: alt, meta: meta))
    }

    private static func bounded(_ body: Any) -> Bool {
        var budget = 65_536
        func walk(_ value: Any, depth: Int) -> Bool {
            guard depth <= 4, budget >= 0 else { return false }
            if let s = value as? String { budget -= s.utf8.count + 2 }
            else if let d = value as? [String: Any] {
                guard d.count <= 20 else { return false }
                budget -= 2 + d.count
                for (key, value) in d {
                    budget -= key.utf8.count + 3
                    guard walk(value, depth: depth + 1) else { return false }
                }
            } else if let a = value as? [Any] {
                guard a.count <= 256 else { return false }
                budget -= 2 + a.count
                for value in a { guard walk(value, depth: depth + 1) else { return false } }
            } else if value is NSNumber { budget -= 1 }
            else { return false }
            return budget >= 0
        }
        guard walk(body, depth: 0), JSONSerialization.isValidJSONObject(body),
              let data = try? JSONSerialization.data(withJSONObject: body, options: [.sortedKeys, .withoutEscapingSlashes]) else { return false }
        return data.count <= 65_536
    }
}
