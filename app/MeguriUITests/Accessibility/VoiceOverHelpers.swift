import Foundation
import XCTest

/// What VoiceOver reads on a screen, in order (R1978), from the Xcode 27
/// VoiceOver service, or from the accessibility hierarchy when the service
/// cannot run. Screen scripts (U253: review, deck list, browser, editor) use
/// it; AE88's order is asserted with `assertOrder`.
enum VoiceOver {
    enum Source: Equatable, CustomStringConvertible {
        /// `XCUIDevice.shared.voiceOverService`: VoiceOver's own utterances.
        case voiceOverService
        /// Accessibility elements in hierarchy order, read as
        /// "label, value, trait" (the reason the service was not used).
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
    }

    /// Reads the current screen from its first element forward, until
    /// VoiceOver stops moving or after `limit` steps (`traverse`). On the Mac
    /// VoiceOver is never turned on: it would speak on the user's machine and
    /// take over its screen; the hierarchy is used.
    @MainActor
    static func read(_ app: XCUIApplication, limit: Int = 60) throws -> Reading {
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
        let utterances = traverse(first: try? service.currentSpeech().utterance, limit: limit) {
            do {
                return try service.moveForward().utterance
            } catch {
                failure = error
                return nil
            }
        }
        if utterances.isEmpty {
            return Reading(source: .hierarchy(reason: "the VoiceOver service spoke nothing: \(failure.map { "\($0)" } ?? "no speech")"), utterances: try hierarchyOrder(app))
        }
        return Reading(source: .voiceOverService, utterances: utterances)
        #endif
    }

    /// A forward reading: `first`, then what each `next()` speaks, until it
    /// speaks nothing (VoiceOver did not move: the last element) or after
    /// `limit` steps. Repeated utterances are kept: two elements may say the
    /// same thing (two Delete buttons). Only a reading that ran to the limit
    /// is checked for VoiceOver having wrapped around to the start: when the
    /// whole reading repeats one sequence from its first utterance, that
    /// sequence is the screen.
    @MainActor
    static func traverse(first: String?, limit: Int, next: () -> String?) -> [String] {
        var utterances: [String] = []
        if let first, !first.isEmpty { utterances.append(first) }
        for _ in 0..<limit {
            guard let spoken = next(), !spoken.isEmpty else { return utterances }
            utterances.append(spoken)
        }
        for period in 1..<max(utterances.count, 1) where utterances.indices.dropFirst(period).allSatisfy({ utterances[$0] == utterances[$0 - period] }) {
            return Array(utterances.prefix(period))
        }
        return utterances
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
