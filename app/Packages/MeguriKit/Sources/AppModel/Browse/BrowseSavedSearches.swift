import CoreBridge
import Foundation

/// Rename… asking for a user saved search's new name (T13, K7.2).
public nonisolated struct RenamingSearch: Identifiable, Equatable, Sendable {
    public let id: UInt64
    /// The saved search renamed.
    public let old: String
    /// The name typed is another saved search's: Replace asks first.
    public var replacing: String?
}

/// The sidebar's saved-search commands (`browser/workspace` V6, T13): Save
/// Search… on the section, Rename… and Delete on a user's saved search,
/// each one undoable step. The built-in saved searches have neither.
extension BrowseModel {
    /// Save Search… from the Saved Searches section: as the suggestions'
    /// row, only while the query is not empty.
    public var canSaveSearch: Bool { !query.isEmpty }

    /// Rename… on a user saved search: asks for the new name.
    public func startRenaming(_ name: String) {
        guard savedSearches.contains(where: { $0.name == name }) else { return }
        savingPresentations &+= 1
        renamingSearch = RenamingSearch(id: savingPresentations, old: name)
    }

    public func cancelRenaming() { renamingSearch = nil }

    /// The new name given. K7.2 drops every `"`; an empty name, or the same
    /// one, changes nothing; another saved search's name asks first.
    public func rename(to typed: String) {
        guard let renaming = renamingSearch else { return }
        let name = typed.replacingOccurrences(of: "\"", with: "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name != renaming.old else {
            renamingSearch = nil
            return
        }
        if savedSearches.contains(where: { $0.name == name }), renaming.replacing != name {
            renamingSearch?.replacing = name
            return
        }
        renamingSearch = nil
        let backend = backend
        Task { try? await backend.renameSavedSearch(renaming.old, to: name) }
    }

    /// The replace question's Cancel: back to the name.
    public func keepRenaming() { renamingSearch?.replacing = nil }

    /// Delete on a user saved search: no question; Undo restores it.
    public func deleteSavedSearch(_ name: String) {
        guard savedSearches.contains(where: { $0.name == name }) else { return }
        let backend = backend
        Task { try? await backend.deleteSavedSearch(name) }
    }

    /// Edit Note Type… on a note type row (W14).
    public func editNoteType(_ id: Int64) {
        commands.openNoteType(id)
    }
}
