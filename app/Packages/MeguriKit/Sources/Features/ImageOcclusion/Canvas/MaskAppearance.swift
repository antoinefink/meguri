import Foundation
import SwiftUI

/// Values stored in study-card content (mask-editor M3.2/M3.4 and format F7).
/// Interface chrome continues to use system semantic styles.
enum MaskAppearance {
    static let defaultFill = "#ffeba2"
    static let labelFill = "#000000"
    static let defaultColor = maskColor(defaultFill, fallback: .primary)
}

/// CSS values are decoded for drawing only; imported fill text stays in the
/// codec's source. Components use CSS clamping and unpremultiplied alpha.
func maskColor(_ value: String?, fallback: Color) -> Color {
    guard let value, let color = MaskCSSColor(value) else { return fallback }
    return Color(.sRGB, red: color.red, green: color.green, blue: color.blue, opacity: color.alpha)
}

struct MaskCSSColor: Equatable {
    let red: Double
    let green: Double
    let blue: Double
    let alpha: Double

    private init(_ red: Double, _ green: Double, _ blue: Double, _ alpha: Double = 1) {
        self.red = Self.clamp(red); self.green = Self.clamp(green)
        self.blue = Self.clamp(blue); self.alpha = Self.clamp(alpha)
    }
    init?(_ source: String) {
        let value = source.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if value == "transparent" { self.init(0, 0, 0, 0); return }
        if let hex = Self.names[value] { self.init(hex: hex); return }
        if value.hasPrefix("#") { self.init(hex: String(value.dropFirst())); return }
        guard let open = value.firstIndex(of: "("), value.last == ")" else { return nil }
        let function = String(value[..<open])
        let body = value[value.index(after: open)..<value.index(before: value.endIndex)]
        let parts = body.replacingOccurrences(of: ",", with: " ")
            .replacingOccurrences(of: "/", with: " / ").split(whereSeparator: { $0.isWhitespace }).map(String.init)
        let channels: [String]
        let opacity: Double
        if let slash = parts.firstIndex(of: "/") {
            guard slash == 3, parts.count == 5, let a = Self.component(parts[4], scale: 1) else { return nil }
            channels = Array(parts.prefix(3)); opacity = a
        } else if parts.count == 4 {
            guard let a = Self.component(parts[3], scale: 1) else { return nil }
            channels = Array(parts.prefix(3)); opacity = a
        } else {
            guard parts.count == 3 else { return nil }
            channels = parts; opacity = 1
        }
        switch function {
        case "rgb", "rgba":
            guard let r = Self.component(channels[0], scale: 255),
                  let g = Self.component(channels[1], scale: 255),
                  let b = Self.component(channels[2], scale: 255) else { return nil }
            self.init(r, g, b, opacity)
        case "hsl", "hsla", "hwb":
            guard let hue = Self.hue(channels[0]), channels[1].hasSuffix("%"), channels[2].hasSuffix("%"),
                  let first = Self.component(channels[1], scale: 1), let second = Self.component(channels[2], scale: 1) else { return nil }
            let h = hue - floor(hue)
            let pure = (0..<3).map { index -> Double in
                let k = (Double(index * 2) + h * 6).truncatingRemainder(dividingBy: 6)
                return Self.clamp(abs(k - 3) - 1)
            }
            // The pure hue channels are R, B, G in this triangular construction.
            let rgb = [pure[0], pure[2], pure[1]]
            if function == "hwb" {
                let total = first + second
                if total >= 1 { self.init(first / total, first / total, first / total, opacity) }
                else { self.init(rgb[0] * (1 - total) + first, rgb[1] * (1 - total) + first, rgb[2] * (1 - total) + first, opacity) }
            } else {
                let amplitude = first * min(second, 1 - second)
                self.init(second + amplitude * (2 * rgb[0] - 1), second + amplitude * (2 * rgb[1] - 1), second + amplitude * (2 * rgb[2] - 1), opacity)
            }
        default: return nil
        }
    }
    private init?(hex: String) {
        guard [3, 4, 6, 8].contains(hex.count), hex.allSatisfy({ $0.isASCII && $0.isHexDigit }) else { return nil }
        let expanded = hex.count < 5 ? hex.map { "\($0)\($0)" }.joined() : hex
        guard let bits = UInt32(expanded, radix: 16) else { return nil }
        let hasAlpha = expanded.count == 8
        let rgb = hasAlpha ? bits >> 8 : bits
        self.init(Double((rgb >> 16) & 255) / 255, Double((rgb >> 8) & 255) / 255,
                  Double(rgb & 255) / 255, hasAlpha ? Double(bits & 255) / 255 : 1)
    }
    private static func clamp(_ value: Double) -> Double { min(1, max(0, value)) }
    private static func component(_ text: String, scale: Double) -> Double? {
        let percent = text.hasSuffix("%")
        guard let value = Double(percent ? String(text.dropLast()) : text), value.isFinite else { return nil }
        return clamp(value / (percent ? 100 : scale))
    }
    private static func hue(_ text: String) -> Double? {
        for (suffix, scale) in [("turn", 1.0), ("grad", 400.0), ("deg", 360.0), ("rad", 2 * Double.pi)] where text.hasSuffix(suffix) {
            guard let value = Double(text.dropLast(suffix.count)), value.isFinite else { return nil }
            return value / scale
        }
        guard let value = Double(text), value.isFinite else { return nil }
        return value / 360
    }

    // Standard CSS named colors, independent of platform semantic color names.
    private static let names: [String: String] = {
        let entries = """
        aliceblue:f0f8ff antiquewhite:faebd7 aqua:00ffff aquamarine:7fffd4 azure:f0ffff
        beige:f5f5dc bisque:ffe4c4 black:000000 blanchedalmond:ffebcd blue:0000ff blueviolet:8a2be2 brown:a52a2a burlywood:deb887
        cadetblue:5f9ea0 chartreuse:7fff00 chocolate:d2691e coral:ff7f50 cornflowerblue:6495ed cornsilk:fff8dc crimson:dc143c cyan:00ffff
        darkblue:00008b darkcyan:008b8b darkgoldenrod:b8860b darkgray:a9a9a9 darkgreen:006400 darkgrey:a9a9a9 darkkhaki:bdb76b darkmagenta:8b008b darkolivegreen:556b2f darkorange:ff8c00 darkorchid:9932cc darkred:8b0000 darksalmon:e9967a darkseagreen:8fbc8f darkslateblue:483d8b darkslategray:2f4f4f darkslategrey:2f4f4f darkturquoise:00ced1 darkviolet:9400d3
        deeppink:ff1493 deepskyblue:00bfff dimgray:696969 dimgrey:696969 dodgerblue:1e90ff
        firebrick:b22222 floralwhite:fffaf0 forestgreen:228b22 fuchsia:ff00ff
        gainsboro:dcdcdc ghostwhite:f8f8ff gold:ffd700 goldenrod:daa520 gray:808080 green:008000 greenyellow:adff2f grey:808080
        honeydew:f0fff0 hotpink:ff69b4 indianred:cd5c5c indigo:4b0082 ivory:fffff0 khaki:f0e68c
        lavender:e6e6fa lavenderblush:fff0f5 lawngreen:7cfc00 lemonchiffon:fffacd lightblue:add8e6 lightcoral:f08080 lightcyan:e0ffff lightgoldenrodyellow:fafad2 lightgray:d3d3d3 lightgreen:90ee90 lightgrey:d3d3d3 lightpink:ffb6c1 lightsalmon:ffa07a lightseagreen:20b2aa lightskyblue:87cefa lightslategray:778899 lightslategrey:778899 lightsteelblue:b0c4de lightyellow:ffffe0 lime:00ff00 limegreen:32cd32 linen:faf0e6
        magenta:ff00ff maroon:800000 mediumaquamarine:66cdaa mediumblue:0000cd mediumorchid:ba55d3 mediumpurple:9370db mediumseagreen:3cb371 mediumslateblue:7b68ee mediumspringgreen:00fa9a mediumturquoise:48d1cc mediumvioletred:c71585 midnightblue:191970 mintcream:f5fffa mistyrose:ffe4e1 moccasin:ffe4b5
        navajowhite:ffdead navy:000080 oldlace:fdf5e6 olive:808000 olivedrab:6b8e23 orange:ffa500 orangered:ff4500 orchid:da70d6
        palegoldenrod:eee8aa palegreen:98fb98 paleturquoise:afeeee palevioletred:db7093 papayawhip:ffefd5 peachpuff:ffdab9 peru:cd853f pink:ffc0cb plum:dda0dd powderblue:b0e0e6 purple:800080
        rebeccapurple:663399 red:ff0000 rosybrown:bc8f8f royalblue:4169e1
        saddlebrown:8b4513 salmon:fa8072 sandybrown:f4a460 seagreen:2e8b57 seashell:fff5ee sienna:a0522d silver:c0c0c0 skyblue:87ceeb slateblue:6a5acd slategray:708090 slategrey:708090 snow:fffafa springgreen:00ff7f steelblue:4682b4
        tan:d2b48c teal:008080 thistle:d8bfd8 tomato:ff6347 turquoise:40e0d0 violet:ee82ee wheat:f5deb3 white:ffffff whitesmoke:f5f5f5 yellow:ffff00 yellowgreen:9acd32
        """
        return Dictionary(uniqueKeysWithValues: entries.split(whereSeparator: { $0.isWhitespace }).map {
            let pair = $0.split(separator: ":"); return (String(pair[0]), String(pair[1]))
        })
    }()
}
