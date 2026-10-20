import Foundation
import XCTest

/// Screen reading order (R1978), from the accessibility hierarchy by default.
/// Optional post-goal speech diagnostics explicitly opt into the Xcode 27
/// VoiceOver service; `Reading.source` identifies hierarchy or actual speech.
/// Screen scripts (U253: review, deck list, browser, editor) use it;
/// AE88's order is asserted with `assertOrder`.
enum VoiceOver {
    enum Source: Equatable, CustomStringConvertible {
        /// `XCUIDevice.shared.voiceOverService`: VoiceOver's own utterances.
        case voiceOverService
        /// Accessibility elements in hierarchy order, read as
        /// "label, value, trait", with the reason the service was not used.
        case hierarchy(reason: String)

        var description: String {
            switch self {
            case .voiceOverService: "VoiceOver service"
            case let .hierarchy(reason): "accessibility hierarchy (\(reason))"
            }
        }
    }

    struct Reading {
        let source: Source
        let utterances: [String]
        /// False when the reading stopped at the step limit: the screen may
        /// go on (`traverse`).
        var complete = true
    }

    /// A forward reading and whether it reached the end.
    struct Traversal: Equatable {
        let utterances: [String]
        let complete: Bool
    }

    /// Reads the current accessibility hierarchy by default. Explicit speech
    /// opt-in reads until VoiceOver stops moving or after `limit` steps
    /// (`traverse`), falling back to the hierarchy if speech is unavailable.
    /// The source always identifies which was used. On the Mac VoiceOver is
    /// never turned on: it would speak on the user's machine and take over
    /// its screen; the hierarchy is used even with speech opt-in.
    @MainActor
    static func read(_ app: XCUIApplication, limit: Int = 60, useSpeechService: Bool = false) throws -> Reading {
        guard useSpeechService else {
            return Reading(source: .hierarchy(reason: "speech diagnostics are optional post-goal work and require explicit opt-in"), utterances: try hierarchyOrder(app))
        }
        #if os(macOS)
        return Reading(source: .hierarchy(reason: "VoiceOver is never turned on on the Mac running the tests"), utterances: try hierarchyOrder(app))
        #else
        let service = XCUIDevice.shared.voiceOverService
        do {
            try service.enable()
        } catch {
            return Reading(source: .hierarchy(reason: "the VoiceOver service did not start: \(error)"), utterances: try hierarchyOrder(app))
        }
        defer { try? service.disable() }
        var failure: (any Error)?
        let reading = traverse(first: try? service.currentSpeech().utterance, limit: limit) {
            do {
                return try service.moveForward().utterance
            } catch {
                failure = error
                return nil
            }
        }
        if reading.utterances.isEmpty {
            return Reading(source: .hierarchy(reason: "the VoiceOver service spoke nothing: \(failure.map { "\($0)" } ?? "no speech")"), utterances: try hierarchyOrder(app))
        }
        return Reading(source: .voiceOverService, utterances: reading.utterances, complete: reading.complete)
        #endif
    }

    /// A forward reading: `first`, then what each `next()` speaks, until it
    /// speaks nothing (VoiceOver did not move: the last element; complete)
    /// or after `limit` steps (not complete). Every utterance is kept,
    /// repeated ones too: many elements may say the same thing (a row of
    /// Delete buttons), and the service tells neither which element spoke
    /// nor that it wrapped around (its output is the utterance alone), so a
    /// repeat proves no wrap-around and a reading is never cut on one.
    @MainActor
    static func traverse(first: String?, limit: Int, next: () -> String?) -> Traversal {
        var utterances: [String] = []
        if let first, !first.isEmpty { utterances.append(first) }
        for _ in 0..<limit {
            guard let spoken = next(), !spoken.isEmpty else { return Traversal(utterances: utterances, complete: true) }
            utterances.append(spoken)
        }
        return Traversal(utterances: utterances, complete: false)
    }

    /// Accessibility elements in hierarchy order: leaves with a label, and
    /// controls (read whole, their children not again).
    @MainActor
    static func hierarchyOrder(_ app: XCUIApplication) throws -> [String] {
        var result: [String] = []
        func walk(_ node: any XCUIElementSnapshot) {
            let isControl = TouchTargets.interactiveTypes.contains(node.elementType)
            let readable = isControl || (node.children.isEmpty && [.staticText, .image, .textView, .other].contains(node.elementType))
            if readable, !node.label.isEmpty {
                var parts = [node.label]
                if let value = node.value as? String, !value.isEmpty, value != node.label { parts.append(value) }
                if let trait = traitName(node.elementType) { parts.append(trait) }
                result.append(parts.joined(separator: ", "))
            }
            if !isControl { node.children.forEach(walk) }
        }
        walk(try app.snapshot())
        return result
    }

    private static func traitName(_ type: XCUIElement.ElementType) -> String? {
        switch type {
        case .button, .menuButton, .popUpButton: "Button"
        case .link: "Link"
        case .switch, .toggle: "Switch button"
        case .slider: "Adjustable"
        case .image: "Image"
        case .textField, .searchField, .secureTextField: "Text field"
        default: nil
        }
    }

    /// Fails unless `expected` appears in `reading` in this order; each
    /// expected entry matches the first later utterance that contains it.
    static func assertOrder(_ reading: Reading, contains expected: [String], file: StaticString = #filePath, line: UInt = #line) {
        var index = reading.utterances.startIndex
        for item in expected {
            guard let found = reading.utterances[index...].firstIndex(where: { $0.localizedCaseInsensitiveContains(item) }) else {
                XCTFail("VoiceOver (\(reading.source)) does not read \"\(item)\" after the earlier items; it read: \(reading.utterances)", file: file, line: line)
                return
            }
            index = reading.utterances.index(after: found)
        }
    }
}
