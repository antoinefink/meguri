#if os(iOS)
import AppModel
import CoreBridge
import SwiftUI

/// Touch collection management uses Browse's tag tree and deletion owner,
/// without opening Browse, running a search or keeping a second tree.
public struct CollectionTagsView: View {
    private let collection: ReviewCollection
    @Environment(\.scenePhase) private var scenePhase

    public init(collection: ReviewCollection) { self.collection = collection }

    private var model: BrowseModel { collection.browse }
    private var deletion: BrowseTagDeletion { model.tagDeletion }
    private var history: CollectionUndoModel { collection.collectionUndo }

    public var body: some View {
        List {
            if deletion.failed != nil {
                Section {
                    Text("Couldn’t delete that tag. Try again when the collection is available.")
                        .accessibilityIdentifier("tags.list.error")
                    if let target = deletion.failed {
                        Button("Try Again") { deletion.delete(target) }
                            .disabled(!deletion.canDelete(target))
                            .accessibilityIdentifier("tags.list.retry")
                    }
                }
            }
            if history.failed {
                Text("Couldn’t undo that change. Try again when the collection is available.")
                    .accessibilityIdentifier("tags.list.undoError")
            }
            if model.tagsFailed {
                Section {
                    Text("Couldn’t load tags. Try again when the collection is available.")
                        .accessibilityIdentifier("tags.list.readError")
                    Button("Try Again") { Task { await model.refreshTags() } }
                        .disabled(!model.ready())
                        .accessibilityIdentifier("tags.list.retry")
                }
            }
            if model.tagsLoaded, model.tags.isEmpty {
                ContentUnavailableView("No Tags", systemImage: "tag")
                    .accessibilityIdentifier("tags.list.empty")
            } else {
                Section {
                    ForEach(Self.rows(model.tags), id: \.fullName) { tag in
                        row(tag)
                    }
                } footer: {
                    Text("Swipe or touch and hold a tag to delete it. Deleting a parent also deletes its subtags. Notes and cards are kept.")
                }
            }
        }
        .navigationTitle("Tags")
        .accessibilityIdentifier("tags.list.content")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { history.undo() } label: {
                    Label("Undo \(history.undoName ?? "Change")", systemImage: "arrow.uturn.backward")
                }
                .labelStyle(.iconOnly)
                .disabled(!history.canUndo)
                .accessibilityIdentifier("tags.list.undo")
            }
        }
        .overlay {
            if model.tagsReading, !model.tagsLoaded {
                ProgressView().accessibilityIdentifier("tags.list.progress")
            }
        }
        .task(id: collection.generation) { await model.refreshTags() }
        .onChange(of: model.ready()) { _, ready in
            if ready { Task { await model.refreshTags() } }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { Task { await model.refreshTags() } }
        }
        .onChange(of: deletion.announcement) { _, result in
            if result != nil { AccessibilityNotification.Announcement("Tag deleted.").post() }
        }
        .refreshable { await model.refreshTags() }
    }

    private func row(_ tag: TagNode) -> some View {
        // Capture the collection identity with the rendered row, including its
        // accessibility action; a retained native menu cannot target a new file.
        let target = deletion.target([tag.fullName])
        return Text(tag.fullName.replacingOccurrences(of: "::", with: " › "))
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityIdentifier("tags.list.row.\(tag.fullName)")
            .swipeActions(allowsFullSwipe: false) { deleteButton(target) }
            .contextMenu { deleteButton(target) }
            .accessibilityAction(named: Text("Delete Tag")) { deletion.delete(target) }
    }

    private func deleteButton(_ target: BrowseTagDeletionTarget) -> some View {
        Button("Delete Tag", systemImage: "trash", role: .destructive) { deletion.delete(target) }
            .disabled(!deletion.canDelete(target))
            .accessibilityIdentifier("tags.list.delete.\(target.names[0])")
    }

    private static func rows(_ nodes: [TagNode]) -> [TagNode] {
        nodes.flatMap { [$0] + rows($0.children) }
    }
}
#endif
