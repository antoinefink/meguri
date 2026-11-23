import AppModel
import CoreBridge
import Foundation

/// The section's formatted strings (`notetypes/workspace` W15,
/// `notetypes/draft-save` D9), from its String Catalog.
public enum NoteTypesStrings {
    static var bundle: Bundle { .module }

    static func text(_ key: String.LocalizationValue, locale: Locale = .current) -> String {
        String(localized: key, bundle: .module, locale: locale)
    }

    /// The detail's subtitle: "412 notes · 786 cards".
    public static func subtitle(notes: UInt64, cards: UInt64, locale: Locale) -> String {
        let notesText = notes.formatted(.number.locale(locale)), cardsText = cards.formatted(.number.locale(locale))
        let notes = Int(clamping: notes), cards = Int(clamping: cards)
        return String(localized: "noteTypes.detail.subtitle", defaultValue: "\(notes) \(notesText) \(cards) \(cardsText)",
                      bundle: .module, locale: locale)
    }

    /// D5 step 5's progress: "Updating 212 of 412 notes".
    public static func progress(completed: UInt64, total: UInt64, locale: Locale) -> String {
        let completedText = completed.formatted(.number.locale(locale)), totalText = total.formatted(.number.locale(locale))
        let total = Int(clamping: total)
        return String(localized: "noteTypes.progress", defaultValue: "\(completedText) \(total) \(totalText)", bundle: .module, locale: locale)
    }

    public static func saveTitle(_ name: String, locale: Locale) -> String {
        String(localized: "noteTypes.save.title", defaultValue: "\(name)", bundle: .module, locale: locale)
    }

    /// D4's line and detail lines of a row.
    public static func line(_ row: NoteTypeSaveRow, locale: Locale) -> (title: String, details: [String]) {
        func cardsDeleted(_ n: UInt64) -> String {
            let text = n.formatted(.number.locale(locale)), n = Int(clamping: n)
            return String(localized: "noteTypes.save.cardsDeleted", defaultValue: "\(n) \(text)", bundle: .module, locale: locale)
        }
        func notesWithoutCards(_ n: UInt64) -> String {
            let text = n.formatted(.number.locale(locale)), n = Int(clamping: n)
            return String(localized: "noteTypes.save.notesWithoutCards", defaultValue: "\(n) \(text)", bundle: .module, locale: locale)
        }
        func contentDeleted(_ n: UInt64) -> String {
            let text = n.formatted(.number.locale(locale)), n = Int(clamping: n)
            return String(localized: "noteTypes.save.contentDeleted", defaultValue: "\(n) \(text)", bundle: .module, locale: locale)
        }
        func cardsCreated(_ n: UInt64) -> String {
            let text = n.formatted(.number.locale(locale)), n = Int(clamping: n)
            return String(localized: "noteTypes.save.cardsCreated", defaultValue: "\(n) \(text)", bundle: .module, locale: locale)
        }
        switch row.kind {
        case .deleteCardType(let name, let cards, let notes):
            let title = String(localized: "noteTypes.save.deleteCardType", defaultValue: "\(name)", bundle: .module, locale: locale)
            guard cards > 0 else { return (title, [text("noteTypes.save.noCards", locale: locale)]) }
            return (title, [cardsDeleted(cards)] + (notes > 0 ? [notesWithoutCards(notes)] : []))
        case .deleteField(let name, let notes):
            let title = String(localized: "noteTypes.save.deleteField", defaultValue: "\(name)", bundle: .module, locale: locale)
            return (title, [notes > 0 ? contentDeleted(notes) : text("noteTypes.save.empty", locale: locale)])
        case .addCardType(let name, let cards):
            let title = String(localized: "noteTypes.save.addCardType", defaultValue: "\(name)", bundle: .module, locale: locale)
            return (title, cards > 0 ? [cardsCreated(cards)] : [])
        case .addField(let name):
            return (String(localized: "noteTypes.save.addField", defaultValue: "\(name)", bundle: .module, locale: locale), [])
        case .reorderFields:
            return (text("noteTypes.save.reorderFields", locale: locale), [])
        case .reorderCardTypes:
            return (text("noteTypes.save.reorderCardTypes", locale: locale), [])
        case .sortBy(let name):
            return (String(localized: "noteTypes.save.sortBy", defaultValue: "\(name)", bundle: .module, locale: locale), [])
        }
    }

    /// D4's paragraph with Meguri Sync, the device named as "Mac", "iPad" or "iPhone".
    public static func syncBody(device: String, locale: Locale) -> String {
        String(localized: "noteTypes.save.sync.body", defaultValue: "\(device)", bundle: .module, locale: locale)
    }

    /// D4's first paragraph when Meguri Sync cannot be reached, after its bold title.
    public static func unreachableBody(device: String, locale: Locale) -> String {
        String(localized: "noteTypes.save.unreachable.body", defaultValue: "\(device)", bundle: .module, locale: locale)
    }

    /// The alert after Save and Upload saved on this device only (D6 step 4).
    public static func notUploaded(device: String, locale: Locale) -> String {
        String(localized: "noteTypes.notUploaded.message", defaultValue: "\(device)", bundle: .module, locale: locale)
    }

    public static func backup(device: String, locale: Locale) -> String {
        String(localized: "noteTypes.save.backup", defaultValue: "\(device)", bundle: .module, locale: locale)
    }

    /// This device as D4 names it.
    public static func device(locale: Locale) -> String {
        #if os(macOS)
        text("noteTypes.device.mac", locale: locale)
        #else
        ProcessInfo.processInfo.isiOSAppOnMac || UIDeviceIdiom.isPad
            ? text("noteTypes.device.ipad", locale: locale) : text("noteTypes.device.iphone", locale: locale)
        #endif
    }

    public static func refusal(_ refusal: NotetypeFieldNameRefusal, locale: Locale) -> String {
        switch refusal {
        case .badFirstCharacter: text("noteTypes.fields.refusal.badFirstCharacter", locale: locale)
        case .badCharacter: text("noteTypes.fields.refusal.badCharacter", locale: locale)
        case .duplicate: text("noteTypes.fields.refusal.duplicate", locale: locale)
        }
    }

    public static func deleteBlock(_ block: NoteTypeFieldDeleteBlock, locale: Locale) -> String {
        switch block {
        case .lastField: text("noteTypes.fields.blocked.last", locale: locale)
        case .protected: text("noteTypes.fields.blocked.protected", locale: locale)
        }
    }

    // MARK: The Cards tab (W5, W7, W15)

    public static func part(_ part: NoteTypeTemplatePart, locale: Locale) -> String {
        switch part {
        case .front: text("noteTypes.editor.front", locale: locale)
        case .back: text("noteTypes.editor.back", locale: locale)
        case .styling: text("noteTypes.editor.styling", locale: locale)
        }
    }

    public static func cardTypeBlock(_ block: NoteTypeCardTypeBlock, locale: Locale) -> String {
        switch block {
        case .lastCardType: text("noteTypes.cards.blocked.last", locale: locale)
        case .cloze: text("noteTypes.cards.blocked.cloze", locale: locale)
        }
    }

    /// W7's line: where ("Recognition · Front, line 3."), what, and for a
    /// yellow issue the reason when one field decides it.
    public static func issue(_ issue: NoteTypeLiveIssue, locale: Locale) -> (location: String, message: String, reason: String?) {
        let card = issue.cardName
        if case .blankFront(let notes, let field) = issue.kind {
            let countText = notes.formatted(.number.locale(locale))
            let line = String(localized: "noteTypes.issues.blank", defaultValue: "\(card) \(notes) \(countText)", bundle: .module, locale: locale)
            let reason = field.map { String(localized: "noteTypes.issues.blankReason", defaultValue: "\($0)", bundle: .module, locale: locale) }
            return (line, "", reason)
        }
        let location: String
        if let part = issue.part {
            let partName = Self.part(part, locale: locale)
            if let line = issue.line {
                let lineText = line.formatted(.number.locale(locale))
                location = String(localized: "noteTypes.issues.where.line", defaultValue: "\(card) \(partName) \(lineText)", bundle: .module, locale: locale)
            } else {
                location = String(localized: "noteTypes.issues.where.part", defaultValue: "\(card) \(partName)", bundle: .module, locale: locale)
            }
        } else {
            location = card
        }
        let message: String = switch issue.kind {
        case .unknownField(let name): String(localized: "noteTypes.issues.unknownField", defaultValue: "\(name)", bundle: .module, locale: locale)
        case .unclosedSection(let tag): String(localized: "noteTypes.issues.unclosed", defaultValue: "\(tag)", bundle: .module, locale: locale)
        case .unopenedSection(let tag): String(localized: "noteTypes.issues.unopened", defaultValue: "\(tag)", bundle: .module, locale: locale)
        case .mismatchedSection(let tag, let open):
            String(localized: "noteTypes.issues.mismatched", defaultValue: "\(tag) \(open)", bundle: .module, locale: locale)
        case .unmatchedSection: text("noteTypes.issues.unmatched", locale: locale)
        case .duplicateFront(let earlier): String(localized: "noteTypes.issues.duplicateFront", defaultValue: "\(earlier)", bundle: .module, locale: locale)
        case .noFrontField: text("noteTypes.issues.noFrontField", locale: locale)
        case .missingCloze: text("noteTypes.issues.missingCloze", locale: locale)
        case .blankFront: ""
        }
        return (location, message, nil)
    }

    /// A fix's button title (W7).
    public static func fix(_ fix: NoteTypeLiveIssue.Fix, locale: Locale) -> String {
        switch fix {
        case .useField(_, let to): String(localized: "noteTypes.issues.use", defaultValue: "\(to)", bundle: .module, locale: locale)
        case .addClose(let close): String(localized: "noteTypes.issues.addClose", defaultValue: "\(close)", bundle: .module, locale: locale)
        case .showNotes: text("noteTypes.issues.showNotes", locale: locale)
        }
    }

    /// W7's bottom line while red issues hold Save.
    public static func fixToSave(_ count: Int, locale: Locale) -> String {
        let countText = count.formatted(.number.locale(locale))
        return String(localized: "noteTypes.issues.fixToSave", defaultValue: "\(count) \(countText)", bundle: .module, locale: locale)
    }

    public static func sortBy(_ name: String, locale: Locale) -> String {
        String(localized: "noteTypes.fields.sortBy", defaultValue: "\(name)", bundle: .module, locale: locale)
    }

    /// W9's note menu: "3 of 12".
    public static func previewPosition(_ index: Int, of count: Int, locale: Locale) -> String {
        String(localized: "noteTypes.preview.position", defaultValue: "\(index) of \(count)", bundle: .module, locale: locale)
    }

    /// W9's Cloze Number menu: "Cloze 2".
    public static func previewCloze(_ number: Int, locale: Locale) -> String {
        String(localized: "noteTypes.preview.cloze", defaultValue: "Cloze \(number)", bundle: .module, locale: locale)
    }
}

#if os(iOS)
import UIKit
enum UIDeviceIdiom {
    @MainActor static var isPad: Bool { UIDevice.current.userInterfaceIdiom == .pad }
}
#endif
