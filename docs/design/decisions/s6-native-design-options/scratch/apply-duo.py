import re
A="app/Packages/MeguriKit/Sources/"
def sub(path, old, new, count=1):
    s=open(path).read()
    assert old in s, (path, old[:70])
    s=s.replace(old,new,count) if count else s.replace(old,new)
    open(path,"w").write(s)

open(A+"DesignSystem/DuoDesign.swift","w").write('''import Foundation

/// Preview only, never shipped: which iPhone Duo design option this launch shows.
public enum DuoDesign {
    public static var rows: String { UserDefaults.standard.string(forKey: "DuoRows") ?? "" }
    public static var home: String { UserDefaults.standard.string(forKey: "DuoHome") ?? "" }
    public static var review: String { UserDefaults.standard.string(forKey: "DuoReview") ?? "" }
}
''')

# Deck menu: long press only when a row option is on.
M=A+"Features/Home/DeckManagement/DeckMenu.swift"
sub(M,"import CoreBridge\nimport SwiftUI","import CoreBridge\nimport DesignSystem\nimport SwiftUI")
sub(M,"        if let collection {\n            HStack {","        if let collection, !DuoDesign.rows.isEmpty {\n            content.contextMenu { DeckMenu(collection: collection, deck: deck) }\n        } else if let collection {\n            HStack {")

H=A+"Features/Home/Home.swift"
# Deck page on iOS.
sub(H,"                #else\n                scrollContent()\n                #endif","                #else\n                if !DuoDesign.home.isEmpty, let deck = macDeckPage {\n                    deckPage(deck)\n                } else {\n                    scrollContent()\n                }\n                #endif")
sub(H,"    #if os(macOS)\n    private var macDeckPage:","    private var macDeckPage:")
sub(H,"            .accessibilityIdentifier(\"home.deck.counts.\\(id)\")\n    }\n    #endif\n}","            .accessibilityIdentifier(\"home.deck.counts.\\(id)\")\n    }\n}")
sub(H,"        .accessibilityElement(children: .contain)\n        .accessibilityIdentifier(\"home.deck.page.\\(deck.id)\")\n    }\n    private func countText","        #if !os(macOS)\n        .listStyle(.plain)\n        #endif\n        .accessibilityElement(children: .contain)\n        .accessibilityIdentifier(\"home.deck.page.\\(deck.id)\")\n    }\n    private func countText")
# Symbol and title toolbar items on iOS.
sub(H,'''                    Button { collection.presentGoToDeck() } label: {
                        Text("Go to Deck…", bundle: .module)
                    }''','''                    Button { collection.presentGoToDeck() } label: {
                        if DuoDesign.home.isEmpty {
                            Text("Go to Deck…", bundle: .module)
                        } else {
                            Label { Text("Go to Deck…", bundle: .module) } icon: { Image(systemName: "magnifyingglass") }
                        }
                    }''')
sub(H,'''                Button { collection.deckActions.present(.create) } label: {
                    Text("Create Deck", bundle: .module)
                }''','''                Button { collection.deckActions.present(.create) } label: {
                    if DuoDesign.home.isEmpty {
                        Text("Create Deck", bundle: .module)
                    } else {
                        Label { Text("Create Deck", bundle: .module) } icon: { Image(systemName: "plus") }
                    }
                }''')
# Row options.
sub(H,'''                VStack(alignment: .leading) {
                    Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)
                        .font(.body)
                        .accessibilityIdentifier("home.deck.name.\\(node.id)")
                    if let subtitle {''','''                if DuoDesign.rows == "b" || DuoDesign.rows == "c" {
                    Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)
                        .accessibilityIdentifier("home.deck.name.\\(node.id)")
                    Spacer()
                    if DuoDesign.rows == "b" {
                        if node.counts.review + node.counts.new + node.counts.learning > 0 {
                            Text(node.counts.review + node.counts.new + node.counts.learning, format: .number)
                                .foregroundStyle(.secondary)
                                .monospacedDigit()
                        }
                    } else {
                        HStack(spacing: Spacing.small) {
                            Text(node.counts.new, format: .number).foregroundStyle(.token(node.counts.new == 0 ? .textSecondary : .countNew))
                            Text(node.counts.learning, format: .number).foregroundStyle(.token(node.counts.learning == 0 ? .textSecondary : .countLearn))
                            Text(node.counts.review, format: .number).foregroundStyle(.token(node.counts.review == 0 ? .textSecondary : .countDue))
                        }
                        .font(.subheadline)
                        .monospacedDigit()
                    }
                } else if DuoDesign.rows == "a" {
                    VStack(alignment: .leading) {
                        Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)
                            .accessibilityIdentifier("home.deck.name.\\(node.id)")
                        if let plainCounts {
                            Text(verbatim: plainCounts)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                                .accessibilityIdentifier("home.deck.rowCounts.\\(node.id)")
                        }
                    }
                } else {
                VStack(alignment: .leading) {
                    Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)
                        .font(.body)
                        .accessibilityIdentifier("home.deck.name.\\(node.id)")
                    if let subtitle {''')
sub(H,'''                            .accessibilityIdentifier("home.deck.rowCounts.\\(node.id)")
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
                #endif''','''                            .accessibilityIdentifier("home.deck.rowCounts.\\(node.id)")
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
                }
                #endif''')
# Sidebar rows: name and one quiet number.
sub(H,'''            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(verbatim: name)
                DeckCounts(counts: node.counts)
            }
            .foregroundStyle(.token(.textPrimary))''','''            Group {
                if DuoDesign.rows.isEmpty {
                    VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                        Text(verbatim: name)
                        DeckCounts(counts: node.counts)
                    }
                } else {
                    HStack {
                        Text(verbatim: name)
                        Spacer()
                        if node.counts.review + node.counts.new + node.counts.learning > 0 {
                            Text(node.counts.review + node.counts.new + node.counts.learning, format: .number)
                                .foregroundStyle(.secondary)
                                .monospacedDigit()
                        }
                    }
                }
            }
            .foregroundStyle(.token(.textPrimary))''')

R=A+"Features/Review/Review.swift"
sub(R,'''    #else
    private var touchBody: some View {
        VStack(spacing: 0) {
            header''','''    #else
    @ViewBuilder private var touchBody: some View {
        if DuoDesign.review.isEmpty { currentTouchBody } else { duoBody }
    }
    /// Preview only. The fold sits at x 455–495 on the inner display (measured); the shipped version reads `reservedRegions`.
    private var duoBody: some View {
        NavigationStack {
            GeometryReader { geometry in
                let wide = geometry.size.width > 700
                let leading = max(0, 455 - geometry.frame(in: .global).minX)
                let cardColumn = VStack(spacing: Spacing.xSmall) {
                    sessionProgress.padding(.horizontal, Spacing.medium)
                    cardRegion
                }
                Group {
                    if DuoDesign.review == "c" {
                        HStack(spacing: 0) {
                            cardColumn
                            ReviewAnswerBar(phase: phase, states: states, enabled: enabled, reveal: reveal, rate: rate, duoStyle: "column", duoLeading: 0)
                                .frame(width: 96)
                        }
                    } else if DuoDesign.review == "b", wide {
                        HStack(spacing: 40) {
                            cardColumn.frame(width: leading)
                            VStack {
                                Spacer()
                                ReviewAnswerBar(phase: phase, states: states, enabled: enabled, reveal: reveal, rate: rate, duoStyle: "list", duoLeading: 0)
                            }
                            .frame(maxWidth: .infinity)
                        }
                    } else {
                        VStack(spacing: 0) {
                            cardColumn
                            ReviewAnswerBar(phase: phase, states: states, enabled: enabled, reveal: reveal, rate: rate,
                                duoStyle: wide ? "split" : (DuoDesign.review == "b" ? "plain" : "row"), duoLeading: leading)
                        }
                    }
                }
            }
            .background(.token(.cardPage))
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(action: close) {
                        Label { Text("Close Review", bundle: .module) } icon: { Image(systemName: "xmark") }
                    }
                    .accessibilityIdentifier("review.session.close")
                }
                ToolbarItem(placement: .primaryAction) {
                    Button { if canUndo { undo() } } label: {
                        Label { Text(verbatim: undoDescription) } icon: { Image(systemName: "arrow.uturn.backward") }
                    }
                    .disabled(!canUndo)
                    .accessibilityIdentifier("review.session.back")
                }
            }
            .navigationBarTitleDisplayMode(.inline)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("review.session.screen")
    }
    private var currentTouchBody: some View {
        VStack(spacing: 0) {
            header''')
sub(R,'''        #else
        touchBar
        #endif''','''        #else
        if duoStyle.isEmpty { touchBar } else { duoBar }
        #endif''')
sub(R,'''    @ViewBuilder private var touchBar: some View {''','''    #if !os(macOS)
    /// Preview only: the iPhone Duo answer options.
    @ViewBuilder private var duoBar: some View {
        if phase == .question || phase == .answer || phase == .transition {
            VStack(spacing: 0) {
                if duoStyle == "plain" { Divider() }
                ZStack(alignment: .bottom) {
                    if phase == .answer {
                        duoRatings.allowsHitTesting(enabled)
                    } else {
                        duoRatings.hidden().allowsHitTesting(false).accessibilityHidden(true)
                    }
                    if phase == .question { duoReveal }
                }
                .padding(Spacing.small)
            }
            .frame(maxHeight: duoStyle == "column" ? .infinity : nil, alignment: .bottom)
            .fixedSize(horizontal: false, vertical: duoStyle != "column")
            .layoutPriority(1)
        }
    }
    @ViewBuilder private var duoRatings: some View {
        let again = duoRating(.again, "Again", states?.again.label, .ratingAgain)
        let hard = duoRating(.hard, "Hard", states?.hard.label, .textPrimary)
        let good = duoRating(.good, "Good", states?.good.label, .ratingGood)
        let easy = duoRating(.easy, "Easy", states?.easy.label, .textPrimary)
        switch duoStyle {
        case "split":
            HStack(spacing: 40) {
                HStack(spacing: Spacing.xSmall) { again; hard }.frame(width: max(0, duoLeading - Spacing.small))
                HStack(spacing: Spacing.xSmall) { good; easy }.frame(maxWidth: .infinity)
            }
        case "column", "list":
            VStack(spacing: Spacing.xSmall) { again; hard; good; easy }
        default:
            HStack(spacing: Spacing.xSmall) { again; hard; good; easy }
        }
    }
    @ViewBuilder private var duoReveal: some View {
        let button = Button(action: reveal) {
            Text("Show Answer", bundle: .module)
                .font(.subheadline.weight(.semibold))
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget)
        }
        .tint(ColorToken.textPrimary.color)
        .disabled(!enabled)
        .accessibilityIdentifier("review.session.reveal")
        if duoStyle == "plain" {
            button.buttonStyle(.plain)
        } else if duoStyle == "split" {
            HStack(spacing: 40) {
                Color.clear.frame(width: max(0, duoLeading - Spacing.small), height: 0)
                button.buttonStyle(.bordered).buttonBorderShape(.roundedRectangle).frame(maxWidth: .infinity)
            }
        } else {
            button.buttonStyle(.bordered).buttonBorderShape(.roundedRectangle)
        }
    }
    @ViewBuilder private func duoRating(_ rating: ReviewRating, _ title: String, _ label: ReviewAnswerLabel?, _ color: ColorToken) -> some View {
        let interval = label.map(Self.interval) ?? "—"
        let button = Button { rate(rating) } label: {
            Group {
                if duoStyle == "list" {
                    HStack {
                        Text(LocalizedStringKey(title), bundle: .module).font(.body)
                        Spacer()
                        Text(verbatim: interval).font(.body).monospacedDigit().foregroundStyle(.secondary)
                    }
                } else {
                    VStack(spacing: 0) {
                        Text(LocalizedStringKey(title), bundle: .module).font(.subheadline.weight(.semibold))
                        Text(verbatim: interval).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            .foregroundStyle(.token(color))
            .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget)
            .contentShape(Rectangle())
        }
        .disabled(!enabled || phase != .answer)
        .accessibilityLabel("\\(title), \\(interval)")
        if duoStyle == "plain" {
            button.buttonStyle(.plain)
        } else {
            button.buttonStyle(.bordered).buttonBorderShape(.roundedRectangle)
        }
    }
    #endif
    @ViewBuilder private var touchBar: some View {''')
s=open(R).read()
m=re.search(r'struct ReviewAnswerBar[^\n]*\n((?:.*\n){0,14})', s)
print(m.group(0)[:900])
s2=open(R).read()
print(re.search(r'public init\(phase[^\n]*\n(?:.*\n){0,4}', s2).group(0)[:400] if re.search(r'public init\(phase', s2) else 'no init')
