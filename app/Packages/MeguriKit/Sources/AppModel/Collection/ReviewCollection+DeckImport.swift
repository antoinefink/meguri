import CoreBridge
import Foundation

/// The deck package import sheet's place in the app (`package/transfer-sheets`
/// X2, `package/file-intake` K1 and K6).
extension ReviewCollection: DeckImportHost {
    public var deckImportSheetShown: Bool { navigation.sheet == .deckImport }
    /// No new import starts once the collection begins to close.
    var acceptsImports: Bool { !isClosingForImports }

    public func presentDeckImport() {
        guard navigation.sheet != .deckImport else { return }
        // A sheet of another flow took the window meanwhile: the import waits behind nothing.
        guard navigation.sheet == nil else { return }
        let host = deckImportHost
        navigation.present(.deckImport)
        navigation.sheetHost = host
    }

    public func dismissDeckImport() {
        if navigation.sheet == .deckImport { navigation.dismissSheet() }
    }

    public func deckImportNotetypeNames() async -> [Int64: String] {
        guard let list = try? await CoreNotetypeService().notetypeList() else { return [:] }
        return Dictionary(list.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first })
    }

    public func deckImportCanUndo(step: UInt64) async -> Bool {
        // A review's undo is session-aware: Undo Import waits for it to end.
        guard review == nil, navigation.review == nil,
              case .available(let top)? = try? await collectionUndo.service.peekUndo(scope: .global) else { return false }
        return top.id == step
    }

    /// The core undoes the step only while it is still the newest one (its
    /// writer compares the step as it takes the op), so a write that lands
    /// after the sheet looked is never undone in its place.
    public func undoDeckImport(step: UInt64) async -> Bool {
        let service = collectionUndo.service
        let undone: Bool
        do {
            guard case .global(_, let restored) = try await service.startUndo(step: step).value() else { return false }
            undone = restored.id == step
        } catch {
            return false
        }
        collectionUndo.scheduleRefresh()
        await collectionUndoFinished(intent: navigation.intentRevision)
        return undone
    }

    public var deckImportShowInBrowse: (([Int64]) -> Void)? {
        guard deckImportBrowseAvailable else { return nil }
        return { [weak self] ids in
            guard let navigation = self?.navigation else { return }
            // X2.5: Browse with exactly these notes, from its root and its top.
            navigation.endReview()
            navigation.browseSearch = "nid:" + ids.map(String.init).joined(separator: ",")
            navigation.setPath([], for: .browse)
            navigation.scrollToTop(.section(.browse))
            navigation.show(.browse)
        }
    }

    public var deckImportSync: (any DeckImportMeguriSync)? {
        sync.providerState?.selection == .meguri ? sync : nil
    }

    public func chooseAnotherImportFile() {
        // After the sheet's dismissal settles, on the window it was on.
        let host = deckImportHost
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(400))
            self?.intake.choose(host: host)
        }
    }

    public func deckImportMessage(_ message: ImportIntake.Message) {
        intake.show(message)
    }

    public func deckImportFoundCollection(_ url: URL, release: (@MainActor () -> Void)?) {
        // After the deck sheet's dismissal settles.
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(400))
            self?.intake.routeCollection(url, release: release)
        }
    }
}
