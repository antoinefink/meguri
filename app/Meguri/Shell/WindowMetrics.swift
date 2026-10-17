import CoreGraphics

/// The Mac window (R6, R12): its scene id and sizes. The minimum fits the
/// sidebar beside a detail column wide enough for the four answer buttons in
/// one row (R338).
enum WindowMetrics {
    /// The id of the app's one `Window` scene (`openWindow(id:)`).
    static let mainWindowID = "main"
    static let minimum = CGSize(width: 800, height: 560)
    static let standard = CGSize(width: 1180, height: 780)
    /// The sidebar column's widths: a source list's, wide enough at first
    /// for a nested deck's name beside the section rows (the system default
    /// leaves about 145 points). A width the user drags to is the window's
    /// to restore (R12), within these bounds.
    static let sidebarMinimumWidth: CGFloat = 180
    static let sidebarIdealWidth: CGFloat = 220
    static let sidebarMaximumWidth: CGFloat = 320
}
