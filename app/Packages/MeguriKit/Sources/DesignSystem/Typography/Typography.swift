import SwiftUI

/// Text style helpers (R1993, KTD20). Every font here is a system text style,
/// so it scales with Dynamic Type; fixed sizes and custom fonts fail the
/// `fixed-font-size` lint rule everywhere, this folder included. Features
/// use system styles directly (`.font(.body)`) or these named roles.
public enum Typography {
    /// Count numbers in deck rows and headers: body with monospaced digits,
    /// so columns of counts line up.
    public static let count: Font = .body.monospacedDigit()
    /// Column titles above counts.
    public static let countHeader: Font = .caption
    /// Titles of content sections outside system lists.
    public static let sectionTitle: Font = .headline
    /// Secondary line under a section title.
    public static let sectionSubtitle: Font = .subheadline
    /// Labels of component previews and captions.
    public static let caption: Font = .caption
}
