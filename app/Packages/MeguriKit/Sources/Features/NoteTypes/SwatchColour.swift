import CoreBridge
import CoreGraphics

extension CardTextColour {
    /// This colour for a colour control (W10.2).
    var cgColor: CGColor {
        CGColor(srgbRed: CGFloat(red) / 255, green: CGFloat(green) / 255, blue: CGFloat(blue) / 255, alpha: CGFloat(alpha) / 255)
    }

    /// A well whose declaration is absent or unreadable: empty (clear).
    static let unsetWell = CGColor(gray: 0, alpha: 0)
}

// A colour value of the person's styling, as the editor's colour swatches show
// it (`notetypes/workspace` W6). It is card data, not an interface colour:
// it is drawn as written, in sRGB.

#if os(macOS)
import AppKit

extension NSColor {
    static func swatch(_ colour: CardTextColour) -> NSColor {
        NSColor(srgbRed: CGFloat(colour.red) / 255, green: CGFloat(colour.green) / 255, blue: CGFloat(colour.blue) / 255,
                alpha: CGFloat(colour.alpha) / 255)
    }
}
#else
import UIKit

extension UIColor {
    static func swatch(_ colour: CardTextColour) -> UIColor {
        UIColor(red: CGFloat(colour.red) / 255, green: CGFloat(colour.green) / 255, blue: CGFloat(colour.blue) / 255,
                alpha: CGFloat(colour.alpha) / 255)
    }
}
#endif
