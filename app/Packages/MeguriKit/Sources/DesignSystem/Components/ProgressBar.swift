import SwiftUI

/// A determinate progress bar (the review top bar, imports, syncs): the system
/// linear `ProgressView` in the system accent colour, whose changes animate
/// with the quick motion token and jump under Reduce Motion. Values outside
/// 0...1 are clamped.
public struct ProgressBar: View {
    private let value: Double
    private let label: Text

    public init(value: Double, label: Text) {
        self.value = value
        self.label = label
    }

    public var body: some View {
        ProgressView(value: min(max(value, 0), 1)) {
            label
        }
        .progressViewStyle(.linear)
        .labelsHidden()
        .motion(Motion.quick, value: value)
        .accessibilityLabel(label)
    }
}
