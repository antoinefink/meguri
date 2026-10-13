import SwiftUI

/// Corner radii (R1992, KTD20). Shapes inside a system container (a window,
/// sheet, glass bar or a `containerShape`) use `Radius.concentric`, whose
/// corners follow the container's, so nested corners stay concentric. Free
/// standing shapes use one of the fixed steps with continuous corners. A
/// number in a radius position fails the `numeric-radius` lint rule.
nonisolated public enum Radius {
    /// Chips, badges and small controls.
    public static let small: CGFloat = 8
    /// Text fields, inset rows and component previews.
    public static let medium: CGFloat = 12
    /// Panels and large surfaces.
    public static let large: CGFloat = 20

    /// Every fixed step, smallest first.
    public static let scale: [CGFloat] = [small, medium, large]

    /// A rectangle concentric with its container, never less round than
    /// `small` when the container has no corners.
    public static var concentric: ConcentricRectangle {
        ConcentricRectangle(corners: .concentric(minimum: .fixed(small)), isUniform: true)
    }

    /// A free-standing rounded rectangle with continuous corners.
    public static func rounded(_ radius: CGFloat) -> RoundedRectangle {
        RoundedRectangle(cornerRadius: radius, style: .continuous)
    }
}
