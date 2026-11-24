import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
import UniformTypeIdentifiers

/// `package/file-intake` K1: a file dropped on a main window (Mac, iPad), and a
/// file opened with Meguri (Finder, the Dock, Open in Meguri), go to the one
/// intake. The window shows that it takes the drop; the intake's messages
/// (K3, K4, K6) show as an alert on it.
struct FileIntakeDrop: ViewModifier {
    let collection: ReviewCollection?
    var host: AppNavigation.SheetHost = .main
    @State private var targeted = false

    /// What a file drag offers: its URL on the Mac, a file representation on the iPad.
    private static let dropTypes: [UTType] = [.fileURL, .meguriDeckPackage, .meguriCollectionPackage, .zip]

    func body(content: Content) -> some View {
        content
            .onDrop(of: Self.dropTypes, isTargeted: $targeted) { providers in
                guard let collection, !providers.isEmpty else { return false }
                Task { @MainActor in
                    let files = await DroppedFiles.load(providers)
                    collection.intake.receive(files.urls, host: host, release: files.release)
                }
                return true
            }
            .overlay { if targeted { FileDropOutline() } }
            .alert(Text(verbatim: collection?.intake.message.map(FileIntakeText.title) ?? ""),
                   isPresented: Binding(get: { collection?.intake.message != nil },
                                        set: { if !$0 { collection?.intake.clearMessage() } })) {
                Button("OK") { collection?.intake.clearMessage() }
            } message: {
                Text(verbatim: collection?.intake.message.map(FileIntakeText.message) ?? "")
            }
    }
}

/// The window's own edge, in the accent colour, while it would take a dropped file.
struct FileDropOutline: View {
    var body: some View {
        Radius.rounded(Radius.medium)
            .strokeBorder(.tint, lineWidth: ControlMetrics.dropOutline)
            .padding(Spacing.xxSmall)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
            .accessibilityIdentifier("shell.root.dropTarget")
    }
}

/// The intake's messages (Meguri's wording, `package/transfer-strings` S4).
enum FileIntakeText {
    static func title(_ message: ImportIntake.Message) -> String {
        switch message {
        case .unsupported: "Can’t Import This File"
        case .unreadable: "Can’t Read This File"
        case .alreadyOpen: "An Import Is Already Open"
        case .needsCollection: "No Collection Is Open"
        case .collectionImportUnavailable: "Can’t Import a Collection Now"
        case .collectionUnavailable: "The Collection Isn’t Available"
        case .imported: "Import Finished"
        }
    }
    static func message(_ message: ImportIntake.Message) -> String {
        switch message {
        case .unsupported(let name?):
            "“\(name)” isn’t a file Meguri can import. Meguri imports deck packages (.apkg), collection packages (.colpkg) and .zip files."
        case .unsupported(nil):
            "None of these files can be imported. Meguri imports deck packages (.apkg), collection packages (.colpkg) and .zip files."
        case .unreadable(let name):
            name.isEmpty ? "Meguri couldn’t read the file. It may have moved, or it may be a folder."
                : "Meguri couldn’t read “\(name)”. It may have moved, or it may be a folder."
        case .alreadyOpen: "Finish or cancel the import that is open, then try again."
        case .needsCollection: "A deck package is imported into a collection. Open or create a collection first."
        case .collectionImportUnavailable:
            "A collection file can’t be imported while a review or another long task is running. Try again once it ends."
        case .collectionUnavailable: "Meguri can’t read the collection right now, so the import can’t start. Try again."
        case .imported(let name): "“\(name)” was imported."
        }
    }
}

/// The URLs of a drop. On the iPad a file that is not opened in place is a
/// temporary copy, kept here until its flow ends (K3 step 6).
@MainActor enum DroppedFiles {
    struct Loaded {
        var urls: [URL] = []
        var copies: [URL] = []
        var release: @MainActor () -> Void {
            let copies = copies
            return { for copy in copies { try? FileManager.default.removeItem(at: copy.deletingLastPathComponent()) } }
        }
    }

    static func load(_ providers: [NSItemProvider]) async -> Loaded {
        var loaded = Loaded()
        for provider in providers {
            if let url = await fileURL(provider) { loaded.urls.append(url); continue }
            if let (url, copy) = await fileRepresentation(provider) {
                loaded.urls.append(url)
                if copy { loaded.copies.append(url) }
            }
        }
        return loaded
    }

    private static func fileURL(_ provider: NSItemProvider) async -> URL? {
        guard provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) else { return nil }
        return await withCheckedContinuation { continuation in
            _ = provider.loadObject(ofClass: URL.self) { url, _ in continuation.resume(returning: url?.isFileURL == true ? url : nil) }
        }
    }

    /// A file representation, in place when the source allows it; otherwise
    /// a copy in a folder of its own, removed when the flow ends.
    private static func fileRepresentation(_ provider: NSItemProvider) async -> (URL, Bool)? {
        guard let type = provider.registeredTypeIdentifiers.first(where: {
            UTType($0)?.conforms(to: .data) == true || UTType($0)?.conforms(to: .package) == true
        }) else { return nil }
        let name = provider.suggestedName
        return await withCheckedContinuation { continuation in
            _ = provider.loadInPlaceFileRepresentation(forTypeIdentifier: type) { url, inPlace, _ in
                guard let url else { continuation.resume(returning: nil); return }
                if inPlace { continuation.resume(returning: (url, false)); return }
                // The system removes its copy when this returns.
                let folder = FileManager.default.temporaryDirectory.appending(path: "Drops/\(UUID().uuidString)", directoryHint: .isDirectory)
                // The name the source gave, with the copy's extension when it has none.
                // Only a name, never a path: the copy stays in its own folder.
                let suggested = name.map { ($0 as NSString).lastPathComponent }
                var base = suggested.flatMap { $0.isEmpty || $0 == ".." || $0 == "." ? nil : $0 } ?? url.lastPathComponent
                if (base as NSString).pathExtension.isEmpty, !url.pathExtension.isEmpty { base += "." + url.pathExtension }
                let file = folder.appending(path: base)
                do {
                    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                    // A provider's file is read under a coordinated read (K3 step 2).
                    var coordination: NSError?, copyError: (any Error)?
                    NSFileCoordinator().coordinate(readingItemAt: url, options: [], error: &coordination) { readable in
                        do { try FileManager.default.copyItem(at: readable, to: file) } catch { copyError = error }
                    }
                    if let failure = coordination ?? copyError { throw failure }
                    continuation.resume(returning: (file, true))
                } catch {
                    try? FileManager.default.removeItem(at: folder)
                    continuation.resume(returning: nil)
                }
            }
        }
    }
}

/// K3 step 6: a file the system delivered as a copy for Meguri (inside
/// `Documents/Inbox`) is removed once its flow ends or it is refused.
enum FileIntakeInbox {
    static func release(_ url: URL) -> (@MainActor () -> Void)? {
        #if os(iOS)
        guard let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first else { return nil }
        let inbox = documents.appending(path: "Inbox", directoryHint: .isDirectory).standardizedFileURL.path
        guard url.standardizedFileURL.path.hasPrefix(inbox + "/") else { return nil }
        return { try? FileManager.default.removeItem(at: url) }
        #else
        return nil
        #endif
    }
}
