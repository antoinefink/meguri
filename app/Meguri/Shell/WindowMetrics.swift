import CoreGraphics

/// The Mac window's sizes (U150 owns the final values; R338 needs the minimum
/// to fit the four answer buttons in one row).
enum WindowMetrics {
    static let minimum = CGSize(width: 800, height: 560)
    static let standard = CGSize(width: 1180, height: 780)
}
