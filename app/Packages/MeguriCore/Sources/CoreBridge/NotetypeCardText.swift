import Foundation
import Instrumentation
internal import MeguriCoreBindings

// The Card Text row over a note type's styling (`notetypes/workspace` W10,
// W10.1, W10.2): pure functions of the CSS text, which read no collection.

/// An 8-bit sRGB colour with alpha.
public struct CardTextColour: Sendable, Hashable {
    public var red: UInt8
    public var green: UInt8
    public var blue: UInt8
    public var alpha: UInt8
    public init(red: UInt8, green: UInt8, blue: UInt8, alpha: UInt8 = 255) {
        self.red = red
        self.green = green
        self.blue = blue
        self.alpha = alpha
    }
    init(_ raw: MeguriCoreBindings.CardTextColour) {
        self.init(red: raw.red, green: raw.green, blue: raw.blue, alpha: raw.alpha)
    }
    var raw: MeguriCoreBindings.CardTextColour { .init(red: red, green: green, blue: blue, alpha: alpha) }
}

/// The Font control: "System", or a family name.
public enum CardTextFont: Sendable, Hashable {
    case system
    case family(String)
    init(_ raw: MeguriCoreBindings.CardTextFont) {
        switch raw {
        case .system: self = .system
        case .family(let name): self = .family(name)
        }
    }
    var raw: MeguriCoreBindings.CardTextFont {
        switch self {
        case .system: .system
        case .family(let name): .family(name: name)
        }
    }
}

/// The Alignment control, in the interface's direction.
public enum CardTextAlignment: Sendable, Hashable {
    case leading
    case center
    case trailing
    init(_ raw: MeguriCoreBindings.CardTextAlignment) {
        switch raw {
        case .leading: self = .leading
        case .center: self = .center
        case .trailing: self = .trailing
        }
    }
    var raw: MeguriCoreBindings.CardTextAlignment {
        switch self {
        case .leading: .leading
        case .center: .center
        case .trailing: .trailing
        }
    }
}

/// What the Card Text row's five controls show; nil is a control's empty
/// state.
public struct CardTextRow: Sendable, Hashable {
    public let font: CardTextFont?
    /// Points.
    public let size: Double?
    public let alignment: CardTextAlignment?
    public let textColour: CardTextColour?
    public let background: CardTextColour?
    public init(font: CardTextFont?, size: Double?, alignment: CardTextAlignment?, textColour: CardTextColour?, background: CardTextColour?) {
        self.font = font
        self.size = size
        self.alignment = alignment
        self.textColour = textColour
        self.background = background
    }
    init(_ raw: MeguriCoreBindings.CardTextRow) {
        self.init(font: raw.font.map(CardTextFont.init), size: raw.size, alignment: raw.alignment.map(CardTextAlignment.init),
                  textColour: raw.textColour.map(CardTextColour.init), background: raw.background.map(CardTextColour.init))
    }
}

/// One control's change.
public enum CardTextChange: Sendable, Hashable {
    case font(CardTextFont)
    /// Points, above 0.
    case size(Double)
    case alignment(CardTextAlignment)
    case textColour(CardTextColour)
    /// Written without alpha.
    case background(CardTextColour)
    var raw: MeguriCoreBindings.CardTextChange {
        switch self {
        case .font(let font): .font(font: font.raw)
        case .size(let points): .size(points: points)
        case .alignment(let alignment): .alignment(alignment: alignment.raw)
        case .textColour(let colour): .textColour(colour: colour.raw)
        case .background(let colour): .background(colour: colour.raw)
        }
    }
}

/// An edit's whole new CSS and the range of the value it wrote.
public struct CardTextEdited: Sendable, Hashable {
    public let css: String
    /// The value written, in `css`.
    public let value: Range<String.Index>
    public init(css: String, value: Range<String.Index>) {
        self.css = css
        self.value = value
    }
    init(_ raw: MeguriCoreBindings.CardTextEdited) throws(BridgeError) {
        let utf8 = raw.css.utf8
        guard let start = utf8.index(utf8.startIndex, offsetBy: Int(raw.valueStart), limitedBy: utf8.endIndex),
              let end = utf8.index(utf8.startIndex, offsetBy: Int(raw.valueEnd), limitedBy: utf8.endIndex),
              start <= end else {
            throw .internal("Card Text value range outside the CSS")
        }
        css = raw.css
        value = start..<end
    }
}

extension CoreNotetypeService {
    /// W10: the Card Text row of `css`, or nil when the row is absent.
    public func cardText(css: String, rightToLeft: Bool) async throws(BridgeError) -> CardTextRow? {
        try await QueryLane.shared.call(.notetypeCardText) {
            try MeguriCoreBindings.notetypeCardText(css: css, rightToLeft: rightToLeft).map(CardTextRow.init)
        }
    }

    /// W10: one control's change; every other byte of `css` stays.
    public func editCardText(css: String, change: CardTextChange, rightToLeft: Bool) async throws(BridgeError) -> CardTextEdited {
        let raw = try await QueryLane.shared.call(.notetypeCardTextEdit) {
            try MeguriCoreBindings.notetypeCardTextEdit(css: css, change: change.raw, rightToLeft: rightToLeft)
        }
        return try CardTextEdited(raw)
    }
}
