import Foundation
import SwiftUI

import DesignSystem

/// The four appearances every colour token is checked in (R1979, R1992).
enum AppearanceMode: String, CaseIterable, CustomStringConvertible {
    case light
    case dark
    case lightIncreasedContrast = "light increased contrast"
    case darkIncreasedContrast = "dark increased contrast"

    var description: String { rawValue }

    /// An environment SwiftUI resolves colours in. Increase Contrast has no
    /// public setter; the underscored one is meant for previews and tests.
    var environment: EnvironmentValues {
        var environment = EnvironmentValues()
        environment.colorScheme = (self == .light || self == .lightIncreasedContrast) ? .light : .dark
        environment._colorSchemeContrast = (self == .light || self == .dark) ? .standard : .increased
        return environment
    }
}

/// A gamma-encoded sRGB colour, components in 0...1.
struct RGB: Equatable, CustomStringConvertible {
    let red: Double
    let green: Double
    let blue: Double

    init(red: Double, green: Double, blue: Double) {
        self.red = red
        self.green = green
        self.blue = blue
    }

    /// `#RRGGBB`.
    init(hex: String) {
        let digits = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        let value = Int(digits, radix: 16)!
        self.init(
            red: Double((value >> 16) & 0xFF) / 255,
            green: Double((value >> 8) & 0xFF) / 255,
            blue: Double(value & 0xFF) / 255
        )
    }

    init(_ resolved: Color.Resolved) {
        self.init(red: Double(resolved.red), green: Double(resolved.green), blue: Double(resolved.blue))
    }

    var description: String {
        String(format: "#%02X%02X%02X", Int((red * 255).rounded()), Int((green * 255).rounded()), Int((blue * 255).rounded()))
    }

    func isClose(to other: RGB, tolerance: Double = 1.0 / 255) -> Bool {
        abs(red - other.red) <= tolerance && abs(green - other.green) <= tolerance && abs(blue - other.blue) <= tolerance
    }
}

/// WCAG 2.2 relative luminance and contrast ratio.
enum WCAG {
    static func relativeLuminance(_ color: RGB) -> Double {
        func linear(_ channel: Double) -> Double {
            channel <= 0.04045 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(color.red) + 0.7152 * linear(color.green) + 0.0722 * linear(color.blue)
    }

    static func contrastRatio(_ first: RGB, _ second: RGB) -> Double {
        let (a, b) = (relativeLuminance(first), relativeLuminance(second))
        return (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }

    /// R1979: 4.5:1 for text, 3:1 for large text and non-text UI.
    static func minimumRatio(for use: ContrastUse) -> Double {
        switch use {
        case .text: 4.5
        case .largeText, .nonText: 3
        }
    }
}

/// A foreground drawn on a background, by token name.
struct AuditPair {
    let foreground: String
    let background: String
    let minimumRatio: Double
}

struct ContrastFailure: CustomStringConvertible {
    let pair: AuditPair
    let mode: AppearanceMode
    let ratio: Double

    var description: String {
        "\(pair.foreground) on \(pair.background) in \(mode): \(String(format: "%.2f", ratio)):1, needs \(String(format: "%g", pair.minimumRatio)):1"
    }
}

enum ContrastAudit {
    /// Every pair below its minimum in any mode.
    static func failures(_ pairs: [AuditPair], color: (String, AppearanceMode) -> RGB) -> [ContrastFailure] {
        pairs.flatMap { pair in
            AppearanceMode.allCases.compactMap { mode -> ContrastFailure? in
                let ratio = WCAG.contrastRatio(color(pair.foreground, mode), color(pair.background, mode))
                return ratio < pair.minimumRatio ? ContrastFailure(pair: pair, mode: mode, ratio: ratio) : nil
            }
        }
    }

    /// The declared pairings of the real tokens.
    static var declaredPairs: [AuditPair] {
        ColorToken.pairings.map {
            AuditPair(foreground: $0.foreground.rawValue, background: $0.background.rawValue, minimumRatio: WCAG.minimumRatio(for: $0.use))
        }
    }

    /// A real token as SwiftUI resolves it in `mode`.
    static func resolved(_ name: String, _ mode: AppearanceMode) -> RGB {
        RGB(ColorToken(rawValue: name)!.color.resolve(in: mode.environment))
    }
}
