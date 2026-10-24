#if os(macOS)
import AppKit
import AppModel
import CoreBridge
import SwiftUI

struct NativeCommandContext {
    let collection: ReviewCollection
    let host: AppNavigation.SheetHost

    var fileActionsAvailable: Bool {
        collection.navigation.sheet == nil && !collection.manualTransfers.busy
            && collection.manualTransfers.preview == nil && collection.manualTransfers.mediaPreview == nil
    }
    func present(_ route: SheetRoute) {
        guard collection.navigation.sheet == nil else { return }
        switch route {
        case .collectionEntry:
            guard fileActionsAvailable, collection.entry.canPresent else { return }
            collection.entry.present()
        case .collectionExport:
            guard fileActionsAvailable, collection.collectionExport.canPresent else { return }
            collection.collectionExport.present()
        case .manualTransfer:
            // Reopening must also admit an active operation or authoritative
            // preview. Only the workflow's explicit actions start work.
            guard !collection.entry.busy, !collection.collectionExport.busy else { return }
            collection.navigation.present(.manualTransfer)
        default: return
        }
        collection.navigation.sheetHost = host
    }
    var review: ReviewCoordinator? {
        guard host == .main, collection.navigation.sheet == nil else { return nil }
        return collection.review
    }
    var reviewInputAvailable: Bool {
        guard let review else { return false }
        return ReviewInputOwnership.admitsNativeCommand(review)
    }
    var selectedDeck: DeckID? {
        guard host == .main, fileActionsAvailable, collection.navigation.section == .home,
              let deck = collection.navigation.homeDeckPage, collection.deck(deck.rawValue) != nil else { return nil }
        return deck
    }
}

private struct NativeCommandContextKey: FocusedValueKey { typealias Value = NativeCommandContext }
extension FocusedValues {
    var nativeCommandContext: NativeCommandContext? {
        get { self[NativeCommandContextKey.self] }
        set { self[NativeCommandContextKey.self] = newValue }
    }
}

struct StudyCommands: Commands {
    @FocusedValue(\.nativeCommandContext) private var context
    var body: some Commands {
        CommandGroup(after: .newItem) {
            Button("Import Collection…") { context?.present(.collectionEntry) }
                .keyboardShortcut("i", modifiers: .command)
                .disabled(context?.fileActionsAvailable != true || context?.collection.entry.canPresent != true)
            Button("Export Collection…") { context?.present(.collectionExport) }
                .keyboardShortcut("e", modifiers: [.command, .shift])
                .disabled(context?.fileActionsAvailable != true || context?.collection.collectionExport.canPresent != true)
        }
        // Keep the system Edit responder chain for native text/DOM undo and redo.
        // Explicit answer restoration remains available in Study and card input.
        CommandMenu("Study") {
            Button("Study") {
                guard let context, let deck = context.selectedDeck, context.collection.canStudy else { return }
                Task { await context.collection.startStudy(deckID: deck.rawValue) }
            }
            .keyboardShortcut("r", modifiers: .command)
            .disabled(context?.selectedDeck == nil || context?.collection.canStudy != true)
            Divider()
            Button("Show Answer") {
                guard context?.reviewInputAvailable == true else { return }
                context?.review?.reveal()
            }
            .keyboardShortcut(.space, modifiers: [])
            .disabled(context?.reviewInputAvailable != true || context?.review?.canReveal != true)
            rating("Again", .again, key: "1")
            rating("Hard", .hard, key: "2")
            rating("Good", .good, key: "3")
            rating("Easy", .easy, key: "4")
            Divider()
            Button("Undo Answer") {
                guard context?.reviewInputAvailable == true else { return }
                context?.review?.restore()
            }
            .keyboardShortcut("z", modifiers: .command)
            .disabled(context?.reviewInputAvailable != true || context?.review?.canUndo != true)
            Button("Redo Answer") {
                guard context?.reviewInputAvailable == true else { return }
                context?.review?.restore(redo: true)
            }
            .keyboardShortcut("z", modifiers: [.command, .shift])
            .disabled(context?.reviewInputAvailable != true || context?.review?.canRedo != true)
            Divider()
            Button("Back to Decks") {
                guard let context, context.reviewInputAvailable, context.review?.canFinish == true else { return }
                Task { await context.collection.finishReview() }
            }
            .keyboardShortcut(.escape, modifiers: [])
            .disabled(context?.reviewInputAvailable != true || context?.review?.canFinish != true)
        }
    }
    private func rating(_ title: String, _ rating: ReviewRating, key: KeyEquivalent) -> some View {
        Button(LocalizedStringKey(title)) {
            guard context?.reviewInputAvailable == true else { return }
            context?.review?.rate(rating)
        }
        .keyboardShortcut(key, modifiers: [])
        .disabled(context?.reviewInputAvailable != true || context?.review?.canRate != true)
    }
}
#endif
