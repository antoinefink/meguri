import CoreGraphics

/// How wide content grows in a wide window (KTD20, brief principle 6: never
/// a phone layout stretched to a window). A column of text and controls
/// stops at `readable` and is centred in a wider container; a primary
/// action that fills the width on an iPhone stops at `primaryAction` in
/// regular width.
nonisolated public enum ContentWidth {
    /// The widest a column of text and controls grows.
    public static let readable: CGFloat = 640
    /// The widest a full-width primary action grows in regular width.
    public static let primaryAction: CGFloat = 320
    /// One Mac answer button: four stay equal whatever their label (N3).
    public static let answerButton: CGFloat = 96
}
