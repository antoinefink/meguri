import SwiftUI

/// Values stored in study-card content (mask-editor M3.2/M3.4 and format F7).
/// Interface chrome continues to use system semantic styles.
enum MaskAppearance {
    static let defaultFill = "#ffeba2"
    static let labelFill = "#000000"
    static let defaultColor = maskColor(defaultFill, fallback: .primary)
}

func maskColor(_ value: String?, fallback: Color) -> Color {
    guard let value else { return fallback }
    if value == "transparent" { return .clear }
    guard value.hasPrefix("#"), value.count == 7, let rgb = UInt32(value.dropFirst(), radix: 16) else { return fallback }
    return Color(.sRGB, red: Double((rgb >> 16) & 255) / 255, green: Double((rgb >> 8) & 255) / 255,
                 blue: Double(rgb & 255) / 255, opacity: 1)
}
