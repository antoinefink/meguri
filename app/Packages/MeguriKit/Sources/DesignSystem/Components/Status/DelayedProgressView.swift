import SwiftUI

/// Unobtrusive feedback for an actual read with an unknown duration. Insert only
/// while waiting; removal cancels the reveal, so fast reads never flash or speak
/// a hidden indicator. The destination supplies an assistive name, with no caption.
public struct DelayedProgressView: View {
    private let label: Text
    private let delay: Duration
    @State private var visible = false

    public init(_ label: Text, delay: Duration = .milliseconds(500)) {
        self.label = label
        self.delay = delay
    }

    public var body: some View {
        ZStack {
            if visible {
                ProgressView()
                    .controlSize(.small)
                    .accessibilityLabel(label)
            }
        }
        .task {
            visible = false
            do { try await Task.sleep(for: delay) } catch { return }
            guard !Task.isCancelled else { return }
            visible = true
        }
        .onDisappear { visible = false }
    }
}
