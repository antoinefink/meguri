import SwiftUI
import Testing
@testable import HeatmapKit

/// N24.6: with Increase Contrast every level of the heatmap reaches 3:1 against the surface
/// it is drawn on (the rubric's non-text rule), in light and dark, and the four levels stay
/// apart. The green is the system's, resolved as SwiftUI resolves it with Increase Contrast;
/// the empty day's fill drawn under a level is left out, which only lowers the ratios.
@Suite struct ActivityGridContrastTests {
    private struct RGB { let red, green, blue: Double }

    private static func environment(dark: Bool) -> EnvironmentValues {
        var environment = EnvironmentValues()
        environment.colorScheme = dark ? .dark : .light
        // No public setter; the underscored one is meant for previews and tests.
        environment._colorSchemeContrast = .increased
        return environment
    }

    private static func luminance(_ color: RGB) -> Double {
        func linear(_ channel: Double) -> Double {
            channel <= 0.04045 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linear(color.red) + 0.7152 * linear(color.green) + 0.0722 * linear(color.blue)
    }

    private static func ratio(_ first: RGB, _ second: RGB) -> Double {
        let (a, b) = (luminance(first), luminance(second))
        return (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }

    /// `color` at `opacity` over `surface`, in gamma-encoded sRGB as drawn.
    private static func over(_ color: RGB, _ opacity: Double, _ surface: RGB) -> RGB {
        func mix(_ top: Double, _ bottom: Double) -> Double { top * opacity + bottom * (1 - opacity) }
        return RGB(red: mix(color.red, surface.red), green: mix(color.green, surface.green), blue: mix(color.blue, surface.blue))
    }

    private static func gray(_ value: Double) -> RGB { RGB(red: value, green: value, blue: value) }

    private static let surfaces: [Bool: [RGB]] = [
        false: [gray(1)],
        true: [RGB(red: 0x1C / 255, green: 0x1C / 255, blue: 0x1E / 255),
               RGB(red: 0x2C / 255, green: 0x2C / 255, blue: 0x2E / 255), gray(0x32 / 255)],
    ]

    @Test(arguments: [false, true])
    func increasedContrastLevelsReachThreeToOne(dark: Bool) throws {
        let resolved = Color.green.resolve(in: Self.environment(dark: dark))
        let green = RGB(red: Double(resolved.red), green: Double(resolved.green), blue: Double(resolved.blue))
        for surface in try #require(Self.surfaces[dark]) {
            let ratios = try (1...4).map { level in
                let opacity = try #require(ActivityGrid.opacity(level: level, increasedContrast: true, dark: dark))
                return Self.ratio(Self.over(green, opacity, surface), surface)
            }
            #expect(ratios.allSatisfy { $0 >= 3 }, "dark \(dark), surface \(surface): \(ratios)")
            // Apart: each level clearly stronger than the one below it.
            for (lower, higher) in zip(ratios, ratios.dropFirst()) {
                #expect(higher - lower >= 0.3, "dark \(dark), surface \(surface): \(ratios)")
            }
        }
    }
}
