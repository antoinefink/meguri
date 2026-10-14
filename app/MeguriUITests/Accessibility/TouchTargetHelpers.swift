import Foundation
import XCTest

/// One interactive element smaller than the platform's minimum.
struct TouchTargetViolation: Equatable, CustomStringConvertible {
    let identifier: String
    let label: String
    let size: CGSize
    let minimum: CGSize

    var description: String {
        let name = identifier.isEmpty ? "an element without identifier (\(label))" : identifier
        return "\(name) is \(Self.points(size)); the minimum here is \(Self.points(minimum)) (R1980)"
    }

    private static func points(_ size: CGSize) -> String {
        "\(Int(size.width.rounded()))×\(Int(size.height.rounded())) pt"
    }
}

/// The touch-target check (R1980) over the app's hierarchy: every
/// interactive element at least 44×44 pt on iPhone and iPad and 28×28 pt on
/// the Mac. Deterministic, so the critic never has to judge it (R1986).
enum TouchTargets {
    static var minimum: CGSize {
        #if os(macOS)
        CGSize(width: 28, height: 28)
        #else
        CGSize(width: 44, height: 44)
        #endif
    }

    /// Element types a person acts on.
    static let interactiveTypes: Set<XCUIElement.ElementType> = [
        .button, .link, .switch, .toggle, .slider, .stepper, .textField, .secureTextField, .searchField,
        .segmentedControl, .picker, .pickerWheel, .menuButton, .popUpButton, .checkBox, .radioButton,
        .colorWell, .incrementArrow, .decrementArrow, .disclosureTriangle, .comboBox, .textView,
    ]

    /// An element as the check sees it.
    struct Element {
        let identifier: String
        let label: String
        let type: XCUIElement.ElementType
        let frame: CGRect
    }

    /// Interactive elements of the hierarchy, in hierarchy order, that lie
    /// in `visible` (elements scrolled away are measured when on screen).
    static func elements(of snapshot: any XCUIElementSnapshot, visible: CGRect) -> [Element] {
        var result: [Element] = []
        func walk(_ node: any XCUIElementSnapshot) {
            if interactiveTypes.contains(node.elementType), node.frame.intersects(visible), !node.frame.isEmpty {
                result.append(Element(identifier: node.identifier, label: node.label, type: node.elementType, frame: node.frame))
            }
            node.children.forEach(walk)
        }
        walk(snapshot)
        return result
    }

    /// The elements below `minimum` in either dimension.
    static func violations(_ elements: [Element], minimum: CGSize = minimum) -> [TouchTargetViolation] {
        elements.compactMap { element in
            // Half a point of layout rounding is not a violation.
            guard element.frame.width + 0.5 < minimum.width || element.frame.height + 0.5 < minimum.height else { return nil }
            return TouchTargetViolation(identifier: element.identifier, label: element.label, size: element.frame.size, minimum: minimum)
        }
    }

    /// The violations on the app's current screen.
    @MainActor
    static func violations(in app: XCUIApplication) throws -> [TouchTargetViolation] {
        let snapshot = try app.snapshot()
        let window = app.windows.firstMatch.frame
        return violations(elements(of: snapshot, visible: window.isEmpty ? snapshot.frame : window))
    }

    /// Fails once per violation, naming the element and its size.
    static func assertMet(_ violations: [TouchTargetViolation], file: StaticString = #filePath, line: UInt = #line) {
        for violation in violations {
            XCTFail("touch target: \(violation)", file: file, line: line)
        }
    }
}
