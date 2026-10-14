import SwiftUI

/// The five states a screen or component can show (R1970). A screen's
/// `docs/design/screen-inventory.yaml` entry lists the ones it has, drawn
/// from these.
nonisolated public enum PreviewState: String, CaseIterable, Sendable, Codable {
    case empty, typical, overflow, error, loading
}

/// One state's fixture: the view a screen shows in that state, with fixed
/// data and no core calls.
public struct PreviewFixture: Identifiable {
    public let state: PreviewState
    private let make: () -> AnyView

    public init(_ state: PreviewState, @ViewBuilder _ content: @escaping () -> some View) {
        self.state = state
        make = { AnyView(content()) }
    }

    public var id: PreviewState { state }

    public func makeView() -> AnyView { make() }
}

/// A screen or component that registers its states (R1970). The same
/// fixtures feed its `#Preview`s and its snapshots (U10:
/// `MeguriSnapshotTests/Support/MatrixRegistry.swift` lists every
/// conformer), so a preview and a snapshot never show different data.
///
/// ```swift
/// extension DeckList: PreviewStates {
///     static let previewID = "home.deckList"
///     static var previewFixtures: [PreviewFixture] {
///         [PreviewFixture(.typical) { DeckList(model: .sample) }, ...]
///     }
/// }
/// #Preview("Typical") { DeckList.preview(.typical) }
/// ```
///
/// `StateCoverageTests` fails when a registered screen lacks a state its
/// inventory entry lists.
public protocol PreviewStates {
    /// The inventory id (`area.screen`, also the prefix of the screen's
    /// accessibility identifiers), e.g. `home.deckList`.
    static var previewID: String { get }
    /// One fixture per state the screen has, in any order.
    static var previewFixtures: [PreviewFixture] { get }
}

extension PreviewStates {
    /// The states that have a fixture, in `PreviewState` order.
    public static var previewStates: [PreviewState] {
        let states = Set(previewFixtures.map(\.state))
        return PreviewState.allCases.filter(states.contains)
    }

    /// The fixture for `state`, or nil when the screen has no such state.
    public static func fixture(_ state: PreviewState) -> PreviewFixture? {
        previewFixtures.first { $0.state == state }
    }

    /// The view for `state`, for `#Preview`s. A state the screen lacks shows
    /// a line saying so, so a stale preview is visible rather than blank.
    public static func preview(_ state: PreviewState) -> AnyView {
        fixture(state)?.makeView()
            ?? AnyView(Text(verbatim: "\(previewID) has no \(state.rawValue) state"))
    }
}
