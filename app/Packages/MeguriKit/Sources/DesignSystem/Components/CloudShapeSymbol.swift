import SwiftUI

/// native-interface-contract N23.4: the cloud shapes for services that are not
/// Apple's iCloud. SF Symbols reserves every `icloud` symbol for iCloud and has
/// no unrestricted cloud with a badge inside, so this draws the unrestricted
/// `cloud` symbol with an unrestricted symbol centred in its body, as the
/// `icloud` shapes place theirs. Nothing of Apple's is modified: two symbols
/// are drawn together. It takes the font, weight and foreground of its place.
public struct CloudShapeSymbol: View {
    public enum Badge: String, Sendable, CaseIterable {
        case none, checkmark, arrows, exclamationmark, slash
    }
    let badge: Badge
    /// The arrows turn with the system rotate effect, the cloud stands still.
    let turning: Bool
    public init(_ badge: Badge, turning: Bool = false) { self.badge = badge; self.turning = turning }

    /// The cloud's body: the inner badge's centre, as fractions of the symbol's
    /// frame, and its size against the cloud's (the `icloud` shapes' layout).
    private static let bodyCentre = UnitPoint(x: 0.5, y: 0.6)
    private static let badgeScale: CGFloat = 0.5

    public var body: some View {
        switch badge {
        case .none:
            Image(systemName: "cloud")
        case .slash:
            // The slash crosses the whole cloud, with a gap cut around it.
            Image(systemName: "cloud")
                .mask {
                    Rectangle()
                        .overlay { slash(gap: true).blendMode(.destinationOut) }
                        .compositingGroup()
                }
                .overlay { slash(gap: false) }
        case .checkmark, .exclamationmark, .arrows:
            Image(systemName: "cloud")
                .overlay {
                    GeometryReader { frame in
                        inner
                            .scaleEffect(Self.badgeScale)
                            .position(x: frame.size.width * Self.bodyCentre.x, y: frame.size.height * Self.bodyCentre.y)
                    }
                }
        }
    }

    /// A line from the upper leading corner to the lower trailing one, as the
    /// `icloud` slash falls; its width follows the frame, so the font scales it.
    private func slash(gap: Bool) -> some View {
        GeometryReader { frame in
            let side = min(frame.size.width, frame.size.height)
            Capsule()
                .frame(width: hypot(frame.size.width, frame.size.height) * Self.slashLength,
                       height: side * (gap ? Self.slashWidth * 3 : Self.slashWidth))
                .rotationEffect(.radians(atan2(frame.size.height, frame.size.width)))
                .position(x: frame.size.width / 2, y: frame.size.height / 2)
        }
    }
    private static let slashLength: CGFloat = 0.95
    private static let slashWidth: CGFloat = 0.09

    @ViewBuilder private var inner: some View {
        switch badge {
        case .checkmark: Image(systemName: "checkmark").fontWeight(.bold)
        case .exclamationmark: Image(systemName: "exclamationmark").fontWeight(.bold)
        case .arrows:
            Image(systemName: "arrow.trianglehead.2.clockwise.rotate.90").fontWeight(.bold)
                .symbolEffect(.rotate, options: .repeating, isActive: turning)
        case .none, .slash: EmptyView()
        }
    }
}
