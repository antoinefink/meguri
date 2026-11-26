#if os(iOS)
import AppModel
import CoreBridge
import SwiftUI

/// Touch collection management only: every node, including implicit parents
/// and unused registry entries. No Browse search, selection or result session.
public struct CollectionTagsView: View {
    private let collection: ReviewCollection
    @Environment(\.scenePhase) private var scenePhase
    @State private var tags: [TagNode] = []
    @State private var shownGeneration: UInt64?
    @State private var loaded = false
    @State private var reading = false
    @State private var working = false
    @State private var readRevision: UInt64 = 0
    @State private var failure: Failure?

    private enum Failure: Equatable {
        case read, delete(String), undo

        var message: String {
            switch self {
            case .read: "Couldn’t load tags. Try again when the collection is available."
            case .delete(let name): "Couldn’t delete “\(name)”. Try again when the collection is available."
            case .undo: "Couldn’t undo that change. Try again when the collection is available."
            }
        }
    }

    public init(collection: ReviewCollection) { self.collection = collection }

    private var canDelete: Bool {
        !working && shownGeneration == collection.generation
            && collection.browseReadable && collection.browse.commands.ready()
    }

    public var body: some View {
        List {
            if let failure {
                Section {
                    Text(failure.message)
                        .accessibilityIdentifier("tags.list.error")
                    Button("Try Again") { retry(failure) }
                        .disabled(working || !collection.browseReadable)
                        .accessibilityIdentifier("tags.list.retry")
                }
            }
            if loaded, tags.isEmpty {
                ContentUnavailableView("No Tags", systemImage: "tag")
                    .accessibilityIdentifier("tags.list.empty")
            } else {
                Section {
                    ForEach(Self.rows(tags), id: \.fullName) { tag in
                        Text(Self.path(tag.fullName))
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("tags.list.row.\(tag.fullName)")
                            .swipeActions {
                                deleteButton(tag)
                            }
                            .contextMenu {
                                deleteButton(tag)
                            }
                            .accessibilityAction(named: Text("Delete Tag")) { delete(tag.fullName) }
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
                Button(action: undo) {
                    Label("Undo \(collection.collectionUndo.undoName ?? "Change")", systemImage: "arrow.uturn.backward")
                }
                .labelStyle(.iconOnly)
                .disabled(working || !collection.collectionUndo.canUndo)
                .accessibilityIdentifier("tags.list.undo")
            }
        }
        .overlay {
            if reading, !loaded { ProgressView().accessibilityIdentifier("tags.list.progress") }
        }
        .task(id: collection.generation) { await reload() }
        .onChange(of: collection.browseReadable) { _, readable in
            if readable { Task { await reload() } }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { Task { await reload() } }
        }
        // Collection undo also covers a keyboard/system command outside this toolbar.
        .onChange(of: collection.collectionUndo.busy) { _, busy in
            if !busy { Task { await reload() } }
        }
        .onDisappear { readRevision &+= 1 }
        .refreshable { await reload() }
    }

    private func deleteButton(_ tag: TagNode) -> some View {
        Button("Delete Tag", systemImage: "trash", role: .destructive) { delete(tag.fullName) }
            .disabled(!canDelete)
            .accessibilityIdentifier("tags.list.delete.\(tag.fullName)")
    }

    private func delete(_ name: String) {
        guard canDelete else { return }
        working = true
        failure = nil
        let generation = collection.generation
        Task {
            defer { working = false }
            do {
                // Integration dependency: BrowseModel.deleteTag(_ fullName: String)
                // async throws. The shared owner admits, awaits, reports failure,
                // and registers the collection undo step; the UI never mutates tags.
                try await collection.browse.deleteTag(name)
            } catch {
                if collection.generation == generation { failure = .delete(name) }
            }
            if collection.generation == generation { await reload() }
        }
    }

    private func undo() {
        let history = collection.collectionUndo
        guard !working, history.canUndo else { return }
        working = true
        failure = nil
        let generation = collection.generation, previous = history.undoStatus
        history.undo()
        Task {
            defer { working = false }
            await history.waitForCompletion()
            guard collection.generation == generation else { return }
            if history.undoStatus == previous { failure = .undo }
            await reload()
        }
    }

    private func retry(_ failure: Failure) {
        switch failure {
        case .read: Task { await reload() }
        case .delete(let name): delete(name)
        case .undo: undo()
        }
    }

    private func reload() async {
        guard collection.browseReadable else { return }
        readRevision &+= 1
        let revision = readRevision, generation = collection.generation
        reading = true
        do {
            let tree = try await CoreTagService().tagTree()
            guard revision == readRevision, generation == collection.generation, !Task.isCancelled else { return }
            tags = tree
            shownGeneration = generation
            loaded = true
            reading = false
            if failure == .read { failure = nil }
        } catch {
            guard revision == readRevision, generation == collection.generation, !Task.isCancelled else { return }
            reading = false
            if failure == nil { failure = .read }
        }
    }

    private static func rows(_ nodes: [TagNode]) -> [TagNode] {
        nodes.flatMap { [$0] + rows($0.children) }
    }

    private static func path(_ name: String) -> String {
        name.replacingOccurrences(of: "::", with: " › ")
    }
}
#endif
