import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

public struct HomeView: View {
    public let collection: ReviewCollection
    public let deckID: Int64?
    public init(collection: ReviewCollection, deckID: Int64? = nil) { self.collection = collection; self.deckID = deckID }
    public var body: some View {
        Group {
            #if os(macOS)
            GeometryReader { geometry in
                scrollContent(availableHeight: geometry.size.height)
            }
            #else
            scrollContent()
            #endif
        }
        .navigationTitle(deckID == nil ? String(localized: "Decks", bundle: .module) : "")
        .accessibilityIdentifier("home.collection.screen")
    }
    private var centersContent: Bool {
        #if os(macOS)
        if case .ready = collection.state {
            return collection.decks.isEmpty || deckID.flatMap { collection.deck($0) } != nil
        }
        if case .noFile = collection.state { return true }
        #endif
        return false
    }
    private func scrollContent(availableHeight: CGFloat = 0) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Spacing.large) {
                if let record = collection.previousRecord, !record.answers.isEmpty {
                    VStack(alignment: .leading, spacing: Spacing.xSmall) {
                        Text("Previous Session", bundle: .module).font(.headline)
                        Text("\(record.answers.count) answers recorded", bundle: .module)
                            .accessibilityIdentifier("home.session.recorded")
                    }
                }
                if let error = collection.error, !hasFailed {
                    Text(verbatim: error).foregroundStyle(.token(.statusError))
                        .accessibilityIdentifier("home.collection.error")
                }
                content
            }
            .padding(Spacing.medium)
            .frame(maxWidth: ContentWidth.readable, alignment: .leading)
            .frame(maxWidth: .infinity)
            .frame(minHeight: centersContent ? availableHeight : 0, alignment: centersContent ? .center : .top)
            .scrollTargetLayout()
        }
        .navigationScrollPosition(collection.navigation, .section(.home), idType: Int64.self)
    }
    private var hasFailed: Bool {
        if case .failed = collection.state { return true }
        return false
    }
    @ViewBuilder private var content: some View {
        switch collection.state {
        case .unopened, .loading: LoadingStateView(Text("Opening collection…", bundle: .module))
        case .noFile:
            HomeCollectionEmptyContent(canPresent: collection.entry.canPresent,
                importAction: { collection.entry.present() })
        case .unavailable:
            LoadingStateView(Text("Collection is busy…", bundle: .module))
        case .failed(let message):
            ErrorStateView(Text("Couldn’t Open Collection", bundle: .module), message: Text(verbatim: message),
                retryIdentifier: "home.collection.retry", retry: { Task { await collection.start() } })
        case .ready:
            if let deckID, let deck = collection.deck(deckID) {
                VStack(spacing: Spacing.large) {
                    Text(verbatim: deck.name)
                        .font(.largeTitle.bold())
                        .multilineTextAlignment(.center)
                        .fixedSize(horizontal: false, vertical: true)
                    #if os(macOS)
                    HStack(spacing: Spacing.large) {
                        VStack(spacing: Spacing.xSmall) {
                            Text(deck.counts.new, format: .number)
                                .font(.title2)
                                .foregroundStyle(.token(deck.counts.new == 0 ? .countZero : .countNew))
                            Text("New", bundle: .module).font(.subheadline)
                        }
                        .accessibilityElement(children: .combine)
                        VStack(spacing: Spacing.xSmall) {
                            Text(deck.counts.learning, format: .number)
                                .font(.title2)
                                .foregroundStyle(.token(deck.counts.learning == 0 ? .countZero : .countLearn))
                            Text("Learning", bundle: .module).font(.subheadline)
                        }
                        .accessibilityElement(children: .combine)
                        VStack(spacing: Spacing.xSmall) {
                            Text(deck.counts.review, format: .number)
                                .font(.title2)
                                .foregroundStyle(.token(deck.counts.review == 0 ? .countZero : .countDue))
                            Text("Due", bundle: .module).font(.subheadline)
                        }
                        .accessibilityElement(children: .combine)
                    }
                    .accessibilityIdentifier("home.deck.counts")
                    #else
                    DeckCounts(counts: deck.counts)
                        .accessibilityIdentifier("home.deck.counts")
                    #endif
                    Button { Task { await collection.startStudy(deckID: deck.id) } } label: {
                        Label {
                            Text("Study", bundle: .module)
                                .fixedSize(horizontal: false, vertical: true)
                        } icon: { Image(systemName: "play.fill") }
                            #if !os(macOS)
                            .font(.headline)
                            .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget)
                            #endif
                    }
                    .buttonStyle(.borderedProminent)
                    #if os(macOS)
                    .controlSize(.regular)
                    .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
                    .contentShape(Rectangle())
                    #else
                    .controlSize(.large)
                    .frame(maxWidth: ContentWidth.primaryAction)
                    #endif
                    .disabled(!collection.canStudy)
                    .accessibilityIdentifier("home.deck.study")
                }
                .frame(maxWidth: .infinity)
                #if !os(macOS)
                .padding(.vertical, Spacing.xxLarge)
                #endif
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("home.deck.page.\(deck.id)")
            } else if collection.decks.isEmpty {
                EmptyStateView(Text("No Decks", bundle: .module), systemImage: "rectangle.stack",
                    description: Text("This collection has no decks to study.", bundle: .module))
            } else {
                HomeDeckTree(nodes: collection.decks) { id in collection.navigation.openDeckPage(DeckID(id)) }
            }
        }
    }
}

struct HomeCollectionEmptyContent: View {
    var canPresent: Bool = true
    var importAction: (() -> Void)? = nil

    var body: some View {
        EmptyStateView(Text("No Collection", bundle: .module), systemImage: "rectangle.stack",
            description: Text("Import a collection to start studying.", bundle: .module)) {
            if let importAction {
                HomeCollectionImportButton(canPresent: canPresent, action: importAction)
            }
        }
    }
}

struct HomeCollectionImportButton: View {
    let canPresent: Bool
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            Text("Import Collection")
        }
        .buttonStyle(.bordered)
        #if os(macOS)
        .controlSize(.regular)
        // EmptyStateView supplies a text-primary tint for its touch controls.
        .tint(nil)
        .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
        .contentShape(Rectangle())
        #else
        .tint(ColorToken.textPrimary.color)
        .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
        .contentShape(Rectangle())
        #endif
        .disabled(!canPresent)
        .accessibilityIdentifier("home.collection.import")
    }
}

public struct HomeDeckTree: View {
    public let nodes: [CountedDeckNode]
    public let select: (Int64) -> Void
    public init(nodes: [CountedDeckNode], select: @escaping (Int64) -> Void) { self.nodes = nodes; self.select = select }
    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.small) {
            ForEach(nodes, id: \.id) { node in
                Button { select(node.id) } label: {
                    HStack(spacing: Spacing.small) {
                        Image(systemName: node.filtered ? "line.3.horizontal.decrease.circle" : "rectangle.stack")
                            .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: Spacing.xSmall) {
                            Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)
                                .font(.headline).fixedSize(horizontal: false, vertical: true)
                                .foregroundStyle(.token(.textPrimary))
                                .accessibilityIdentifier("home.deck.name.\(node.id)")
                            DeckCounts(counts: node.counts)
                                .accessibilityIdentifier("home.deck.rowCounts.\(node.id)")
                        }
                        Spacer(minLength: 0)
                        Image(systemName: "chevron.forward").accessibilityHidden(true)
                    }
                    .padding(Spacing.small)
                    .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                    .background(.token(.surface), in: .rect(cornerRadius: Radius.small))
                }
                .buttonStyle(.plain)
                .id(node.id)
                .accessibilityIdentifier("home.deck.row.\(node.id)")
                if !node.children.isEmpty {
                    AnyView(HomeDeckTree(nodes: node.children, select: select))
                        .padding(.leading, Spacing.large)
                }
            }
        }
    }
}

public struct DeckCounts: View {
    public let counts: ReviewCounts
    public init(counts: ReviewCounts) { self.counts = counts }
    public var body: some View {
        Text("\(Text(counts.new, format: .number).foregroundStyle(.token(.countNew))) new · \(Text(counts.learning, format: .number).foregroundStyle(.token(.countLearn))) learning · \(Text(counts.review, format: .number).foregroundStyle(.token(.countDue))) due", bundle: .module)
            .font(.subheadline)
    }
}

public struct HomeSidebarDecks: View {
    public let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    public var body: some View {
        if collection.decks.isEmpty {
            emptyGuidance
                .font(.subheadline)
                .foregroundStyle(.token(.textSecondary))
                .lineLimit(nil)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityIdentifier("home.sidebar.guidance")
        } else {
            ForEach(Self.flatten(collection.decks), id: \.id) { deck in
                Group {
                    #if os(macOS)
                    HStack(spacing: Spacing.small) {
                        Text(verbatim: deck.path)
                        Spacer(minLength: 0)
                        if let node = collection.deck(deck.id) {
                            Text("\(Text(node.counts.review, format: .number)) due", bundle: .module)
                                .font(.subheadline)
                                .foregroundStyle(.token(node.counts.review == 0 ? .countZero : .textSecondary))
                        }
                    }
                    #else
                    VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                        Text(verbatim: deck.path)
                        if let node = collection.deck(deck.id) { DeckCounts(counts: node.counts) }
                    }
                    #endif
                }
                .foregroundStyle(.token(.textPrimary))
                .lineLimit(nil)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityIdentifier("home.sidebar.deck.\(deck.id)")
                .listRowBackground(ColorToken.surface.color)
                .tag(SidebarItem.deck(DeckID(deck.id)))
            }
        }
    }

    private var emptyGuidance: Text { Text("No decks", bundle: .module) }

    private struct Row {
        let id: Int64
        let path: String
    }

    private static func flatten(_ nodes: [CountedDeckNode], ancestors: [String] = []) -> [Row] {
        nodes.flatMap { node in
            let components = ancestors + [node.name]
            return [Row(id: node.id, path: components.joined(separator: "::"))]
                + flatten(node.children, ancestors: components)
        }
    }
}

public struct FsrsAdmissionView: View {
    public let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.large) {
            Label { Text("Study with FSRS", bundle: .module) } icon: { Image(systemName: "brain") }
                .font(.title2.bold())
            Text(collection.policy == .needsReEnable
                 ? String(localized: "Another device turned FSRS off. Meguri will turn it on again for this collection.", bundle: .module)
                 : String(localized: "Meguri schedules with FSRS. Enabling it computes memory states from your review history without changing due dates or intervals. The change reaches your other devices when you sync.", bundle: .module))
            if let error = collection.error {
                Text(verbatim: error).foregroundStyle(.token(.statusError))
                    .accessibilityIdentifier("home.policy.error")
            }
            if collection.enabling {
                if let progress = collection.progress, let total = progress.total {
                    ProgressView(value: Double(progress.completed), total: Double(max(1, total)))
                    Text("\(progress.completed) of \(total) cards", bundle: .module)
                } else { ProgressView() }
            }
            Button { Task { await collection.enableFsrs() } } label: {
                Text("Enable FSRS", bundle: .module).frame(maxWidth: .infinity, minHeight: 44)
            }
            .buttonStyle(.borderedProminent)
            .disabled(collection.enabling)
            .accessibilityIdentifier("home.policy.enable")
        }
        .padding(Spacing.large)
        .frame(maxWidth: ContentWidth.readable)
        .interactiveDismissDisabled()
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("home.policy.sheet")
    }
}
