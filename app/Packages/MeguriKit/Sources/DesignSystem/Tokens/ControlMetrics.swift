import CoreGraphics

/// Minimum native interaction region, independent of a control's visible chrome.
nonisolated public enum ControlMetrics {
    #if os(macOS)
    public static let minimumTarget: CGFloat = 28
    #else
    public static let minimumTarget: CGFloat = 44
    #endif
}
