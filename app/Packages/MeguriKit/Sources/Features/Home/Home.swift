import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

public struct HomeView: View {
    public let collection: ReviewCollection
    public let deckID: Int64?
    public init(collection: ReviewCollection, deckID: Int64? = nil) { self.collection = collection; self.deckID = deckID }
    public var body: some View {
        Group {
            if showsRootDeckList {
                HomeDeckTree(nodes: collection.decks, navigation: collection.navigation, collection: collection,
                    previousAnswerCount: collection.previousRecord?.answers.count, error: collection.error,
                    isExpanded: { collection.studyExpanded(deckID: $0) },
                    isExpansionPending: { collection.isStudyExpansionPending(deckID: $0) },
                    setExpanded: { id, expanded, descendants in
                        collection.setStudyExpanded(deckID: id, expanded: expanded, includingDescendants: descendants)
                    }) { id in
                    collection.navigation.select(.deck(DeckID(id)))
                }
            } else {
                #if os(macOS)
                GeometryReader { geometry in
                    scrollContent(availableHeight: geometry.size.height)
                }
                #else
                scrollContent()
                #endif
            }
        }
        .navigationTitle(deckID == nil ? String(localized: "Decks", bundle: .module) : "")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button { collection.deckActions.present(.create) } label: {
                    Text("Create Deck", bundle: .module)
                }
                .disabled(!collection.deckActions.canPresent(.create))
                .accessibilityIdentifier("home.collection.createDeck")
            }
        }
        .accessibilityIdentifier("home.collection.screen")
    }
    private var showsRootDeckList: Bool {
        guard case .ready = collection.state, !collection.decks.isEmpty else { return false }
        return deckID.flatMap { collection.deckPage($0) } == nil
    }
    private var centersContent: Bool {
        #if os(macOS)
        if case .ready = collection.state {
            return collection.decks.isEmpty || deckID.flatMap { collection.deckPage($0) } != nil
        }
        if case .noFile = collection.state { return true }
        #endif
        return false
    }
    private func scrollContent(availableHeight: CGFloat = 0) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Spacing.large) {
                if !showsDeckSessionFooter, let record = collection.previousRecord, !record.answers.isEmpty {
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
    private var showsDeckSessionFooter: Bool {
        #if os(macOS)
        if case .ready = collection.state {
            return deckID.flatMap { collection.deckPage($0) } != nil
        }
        #endif
        return false
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
            if let deckID, let deck = collection.deckPage(deckID) {
                #if os(macOS)
                macDeckContent(deck)
                #else
                VStack(spacing: Spacing.large) {
                    Text(verbatim: deck.name)
                        .font(.largeTitle.bold())
                        .multilineTextAlignment(.center)
                        .fixedSize(horizontal: false, vertical: true)
                    DeckCounts(counts: deck.counts)
                        .accessibilityIdentifier("home.deck.counts")
                    Button { Task { await collection.startStudy(deckID: deck.id) } } label: {
                        Label {
                            Text("Study", bundle: .module)
                                .fixedSize(horizontal: false, vertical: true)
                        } icon: { Image(systemName: "play.fill") }
                            .font(.headline)
                            .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .frame(maxWidth: ContentWidth.primaryAction)
                    .disabled(!collection.canStudy)
                    .accessibilityIdentifier("home.deck.study")
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, Spacing.xxLarge)
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("home.deck.page.\(deck.id)")
                #endif
            } else if collection.decks.isEmpty {
                EmptyStateView(Text("No Decks", bundle: .module), systemImage: "rectangle.stack",
                    description: Text("This collection has no decks to study.", bundle: .module))
            }
        }
    }

    #if os(macOS)
    private func macDeckContent(_ deck: ReviewCollection.DeckPageDetail) -> some View {
        VStack {
            Text(verbatim: deck.name)
                .font(.title)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            Grid {
                GridRow {
                    Text(deck.counts.new, format: .number)
                        .foregroundStyle(.token(deck.counts.new == 0 ? .countZero : .countNew))
                        .accessibilityLabel(Text("New", bundle: .module))
                        .accessibilityValue(Text(deck.counts.new, format: .number))
                        .accessibilityIdentifier("home.deck.counts.new")
                    Text(deck.counts.learning, format: .number)
                        .foregroundStyle(.token(deck.counts.learning == 0 ? .countZero : .countLearn))
                        .accessibilityLabel(Text("Learning", bundle: .module))
                        .accessibilityValue(Text(deck.counts.learning, format: .number))
                        .accessibilityIdentifier("home.deck.counts.learning")
                    Text(deck.counts.review, format: .number)
                        .foregroundStyle(.token(deck.counts.review == 0 ? .countZero : .countDue))
                        .accessibilityLabel(Text("Due", bundle: .module))
                        .accessibilityValue(Text(deck.counts.review, format: .number))
                        .accessibilityIdentifier("home.deck.counts.due")
                }
                GridRow {
                    Text("New", bundle: .module).padding(.horizontal).accessibilityHidden(true)
                    Text("Learning", bundle: .module).padding(.horizontal).accessibilityHidden(true)
                    Text("Due", bundle: .module).padding(.horizontal).accessibilityHidden(true)
                }
            }
            .padding(.vertical)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("home.deck.counts")
            Button { Task { await collection.startStudy(deckID: deck.id) } } label: {
                Label { Text("Study", bundle: .module) } icon: { Image(systemName: "play.fill") }
            }
            .buttonStyle(.borderedProminent)
            .frame(minHeight: ControlMetrics.minimumTarget)
            .contentShape(Rectangle())
            .disabled(!collection.canStudy)
            .accessibilityIdentifier("home.deck.study")
            if let record = collection.previousRecord, record.deckID == deck.id, !record.answers.isEmpty {
                VStack {
                    Text("Previous Session", bundle: .module)
                    Text("\(record.answers.count) answers recorded", bundle: .module)
                        .accessibilityIdentifier("home.session.recorded")
                }
                .font(.footnote)
                .foregroundStyle(.secondary)
            }
        }
        .multilineTextAlignment(.center)
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("home.deck.page.\(deck.id)")
    }
    #endif
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

/// Owns the root's native scroll container; never embed this list in a ScrollView.
public struct HomeDeckTree: View {
    public let nodes: [CountedDeckNode]
    public let select: (Int64) -> Void
    private let navigation: AppNavigation?
    private let collection: ReviewCollection?
    private let previousAnswerCount: Int?
    private let error: String?
    private let isExpanded: ((Int64) -> Bool)?
    private let isExpansionPending: ((Int64) -> Bool)?
    private let setExpanded: ((Int64, Bool, Bool) -> Void)?
    @State private var rowFrames: [Int64: CGRect] = [:]
    @State private var restoring: Int64?
    @State private var tracking = false

    public init(nodes: [CountedDeckNode], navigation: AppNavigation? = nil, collection: ReviewCollection? = nil,
                previousAnswerCount: Int? = nil, error: String? = nil,
                isExpanded: ((Int64) -> Bool)? = nil,
                isExpansionPending: ((Int64) -> Bool)? = nil,
                setExpanded: ((Int64, Bool, Bool) -> Void)? = nil,
                select: @escaping (Int64) -> Void) {
        self.nodes = nodes; self.navigation = navigation; self.collection = collection
        self.previousAnswerCount = previousAnswerCount; self.error = error; self.select = select
        self.isExpanded = isExpanded; self.setExpanded = setExpanded
        self.isExpansionPending = isExpansionPending
    }

    public var body: some View {
        GeometryReader { viewport in
            ScrollViewReader { proxy in
                List {
                    Section {
                        ForEach(nodes, id: \.id) { node in
                            HomeDeckBranch(node: node, collection: collection, ancestors: [], depth: 0, select: select,
                                isExpanded: isExpanded, isExpansionPending: isExpansionPending,
                                setExpanded: setExpanded, frameChanged: recordFrame)
                        }
                    } header: {
                        Text("Decks", bundle: .module)
                            .accessibilityIdentifier("home.deck.header")
                    }
                    if let error {
                        Section {
                            Text(verbatim: error)
                                .foregroundStyle(.token(.statusError))
                                .accessibilityIdentifier("home.collection.error")
                                .id(InfoAnchor.error)
                        }
                    }
                    if let previousAnswerCount, previousAnswerCount > 0 {
                        Section {
                            Text("\(previousAnswerCount) answers recorded", bundle: .module)
                                .foregroundStyle(.secondary)
                                .accessibilityIdentifier("home.session.recorded")
                        } header: { Text("Previous Session", bundle: .module) }
                        .id(InfoAnchor.previousSession)
                    }
                }
                .coordinateSpace(name: "home.deck.list")
                // List does not use the ScrollView target-layout modifier.
                // Track actual row geometry, not lazy row creation, so cached
                // off-screen rows cannot overwrite the saved navigation anchor.
                .onChange(of: rowFrames) { _, frames in
                    let visible = frames.filter { $0.value.maxY > 0 && $0.value.minY < viewport.size.height }
                    if let restoring {
                        // Saved anchors can become hidden by a persisted parent disclosure.
                        guard visible[restoring] != nil || !contains(restoring, in: nodes) else { return }
                        self.restoring = nil
                        tracking = true
                    }
                    guard tracking, let top = visible.min(by: { $0.value.minY < $1.value.minY })?.key else { return }
                    navigation?.setScrollPosition(ScrollPosition(id: top, anchor: .top), for: .section(.home))
                }
                .task {
                    tracking = false
                    await Task.yield()
                    let saved = navigation?.scrollPosition(for: .section(.home))
                    if let id = saved?.viewID as? Int64, contains(id, in: nodes) {
                        restoring = id
                        proxy.scrollTo(id, anchor: .top)
                        if let frame = rowFrames[id], frame.maxY > 0, frame.minY < viewport.size.height {
                            restoring = nil
                            tracking = true
                        }
                    } else {
                        restoring = nil
                        if saved?.edge == .top { scrollToTop(proxy) }
                        tracking = true
                    }
                }
                .onDisappear { tracking = false }
                .onChange(of: navigation?.scrollPosition(for: .section(.home)).edge) { _, edge in
                    if edge == .top {
                        restoring = nil
                        tracking = true
                        scrollToTop(proxy)
                    }
                }
            }
        }
    }

    private enum InfoAnchor: Hashable { case previousSession, error }
    private func recordFrame(_ id: Int64, _ frame: CGRect?) { rowFrames[id] = frame }
    private func contains(_ id: Int64, in nodes: [CountedDeckNode]) -> Bool {
        nodes.contains { node in
            node.id == id || ((isExpanded?(node.id) ?? !node.studyCollapsed) && contains(id, in: node.children))
        }
    }
    private func scrollToTop(_ proxy: ScrollViewProxy) {
        if let first = nodes.first {
            proxy.scrollTo(first.id, anchor: .top)
        } else if error != nil {
            proxy.scrollTo(InfoAnchor.error, anchor: .top)
        } else if let previousAnswerCount, previousAnswerCount > 0 {
            proxy.scrollTo(InfoAnchor.previousSession, anchor: .top)
        }
    }
}

private struct HomeDeckBranch: View {
    let node: CountedDeckNode
    let collection: ReviewCollection?
    let ancestors: [String]
    let depth: Int
    let select: (Int64) -> Void
    let isExpanded: ((Int64) -> Bool)?
    let isExpansionPending: ((Int64) -> Bool)?
    let setExpanded: ((Int64, Bool, Bool) -> Void)?
    let frameChanged: (Int64, CGRect?) -> Void
    @Environment(\.locale) private var locale

    private var path: String {
        node.name.contains("::") ? node.name : (ancestors + [node.name]).joined(separator: "::")
    }

    private var expanded: Binding<Bool> {
        Binding(get: { isExpanded?(node.id) ?? !node.studyCollapsed },
                set: { setExpanded?(node.id, $0, studyDisclosureIncludesDescendants()) })
    }

    var body: some View {
        Group {
            #if os(macOS)
            if node.children.isEmpty {
                row
            } else {
                DisclosureGroup(isExpanded: expanded) {
                    ForEach(node.children, id: \.id) { child in
                        childBranch(child)
                    }
                } label: { row }
            }
            #else
            HStack {
                row
                if !node.children.isEmpty {
                    disclosure
                }
            }
            .padding(.leading, CGFloat(depth) * Spacing.medium)
            if expanded.wrappedValue {
                ForEach(node.children, id: \.id) { child in
                    childBranch(child)
                }
            }
            #endif
        }
        .id(node.id)
    }

    private func childBranch(_ child: CountedDeckNode) -> some View {
        AnyView(HomeDeckBranch(node: child, collection: collection, ancestors: path.components(separatedBy: "::"), depth: depth + 1,
            select: select, isExpanded: isExpanded, isExpansionPending: isExpansionPending,
            setExpanded: setExpanded, frameChanged: frameChanged))
    }

    #if !os(macOS)
    private var disclosure: some View {
        // Only this leaf owns disclosure semantics; selection and child rows are siblings.
        DisclosureGroup(isExpanded: expanded) { EmptyView() } label: { EmptyView() }
            .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
            .contentShape(Rectangle())
            .accessibilityRepresentation {
                Button { expanded.wrappedValue.toggle() } label: {
                    Text(verbatim: path)
                }
                .accessibilityValue(disclosureValue)
                .accessibilityIdentifier("home.deck.disclosure.\(node.id)")
            }
    }

    private var disclosureValue: Text {
        if isExpansionPending?(node.id) == true {
            return expanded.wrappedValue
                ? Text("Expanded, saving", bundle: .module) : Text("Collapsed, saving", bundle: .module)
        }
        return expanded.wrappedValue
            ? Text("Expanded", bundle: .module) : Text("Collapsed", bundle: .module)
    }
    #endif

    private var row: some View {
        Button { select(node.id) } label: {
            HStack {
                if node.filtered {
                    Image(systemName: "line.3.horizontal.decrease.circle")
                        .imageScale(.small)
                        .accessibilityHidden(true)
                }
                VStack(alignment: .leading) {
                    Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)
                        .font(.body)
                        .accessibilityIdentifier("home.deck.name.\(node.id)")
                    if let subtitle {
                        Text(verbatim: subtitle)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .accessibilityIdentifier("home.deck.rowCounts.\(node.id)")
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        // Keep the row action distinct from its native disclosure control.
        .buttonStyle(.plain)
        .accessibilityLabel(Text(verbatim: path))
        .accessibilityValue(Text("\(Text(node.counts.new, format: .number)) new · \(Text(node.counts.learning, format: .number)) learning · \(Text(node.counts.review, format: .number)) due", bundle: .module))
        .accessibilityIdentifier("home.deck.row.\(node.id)")
        #if os(macOS)
        .help(Text(verbatim: path))
        #endif
        .onGeometryChange(for: CGRect.self) { $0.frame(in: .named("home.deck.list")) } action: {
            frameChanged(node.id, $0)
        }
        .onDisappear { frameChanged(node.id, nil) }
        .deckMenu(collection: collection, deck: node)
    }

    private var subtitle: String? {
        let parts: [String?] = [
            node.counts.new == 0 ? nil : String(localized: "\(node.counts.new.formatted(.number.locale(locale))) new", bundle: .module),
            node.counts.learning == 0 ? nil : String(localized: "\(node.counts.learning.formatted(.number.locale(locale))) learning", bundle: .module),
            node.counts.review == 0 ? nil : String(localized: "\(node.counts.review.formatted(.number.locale(locale))) due", bundle: .module),
        ]
        let visible = parts.compactMap { $0 }
        return visible.isEmpty ? nil : visible.joined(separator: " · ")
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
                #if !os(macOS)
                .font(.subheadline)
                .foregroundStyle(.token(.textSecondary))
                .lineLimit(nil)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                #endif
                .accessibilityIdentifier("home.sidebar.guidance")
        } else {
            ForEach(collection.decks, id: \.id) { node in
                HomeSidebarBranch(collection: collection, node: node, ancestors: [])
            }
        }
    }

    private var emptyGuidance: Text { Text("No decks", bundle: .module) }
}

private struct HomeSidebarBranch: View {
    let collection: ReviewCollection
    let node: CountedDeckNode
    let ancestors: [String]
    private var name: String { node.name.components(separatedBy: "::").last ?? node.name }
    private var path: String { (ancestors + [name]).joined(separator: "::") }
    private var expanded: Binding<Bool> {
        Binding(get: { collection.studyExpanded(deckID: node.id) }, set: {
            collection.setStudyExpanded(deckID: node.id, expanded: $0,
                includingDescendants: studyDisclosureIncludesDescendants())
        })
    }

    var body: some View {
        Group {
            if node.children.isEmpty {
                row
            } else {
                DisclosureGroup(isExpanded: expanded) {
                    ForEach(node.children, id: \.id) { child in
                        AnyView(HomeSidebarBranch(collection: collection, node: child, ancestors: ancestors + [name]))
                    }
                } label: { row }
                .tag(SidebarItem.deck(DeckID(node.id)))
                #if !os(macOS)
                .accessibilityElement(children: .contain)
                .accessibilityLabel(Text(verbatim: path))
                .accessibilityValue(expanded.wrappedValue
                    ? Text("Expanded", bundle: .module) : Text("Collapsed", bundle: .module))
                .accessibilityIdentifier("home.sidebar.disclosure.\(node.id)")
                #endif
            }
        }
        .id(node.id)
    }

    private var row: some View {
        Group {
            #if os(macOS)
            Button { collection.openDeckPage(deckID: node.id) } label: {
                Text(verbatim: name)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(Text(verbatim: path))
            .accessibilityLabel(Text(verbatim: path))
            .accessibilityValue(Text("\(Text(node.counts.review, format: .number)) due", bundle: .module))
            .accessibilityIdentifier("home.sidebar.deck.\(node.id)")
            .tag(SidebarItem.deck(DeckID(node.id)))
            #else
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(verbatim: name)
                DeckCounts(counts: node.counts)
            }
            .foregroundStyle(.token(.textPrimary))
            .lineLimit(nil)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .listRowBackground(ColorToken.surface.color)
            .accessibilityElement(children: .combine)
            .accessibilityLabel(Text(verbatim: path))
            #endif
        }
        #if !os(macOS)
        .accessibilityIdentifier("home.sidebar.deck.\(node.id)")
        #endif
        .deckMenu(collection: collection, deck: node)
        .tag(SidebarItem.deck(DeckID(node.id)))
        #if os(macOS)
        .badge(Text(node.counts.review, format: .number))
        #endif
    }
}

private func studyDisclosureIncludesDescendants() -> Bool {
    #if os(macOS)
    NSEvent.modifierFlags.contains(.option)
    #else
    false
    #endif
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
