import Foundation

/// The colour sets of DesignSystem's `Colors.xcassets`, read from the source
/// tree: what each set declares for each appearance mode, and anything about
/// a set that the token rules do not allow.
struct ColorCatalog {
    static let sourceURL = URL(filePath: #filePath)
        .deletingLastPathComponent() // Support
        .deletingLastPathComponent() // DesignSystemTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // MeguriKit
        .appending(path: "Sources/DesignSystem/Resources/Colors.xcassets")

    /// Colour per mode, by set name.
    private(set) var sets: [String: [AppearanceMode: RGB]] = [:]
    /// Rule violations, one line each.
    private(set) var problems: [String] = []

    init(url: URL = Self.sourceURL) throws {
        let names = try FileManager.default.contentsOfDirectory(atPath: url.path(percentEncoded: false))
            .filter { $0.hasSuffix(".colorset") }
            .sorted()
        for directory in names {
            let name = String(directory.dropLast(".colorset".count))
            let data = try Data(contentsOf: url.appending(path: "\(directory)/Contents.json"))
            let contents = try JSONDecoder().decode(Contents.self, from: data)
            var modes: [AppearanceMode: RGB] = [:]
            for entry in contents.colors {
                guard let mode = Self.mode(of: entry.appearances ?? []) else {
                    problems.append("\(name): appearances \(entry.appearances ?? []) are not one of light, dark and high contrast of each")
                    continue
                }
                if entry.idiom != "universal" {
                    problems.append("\(name) (\(mode)): idiom \(entry.idiom), expected universal")
                }
                if entry.color.colorSpace != "srgb" {
                    problems.append("\(name) (\(mode)): colour space \(entry.color.colorSpace), expected srgb")
                }
                let components = entry.color.components
                guard let red = Self.component(components.red), let green = Self.component(components.green),
                      let blue = Self.component(components.blue), let alpha = Self.component(components.alpha)
                else {
                    problems.append("\(name) (\(mode)): unreadable components")
                    continue
                }
                if alpha != 1 {
                    problems.append("\(name) (\(mode)): alpha \(alpha); tokens are opaque so their contrast is defined")
                }
                if modes[mode] != nil {
                    problems.append("\(name): \(mode) is declared twice")
                }
                modes[mode] = RGB(red: red, green: green, blue: blue)
            }
            for mode in AppearanceMode.allCases where modes[mode] == nil {
                problems.append("\(name): no \(mode) variant")
            }
            sets[name] = modes
        }
    }

    private static func mode(of appearances: [Appearance]) -> AppearanceMode? {
        let pairs = Set(appearances.map { "\($0.appearance)=\($0.value)" })
        switch pairs {
        case []: return .light
        case ["luminosity=dark"]: return .dark
        case ["contrast=high"]: return .lightIncreasedContrast
        case ["luminosity=dark", "contrast=high"]: return .darkIncreasedContrast
        default: return nil
        }
    }

    /// `0xNN` (0-255), a fraction such as `0.500`, or an integer 0-255.
    private static func component(_ text: String) -> Double? {
        if text.hasPrefix("0x"), let value = Int(text.dropFirst(2), radix: 16) { return Double(value) / 255 }
        if text.contains("."), let value = Double(text) { return value }
        if let value = Int(text) { return Double(value) / 255 }
        return nil
    }

    private struct Contents: Decodable {
        let colors: [Entry]
    }

    private struct Entry: Decodable {
        let appearances: [Appearance]?
        let color: ColorValue
        let idiom: String
    }

    private struct Appearance: Decodable, CustomStringConvertible {
        let appearance: String
        let value: String
        var description: String { "\(appearance)=\(value)" }
    }

    private struct ColorValue: Decodable {
        let colorSpace: String
        let components: Components

        enum CodingKeys: String, CodingKey {
            case colorSpace = "color-space"
            case components
        }
    }

    private struct Components: Decodable {
        let red: String
        let green: String
        let blue: String
        let alpha: String
    }
}
