import CoreBridge
import Foundation
import Observation
import UniformTypeIdentifiers

/// U162's one file intake (`package/file-intake`): every source of K1 (File ›
/// Import…, Settings › Collection's Import, a drop on a window or the Dock,
/// Open in Meguri) hands its URLs here. P18 decides the flow: a deck package
/// goes to `DeckImportModel`'s sheet, a whole collection to the collection
/// package flow and its replacement confirmation (K5), anything else is
/// refused with a message (K3 step 4). No file is read before that.
@Observable @MainActor
public final class ImportIntake {
    /// K3, K4 and K6's messages, shown as an alert on the window the file came to.
    public enum Message: Equatable, Sendable {
        /// K3 step 4: names the accepted types.
        case unsupported(fileName: String?)
        /// K3 step 3: a folder, a missing or unreadable file.
        case unreadable(fileName: String)
        /// K4: a file arrived while an import shows.
        case alreadyOpen
        /// K6: a deck package needs an open collection.
        case needsCollection
        /// K6: a collection package cannot be imported now (a review, another long task).
        case collectionImportUnavailable
        /// X2.6: `read_options` failed: the collection cannot be read now.
        case collectionUnavailable
        /// The import finished while another sheet held the window.
        case imported(fileName: String)
    }

    /// The file types every picker of K1 offers (K2.2).
    public static let pickerTypes: [UTType] = [.meguriDeckPackage, .meguriCollectionPackage, .zip]

    /// The picker this intake opened (not the collection sheet's own). The
    /// system sets it false before the choice arrives: `pickerActive` says
    /// whose picker it was until then.
    public var pickerPresented = false
    public private(set) var pickerActive = false
    /// The window of the open picker, apart from deliveries that arrive meanwhile.
    private var pickerHost: AppNavigation.SheetHost = .main
    public private(set) var message: Message?
    /// The window the picker or the file belongs to.
    public private(set) var host: AppNavigation.SheetHost = .main
    /// A file being classified: nothing else is taken meanwhile.
    public private(set) var classifying = false

    @ObservationIgnored private weak var owner: ReviewCollection?
    @ObservationIgnored private let service: any DeckPackageImportService

    init(owner: ReviewCollection, service: any DeckPackageImportService) {
        self.owner = owner; self.service = service
    }

    /// An import flow is open or starting: nothing else is admitted (K4).
    public var importOpen: Bool {
        guard let owner else { return false }
        return classifying || creating || owner.deckImport.active || owner.entry.busy || owner.entry.preview != nil
            || owner.navigation.sheet == .collectionEntry || owner.navigation.sheet == .deckImport
    }

    /// File › Import… and Settings' Import can run: a deck package could be
    /// imported (an open collection) or a collection package (the entry
    /// flow's own admission), and no sheet or import is open.
    public var canChoose: Bool {
        guard let owner else { return false }
        return (owner.state == .ready || owner.entry.canPresent) && owner.navigation.sheet == nil && !importOpen
            && !pickerPresented && !pickerActive && !owner.entry.pickerPresented
    }

    /// K1's pickers: the system file picker at once, on the window that asked (X7.1).
    public func choose(host: AppNavigation.SheetHost = .main) {
        guard canChoose else { return }
        self.host = host
        if owner?.navigation.sheet == nil { owner?.navigation.sheetHost = host }
        pickerHost = host
        pickerActive = true
        pickerPresented = true
    }

    /// The picker closed with no file.
    public func pickerCancelled() { pickerActive = false; pickerPresented = false }

    public func selection(_ result: Result<[URL], any Error>) {
        pickerPresented = false; pickerActive = false
        switch result {
        case .success(let urls): receive(urls, host: pickerHost, fromPicker: true)
        case .failure(let error):
            if (error as NSError).code != NSUserCancelledError { message = .unreadable(fileName: "") }
        }
    }

    /// K3 and K4 for the URLs a source gave. `release` ends a hold on them
    /// (a dropped file's copy) once the flow ends or they are refused.
    public func receive(_ urls: [URL], host: AppNavigation.SheetHost = .main, fromPicker: Bool = false,
                        release: (@MainActor () -> Void)? = nil) {
        guard let owner else { release?(); return }
        // A drop whose files could not be loaded still says so.
        guard !urls.isEmpty else { message = .unreadable(fileName: ""); release?(); return }
        guard !importOpen else { message = .alreadyOpen; release?(); return }
        self.host = host
        classifying = true
        Task {
            defer { classifying = false }
            // K6: a file that arrives while the collection opens waits until it settles.
            for _ in 0..<300 where owner.state == .unopened || owner.state == .loading {
                try? await Task.sleep(for: .milliseconds(100))
            }
            var chosen: (URL, PackageFileClass)?
            var unreadable: URL?
            for url in urls {
                do {
                    let kind = try await service.classify(file: url)
                    if kind != .unsupported { chosen = (url, kind); break }
                } catch {
                    unreadable = unreadable ?? url
                }
            }
            guard let (url, kind) = chosen else {
                // K4: one message when no file is a package.
                message = urls.count == 1 && unreadable != nil
                    ? .unreadable(fileName: urls[0].lastPathComponent)
                    : .unsupported(fileName: urls.count == 1 ? urls[0].lastPathComponent : nil)
                release?()
                return
            }
            route(url, kind: kind, owner: owner, others: urls.count > 1, fromPicker: fromPicker, release: release)
        }
    }

    private func route(_ url: URL, kind: PackageFileClass, owner: ReviewCollection, others: Bool,
                       fromPicker: Bool = false, release: (@MainActor () -> Void)?) {
        if owner.navigation.sheet != nil, owner.navigation.sheet != .deckImport {
            message = .alreadyOpen; release?(); return
        }
        switch kind {
        case .collectionPackage:
            // K5: the collection package flow with its own confirmation.
            guard owner.entryAllowed, owner.entry.canChoose else { message = .collectionImportUnavailable; release?(); return }
            owner.navigation.sheetHost = host
            // The flow reads the file later: a copy goes when that flow ends (K3 step 6).
            owner.entry.adoptRelease(release)
            owner.entry.selection(.success([url]))
        case .deckPackage:
            // K6.1: no collection yet. From a picker the person chose to
            // import: an empty collection is created at once. From any other
            // source Welcome asks first.
            if owner.state == .noFile, owner.acceptsImports {
                if fromPicker { createCollection(then: url, owner: owner, release: release) }
                else { askToCreate(url, release: release) }
                return
            }
            // K6: a deck package needs an open collection.
            guard owner.state == .ready, owner.hasCollection, owner.acceptsImports else {
                message = .needsCollection; release?(); return
            }
            owner.deckImportHost = host
            if !owner.deckImport.begin(url, otherFilesGiven: others, release: release) { message = .alreadyOpen }
        case .unsupported:
            message = .unsupported(fileName: url.lastPathComponent); release?()
        }
    }

    public func clearMessage() { message = nil }

    // MARK: K6.1

    /// The file Welcome asks about: create a collection and import it.
    public private(set) var pendingCreation: URL?
    @ObservationIgnored private var pendingRelease: (@MainActor () -> Void)?
    /// K6.1's creation runs: nothing else is taken until its sheet opens.
    public private(set) var creating = false

    /// A second file replaces the first (K6.1).
    private func askToCreate(_ url: URL, release: (@MainActor () -> Void)?) {
        pendingRelease?()
        pendingCreation = url
        pendingRelease = release
    }

    /// Welcome's Import: Start Empty's creation, then the sheet for the file.
    public func confirmCreation() {
        guard let url = pendingCreation, let owner else { return }
        let release = pendingRelease
        pendingCreation = nil; pendingRelease = nil
        createCollection(then: url, owner: owner, release: release)
    }

    /// Welcome's Cancel: the file is dropped and Welcome stays as it was.
    public func cancelCreation() {
        let release = pendingRelease
        pendingCreation = nil; pendingRelease = nil
        release?()
    }

    private func createCollection(then url: URL, owner: ReviewCollection, release: (@MainActor () -> Void)?) {
        // A file Welcome still asks about is replaced by this one (K6.1).
        if pendingCreation != nil { cancelCreation() }
        creating = true
        Task {
            defer { creating = false }
            await owner.startEmptyFromPress()
            // A creation failure shows its error on Welcome and drops the file.
            guard owner.state == .ready, owner.hasCollection else { release?(); return }
            owner.deckImportHost = .main
            if !owner.deckImport.begin(url, release: release) { message = .alreadyOpen }
        }
    }

    /// K5: the deck sheet's inspect found a whole collection by its content.
    func routeCollection(_ url: URL, release: (@MainActor () -> Void)?) {
        guard let owner else { release?(); return }
        route(url, kind: .collectionPackage, owner: owner, others: false, release: release)
    }

    /// The deck sheet's own message when it could not open (X2.6's `read_options` row).
    func show(_ value: Message) { message = value }
}
