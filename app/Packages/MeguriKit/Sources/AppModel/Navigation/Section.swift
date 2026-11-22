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

    /// The sidebar's section group, top to bottom (R17).
    public static let sidebar: [AppSection] = [.home, .browse, .stats, .noteTypes, .settings]

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
}
