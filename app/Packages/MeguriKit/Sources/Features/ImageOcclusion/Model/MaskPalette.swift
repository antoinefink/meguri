import Foundation
import Observation

nonisolated public enum MaskPaletteChange: Sendable {
    case add(String)
    case remove(String)
}

/// K9's value, including unreadable strings that must survive a later write.
/// The host applies a change to a freshly read value inside its config operation.
nonisolated public struct MaskPalette: Sendable, Equatable {
    private enum Slot: Sendable, Equatable {
        case empty
        case color(String)
        case kept(String)
    }
    private static let emptyFill = "#ffffff"
    private var slots: [Slot]

    public init(json: String? = nil) {
        let entries = json.flatMap { $0.data(using: .utf8) }
            .flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [Any] ?? []
        slots = (0..<16).map { index in
            guard index < entries.count, let text = entries[index] as? String else { return .empty }
            guard let color = Self.color(text) else { return .kept(text) }
            return color == Self.emptyFill ? .empty : .color(color)
        }
    }

    public var colors: [String] {
        slots.compactMap { if case let .color(color) = $0 { color } else { nil } }
    }

    /// Returns nil for a no-op, so the host does not issue a config write.
    public func applying(_ change: MaskPaletteChange) -> String? {
        var next = self
        switch change {
        case let .add(value):
            guard let color = Self.color(value), color != Self.emptyFill, !colors.contains(color) else { return nil }
            if let index = slots.firstIndex(of: .empty) { next.slots[index] = .color(color) }
            else { next.slots.removeFirst(); next.slots.append(.color(color)) }
        case let .remove(value):
            guard let color = Self.color(value), colors.contains(color) else { return nil }
            for index in slots.indices {
                if case let .color(current) = slots[index], current == color { next.slots[index] = .empty }
            }
        }
        let values = next.slots.map { slot in
            switch slot {
            case .empty: Self.emptyFill
            case let .color(color): color
            case let .kept(stored): stored
            }
        }
        guard let data = try? JSONSerialization.data(withJSONObject: values),
              let json = String(data: data, encoding: .utf8) else { return nil }
        return json
    }

    private static func color(_ value: String) -> String? {
        guard value.first == "#" else { return nil }
        let digits = value.dropFirst().lowercased()
        guard [3, 6].contains(digits.count), digits.allSatisfy({ $0.isASCII && $0.isHexDigit }) else { return nil }
        return "#" + (digits.count == 3 ? digits.map { "\($0)\($0)" }.joined() : digits)
    }
}

/// Collection-backed adapter for customColorPickerPalette, shared with the
/// field editor. change must reread and mutate K9 atomically, skip undo, and
/// return the value actually stored. Picking a fill never invokes change.
public protocol MaskPaletteStore: Sendable {
    func read() async throws -> MaskPalette
    func change(_ change: MaskPaletteChange) async throws -> MaskPalette
}

@MainActor @Observable public final class MaskPaletteModel {
    public private(set) var value = MaskPalette()
    public private(set) var busy = false
    public private(set) var error: String?
    private let store: any MaskPaletteStore

    public init(store: any MaskPaletteStore) { self.store = store }

    func reload() async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do { value = try await store.read(); error = nil }
        catch { self.error = error.localizedDescription }
    }

    func change(_ change: MaskPaletteChange) async {
        guard !busy else { return }
        busy = true
        defer { busy = false }
        do { value = try await store.change(change); error = nil }
        catch { self.error = error.localizedDescription }
    }
}
