import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// Shared by sidebar row menus, search suggestions and the touch token menu.
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
    var active = true
    @Environment(\.locale) private var locale

    func body(content: Content) -> some View {
        content
            .overlay(alignment: .bottom) {
                if active, let result = deletion.announcement {
                    ToastView(Text(verbatim: BrowseCommandText.announcement(result, locale: locale)), systemImage: "checkmark")
                        .padding()
                        .accessibilityIdentifier("browse.tag.deleted")
                        .allowsHitTesting(false)
                }
            }
            .task(id: active ? deletion.announcement?.serial : nil) {
                guard active, let result = deletion.announcement else { return }
                AccessibilityNotification.Announcement(BrowseCommandText.announcement(result, locale: locale)).post()
                do { try await Task.sleep(for: .seconds(4)) } catch { return }
                deletion.dismissAnnouncement()
            }
            .alert(Text("browse.tag.failed.title", bundle: .module), isPresented: Binding(
                get: { active && deletion.failed != nil }, set: { if !$0 { deletion.dismissFailure() } })) {
                Button(role: .cancel) { deletion.dismissFailure() } label: { Text("browse.failed.command.ok", bundle: .module) }
            } message: {
                Text("browse.tag.failed.message", bundle: .module)
            }
    }
}

#if !os(macOS)
/// The compact tag filter surface uses a native list so its row menus and
/// trailing swipe actions remain available independently of search completion.
struct BrowseTagFilterSheet: View {
    let model: BrowseModel

    var body: some View {
        NavigationStack {
            List {
                ForEach(model.tags, id: \.fullName) { node in
                    BrowseTagFilterBranch(model: model, node: node)
                }
            }
            .overlay {
                if model.tags.isEmpty {
                    ContentUnavailableView {
                        Label { Text("browse.tags.empty", bundle: .module) } icon: { Image(systemName: "tag") }
                    }
                }
            }
            .navigationTitle(Text("browse.tags.title", bundle: .module))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button { model.showingTags = false } label: { Text("browse.tags.done", bundle: .module) }
                        .accessibilityIdentifier("browse.tags.done")
                }
            }
            .modifier(BrowseTagDeletionFeedback(deletion: model.tagDeletion))
        }
        .accessibilityIdentifier("browse.tags.list")
    }
}

private struct BrowseTagFilterBranch: View {
    let model: BrowseModel
    let node: CoreBridge.TagNode

    var body: some View {
        if node.children.isEmpty {
            row
        } else {
            DisclosureGroup(isExpanded: Binding(get: { model.tagExpanded(node) }, set: { model.setTagExpanded(node, $0) })) {
                ForEach(node.children, id: \.fullName) { child in
                    AnyView(BrowseTagFilterBranch(model: model, node: child))
                }
            } label: { row }
        }
    }

    private var row: some View {
        let target = model.tagDeletion.target([node.fullName])
        return HStack {
            Button {
                model.activate(.tag(node.fullName))
                model.showingTags = false
            } label: { Text(verbatim: node.name).frame(maxWidth: .infinity, alignment: .leading) }
                .buttonStyle(.plain)
                .accessibilityIdentifier("browse.tags.row.\(node.fullName)")
            Menu {
                BrowseDeleteTagButton(deletion: model.tagDeletion, target: target)
            } label: { Image(systemName: "ellipsis.circle") }
                .accessibilityLabel(Text(String(format: BrowseStrings.text("browse.tags.actions"), node.fullName)))
                .accessibilityIdentifier("browse.tags.actions.\(node.fullName)")
        }
        .contextMenu { BrowseDeleteTagButton(deletion: model.tagDeletion, target: target) }
        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
            BrowseDeleteTagButton(deletion: model.tagDeletion, target: target)
        }
    }
}
#endif
