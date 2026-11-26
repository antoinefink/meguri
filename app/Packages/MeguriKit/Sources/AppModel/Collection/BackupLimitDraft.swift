import Foundation

/// The field's edit baseline, separate from later synced values.
public struct BackupLimitDraft {
    public var text = ""
    public private(set) var invalid = false
    private var original: UInt32?

    public init() {}
    public mutating func reset(to value: UInt32) {
        original = value; text = String(value); invalid = false
    }
    public mutating func commit(value: UInt32, minimum: UInt32) -> UInt32? {
        // Focus/defocus is not an edit, including for legacy unbounded values.
        guard let original, text != String(original) else { reset(to: value); return nil }
        guard let number = UInt32(text.trimmingCharacters(in: .whitespaces)), number >= minimum, number <= 9999 else {
            invalid = true; return nil
        }
        if number == original || number == value { reset(to: value); return nil }
        reset(to: number)
        return number
    }
}
