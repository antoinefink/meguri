import CoreGraphics

/// Minimum native interaction region, independent of a control's visible chrome.
nonisolated public enum ControlMetrics {
    #if os(macOS)
    public static let minimumTarget: CGFloat = 28
    #else
    public static let minimumTarget: CGFloat = 44
    #endif
    /// The outline a window draws while a dropped file would be taken
    /// (`package/file-intake` K1): as wide as the system's focus ring.
    public static let dropOutline: CGFloat = 3
}
