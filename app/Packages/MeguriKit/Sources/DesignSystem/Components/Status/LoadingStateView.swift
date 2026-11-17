import SwiftUI

/// What a screen or section shows while its content loads: a system progress
/// indicator with a label, indeterminate or with a fraction done (0...1).
public struct LoadingStateView: View {
    private let label: Text
    private let fractionDone: Double?

    public init(_ label: Text, fractionDone: Double? = nil) {
        self.label = label
        self.fractionDone = fractionDone
    }

    public var body: some View {
        Group {
            if let fractionDone {
                ProgressView(value: min(max(fractionDone, 0), 1)) { label }
            } else {
                ProgressView { label }
            }
        }
        .foregroundStyle(.token(.textSecondary))
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
