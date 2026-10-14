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

    /// Reads the current screen from its first element forward, at most
    /// `limit` steps, stopping when VoiceOver comes back to an element it
    /// already read. On the Mac VoiceOver is never turned on: it would speak
    /// on the user's machine and take over its screen; the hierarchy is used.
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
        var utterances: [String] = []
        do {
            if let first = try? service.currentSpeech(), !first.utterance.isEmpty {
                utterances.append(first.utterance)
            }
            for _ in 0..<limit {
                let output = try service.moveForward()
                guard !output.utterance.isEmpty, !utterances.contains(output.utterance) else { break }
                utterances.append(output.utterance)
            }
        } catch {
            if utterances.isEmpty {
                return Reading(source: .hierarchy(reason: "the VoiceOver service spoke nothing: \(error)"), utterances: try hierarchyOrder(app))
            }
        }
        return Reading(source: .voiceOverService, utterances: utterances)
        #endif
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
