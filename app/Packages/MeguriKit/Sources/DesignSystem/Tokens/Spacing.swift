import CoreGraphics

/// Spacing, padding and insets (R1992, KTD20): a 4-pt scale. Features write
/// `.padding(Spacing.medium)` or `VStack(spacing: Spacing.small)`; a number
/// other than 0 in those positions fails the `numeric-spacing` lint rule.
nonisolated public enum Spacing {
    public static let xxSmall: CGFloat = 4
    public static let xSmall: CGFloat = 8
    public static let small: CGFloat = 12
    public static let medium: CGFloat = 16
    public static let large: CGFloat = 24
    public static let xLarge: CGFloat = 32
    public static let xxLarge: CGFloat = 48

    /// Every step, smallest first. A new step joins this list.
    public static let scale: [CGFloat] = [xxSmall, xSmall, small, medium, large, xLarge, xxLarge]
}
