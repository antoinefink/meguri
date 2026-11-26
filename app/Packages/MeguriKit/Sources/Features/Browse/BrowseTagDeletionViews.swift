import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// Shared by sidebar row menus and the visible filter chip menu.
/// Capturing the target when the menu is built fences collection replacement.
struct BrowseDeleteTagButton: View {
    let deletion: BrowseTagDeletion
    let target: BrowseTagDeletionTarget
    var identifier = "browse.tag.delete"

    var body: some View {
        Button(role: .destructive) { deletion.delete(target) } label: {
            Label {
                Text(target.names.count == 1 ? "browse.tag.delete" : "browse.tag.deleteMany", bundle: .module)
            } icon: { Image(systemName: "trash") }
        }
        .disabled(!deletion.canDelete(target))
        .accessibilityIdentifier(identifier)
    }
}

struct BrowseTagDeletionFeedback: ViewModifier {
    let deletion: BrowseTagDeletion
    @Environment(\.locale) private var locale

    func body(content: Content) -> some View {
        content
            .overlay(alignment: .bottom) {
                if let result = deletion.announcement {
                    ToastView(Text(verbatim: BrowseCommandText.announcement(result, locale: locale)), systemImage: "checkmark")
                        .padding()
                        .accessibilityIdentifier("browse.tag.deleted")
                        .allowsHitTesting(false)
                }
            }
            .task(id: deletion.announcement?.serial) {
                guard let result = deletion.announcement else { return }
                AccessibilityNotification.Announcement(BrowseCommandText.announcement(result, locale: locale)).post()
                do { try await Task.sleep(for: .seconds(4)) } catch { return }
                deletion.dismissAnnouncement()
            }
            #if os(macOS)
            // A failure may arrive while the app is hidden. Keep it in the
            // content, without a timer or an alert that could close the app.
            .safeAreaInset(edge: .bottom) {
                if let target = deletion.failed {
                    GroupBox {
                        VStack(alignment: .leading) {
                            Text("browse.tag.failed.message", bundle: .module)
                                .fixedSize(horizontal: false, vertical: true)
                            HStack {
                                Button { deletion.delete(target) } label: {
                                    Text("browse.failed.tryAgain", bundle: .module)
                                }
                                .disabled(!deletion.canDelete(target))
                                .accessibilityIdentifier("browse.tag.failed.retry")
                                Button { deletion.dismissFailure() } label: {
                                    Text("browse.tag.failed.dismiss", bundle: .module)
                                }
                                .accessibilityIdentifier("browse.tag.failed.dismiss")
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    } label: {
                        Label {
                            Text("browse.tag.failed.title", bundle: .module)
                        } icon: { Image(systemName: "exclamationmark.triangle") }
                    }
                    .padding()
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("browse.tag.failed")
                    .onAppear {
                        AccessibilityNotification.Announcement(BrowseStrings.text("browse.tag.failed.message", locale: locale)).post()
                    }
                }
            }
            #else
            .alert(Text("browse.tag.failed.title", bundle: .module), isPresented: Binding(
                get: { deletion.failed != nil }, set: { if !$0 { deletion.dismissFailure() } })) {
                Button(role: .cancel) { deletion.dismissFailure() } label: { Text("browse.failed.command.ok", bundle: .module) }
            } message: {
                Text("browse.tag.failed.message", bundle: .module)
            }
            #endif
    }
}
