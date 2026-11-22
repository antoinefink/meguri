import CoreBridge

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
