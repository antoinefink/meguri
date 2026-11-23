import Foundation
import SwiftUI

/// A deck's id in the collection (the `id` of Anki's deck records).
nonisolated public struct DeckID: RawRepresentable, Hashable, Codable, Sendable, CustomStringConvertible {
    public let rawValue: Int64

    public init(rawValue: Int64) {
        self.rawValue = rawValue
    }

    public init(_ rawValue: Int64) {
        self.rawValue = rawValue
    }

    public var description: String { String(rawValue) }
}

/// The app's top-level sections (R17, R18). The sidebar layout lists them
/// all; the tab layout has a tab for each but Note Types, which it shows
/// inside the Settings tab.
nonisolated public enum AppSection: String, CaseIterable, Codable, Sendable {
    case home, browse, stats, noteTypes, settings

    /// The sidebar's section group, top to bottom (R17; N25.1: Statistics second, under Today).
    public static let sidebar: [AppSection] = [.home, .stats, .browse, .noteTypes, .settings]

    /// Whether the Statistics section is in the product on a device family (N25.1, plan U307
    /// for the Mac and the iPad): its page is built for the Mac and the iPad at regular width
    /// (U296 to U301); the iPhone, the Duo and the compact layouts wait for U304.
    public static func statisticsShown(on family: DeviceFamily) -> Bool { family != .phone }

    /// The product sidebar's section rows on a device family, top to bottom (N21.1, N22.1,
    /// N25.1): Today, Statistics when it is shown, then Browse and Note Types on the Mac, Note
    /// Types and Settings elsewhere (the iPad's Browse row waits for its own layout; the Mac's
    /// Settings is its own window).
    public static func productSidebar(on family: DeviceFamily, statistics: Bool) -> [AppSection] {
        let rest: [AppSection] = family == .mac ? [.browse, .noteTypes] : [.noteTypes, .settings]
        return [.home] + (statistics ? [.stats] : []) + rest
    }

    /// The tab that shows this section in the tab layout.
    public var tab: CompactTab {
        switch self {
        case .home: .decks
        case .browse: .browse
        case .stats: .stats
        case .noteTypes, .settings: .settings
        }
    }
}

/// The kind of device the app runs on, for the sections whose pages are not built for every
/// layout yet.
nonisolated public enum DeviceFamily: Sendable {
    case mac, pad, phone
}

/// The tab layout's tabs, in tab bar order (R18).
nonisolated public enum CompactTab: String, CaseIterable, Codable, Sendable {
    case decks, browse, stats, settings

    /// The section at the tab's root.
    public var section: AppSection {
        switch self {
        case .decks: .home
        case .browse: .browse
        case .stats: .stats
        case .settings: .settings
        }
    }
}

/// A screen pushed onto a section's navigation stack. Units add cases for
/// the screens they push.
nonisolated public enum Destination: Hashable, Codable, Sendable {
    /// A deck's page (U155). The sidebar layout shows it as the Home
    /// section's root when the deck is selected; the tab layout pushes it on
    /// the Decks tab.
    case deckPage(DeckID)
    /// The deck's Deck Options page (N20.9), pushed on its deck page.
    case deckOptions(DeckID)
    /// Note Types, pushed from Settings or from Browse's ⋯ menu in the tab
    /// layout (R18).
    case noteTypes
    /// iPad: one note type's page, pushed over the Note Types list (N22.7).
    case noteType(Int64)
    case settingsPage(SettingsPage)
}

/// What the sidebar has selected (R17): a section row or a deck row.
nonisolated public enum SidebarItem: Hashable, Sendable {
    case section(AppSection)
    case deck(DeckID)
}

/// A scroll position the navigation keeps across layout switches (R5): a
/// section's root screen, or the deck page.
nonisolated public enum ScrollAnchorKey: Hashable, Sendable {
    case section(AppSection)
    case deckPage
}

/// Which root layout shows the navigation (R2): the tab layout at compact
/// width, the sidebar layout at regular width and on the Mac.
nonisolated public enum ShellLayout: Sendable {
    case compact, sidebar
}

extension EnvironmentValues {
    /// The live presenting shell layout, supplied by RootView independently of sheet traits.
    @Entry public var shellLayout: ShellLayout = .sidebar
    /// The tab layout's deck list while Today shows beside it (an unfolded iPhone held wide, N14.1):
    /// what Today already shows is not repeated in the list.
    @Entry public var todayBesideDeckList = false
    /// Test hooks force a layout through the size class (`-MeguriSizeClass`), on iPhone too.
    @Entry public var layoutFollowsForcedSizeClass = false
    /// N23.6: the sync status button, from the tab layout's Decks stack. A screen of
    /// that stack puts it first in its own trailing group (search, Add), so the
    /// system keeps the three in one group wherever it draws them.
    @Entry public var syncStatusItem: AnyView? = nil
}
