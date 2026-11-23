import CardRendering
import CoreBridge
import Foundation
import Observation

/// One render of the Cards tab's preview, as the page shows it
/// (`render/preview` V11): both sides' display HTML, their audio, the
/// outcome, and the note rendered.
public nonisolated struct NoteTypePreviewRender: Sendable, Hashable {
    public var cardID: Int64
    public var ordinal: UInt32
    public var question: String
    public var answer: String
    public var questionReplayIDs: [String]
    public var answerReplayIDs: [String]
    public var questionAudio: [CardAudio]
    public var answerAudio: [CardAudio]
    public var mediaRefs: [String]
    public var outcome: CardRenderOutcome
    public var flag: UInt8
    public var marked: Bool
    /// The stored note rendered; nil for a new empty note.
    public var noteID: Int64?
    /// A cloze note type's card numbers, ascending.
    public var clozeNumbers: [UInt32]

    public init(cardID: Int64 = 0, ordinal: UInt32 = 0, question: String, answer: String, questionReplayIDs: [String] = [],
                answerReplayIDs: [String] = [], questionAudio: [CardAudio] = [], answerAudio: [CardAudio] = [], mediaRefs: [String] = [],
                outcome: CardRenderOutcome = .rendered, flag: UInt8 = 0, marked: Bool = false, noteID: Int64? = nil,
                clozeNumbers: [UInt32] = []) {
        self.cardID = cardID
        self.ordinal = ordinal
        self.question = question
        self.answer = answer
        self.questionReplayIDs = questionReplayIDs
        self.answerReplayIDs = answerReplayIDs
        self.questionAudio = questionAudio
        self.answerAudio = answerAudio
        self.mediaRefs = mediaRefs
        self.outcome = outcome
        self.flag = flag
        self.marked = marked
        self.noteID = noteID
        self.clozeNumbers = clozeNumbers
    }

    init(_ render: NotetypeDraftRender) {
        let card = render.card
        self.init(cardID: card.cardID, ordinal: card.ordinal, question: card.question ?? "", answer: card.answer ?? "",
                  questionReplayIDs: card.questionReplayIDs, answerReplayIDs: card.answerReplayIDs, questionAudio: card.questionAudio,
                  answerAudio: card.answerAudio, mediaRefs: card.mediaRefs, outcome: card.outcome, flag: card.flag, marked: card.marked,
                  noteID: render.noteID, clozeNumbers: render.clozeNumbers)
    }

    /// The page's payload for one side. The preview shows each side alone
    /// (V12): the side's own HTML is the page's question, so a switch never
    /// shows the other side first, and audio never plays by itself.
    public func payload(_ side: CardRenderSide) -> CardPayload {
        let html = side == .question ? question : answer
        let replay = side == .question ? questionReplayIDs : answerReplayIDs
        let audio = side == .question ? questionAudio : answerAudio
        return CardPayload(cardID: cardID, ordinal: Int(ordinal), question: html, answer: html, questionReplayIDs: replay,
                           answerReplayIDs: replay, questionAudio: audio, answerAudio: audio, mediaRefs: mediaRefs, typeIn: nil,
                           // V12: the preview draws no flag or mark.
                           flag: 0, marked: false, audioPolicy: CardAudioPolicy(autoplay: false, interrupt: true, replayQuestion: false))
    }
}

/// The preview's core reads (`notetypes/workspace` W9): pure reads of one
/// snapshot that write nothing. Production is `CoreNoteTypePreviewBackend`.
@MainActor public protocol NoteTypePreviewBackend: AnyObject {
    /// `render/preview` V2 for the draft: `target` is the card type's
    /// position in the draft, or a cloze number minus one.
    func render(_ draft: NotetypeStructureDraft, note: Int64?, target: UInt32, fillEmpty: Bool) async throws -> NoteTypePreviewRender
    /// The type's note ids, ascending.
    func noteIds(_ notetype: Int64) async throws -> [Int64]
}

@MainActor public final class CoreNoteTypePreviewBackend: NoteTypePreviewBackend {
    private let service = CoreNotetypeService()
    public init() {}

    public func render(_ draft: NotetypeStructureDraft, note: Int64?, target: UInt32, fillEmpty: Bool) async throws -> NoteTypePreviewRender {
        #if os(macOS)
        let platform: CardRenderPlatform = .mac
        #else
        let platform: CardRenderPlatform = .ios
        #endif
        return NoteTypePreviewRender(try await service.renderNotetypeDraft(draft, note: note, target: target, fillEmpty: fillEmpty,
                                                                           platform: platform))
    }

    public func noteIds(_ notetype: Int64) async throws -> [Int64] { try await service.notetypeNoteIds(notetype) }
}

/// The Cards tab's live preview (`notetypes/workspace` W9; N22.3, N22.7):
/// the selected card type of the draft, unsaved texts and styling included,
/// for one note of the type (the note Note Types was opened from, else the
/// newest; Previous and Next step through them), or with Fill Empty Fields
/// for a type without notes. It follows the editor's Front | Back (Styling
/// keeps the side shown) and redraws about 300 ms after typing pauses; the
/// shown render stays until the next one is on the page, and a template
/// error keeps it under the error rather than showing the error page.
/// Nothing here writes to the collection.
@Observable @MainActor
public final class NoteTypePreview {
    /// What the preview shows: the draft and the card type, from the section.
    public nonisolated struct Subject: Sendable, Hashable {
        public let notetype: Int64
        public let draft: NotetypeStructureDraft
        /// The card type's position among the draft's kept card types.
        public let position: Int
        public let cloze: Bool
        public init(notetype: Int64, draft: NotetypeStructureDraft, position: Int, cloze: Bool) {
            self.notetype = notetype
            self.draft = draft
            self.position = position
            self.cloze = cloze
        }
    }

    /// Why the shown render is not the draft's (the view says it in words).
    public nonisolated enum Problem: Sendable, Hashable {
        /// The draft's template does not render (V3): a red issue says why.
        case templateError(side: CardRenderSide)
        /// The core could not render the draft (changed elsewhere, a name it refuses).
        case unavailable
    }

    /// The side shown: Front shows the question, Back the answer; Styling
    /// keeps the side last shown.
    public var side: CardRenderSide = .question {
        didSet { if side != oldValue { refresh() } }
    }
    /// Light or Dark (the card's own appearance only, R940); nil follows the app.
    public var appearanceChoice: CardRuntime.Appearance? {
        didSet { applyAppearance() }
    }
    /// The app's appearance, which the card follows until one is chosen.
    public var systemAppearance: CardRuntime.Appearance = .light {
        didSet { applyAppearance() }
    }
    public var appearance: CardRuntime.Appearance { appearanceChoice ?? systemAppearance }

    /// The page pair, once made (production presenter).
    public private(set) var pair: CardPagePair?
    /// The note type whose notes `notes` lists.
    public private(set) var notesOf: Int64?
    /// The type's note ids, ascending; nil until read.
    public private(set) var notes: [Int64]?
    /// The note previewed, by its place in `notes`.
    public private(set) var noteIndex: Int?
    /// The render on the page and the subject it was made for.
    public private(set) var shown: NoteTypePreviewRender?
    public private(set) var problem: Problem?
    /// The cloze number previewed (cloze note types).
    public private(set) var clozeNumber: UInt32 = 1
    public var clozeNumbers: [UInt32] { shown?.clozeNumbers ?? [clozeNumber] }

    /// The toolbar's toggle, kept on the device (N22.3).
    public var requested: Bool {
        get { access(keyPath: \.requested); return defaults?.object(forKey: Self.shownKey) as? Bool ?? requestedHere }
        set {
            withMutation(keyPath: \.requested) {
                requestedHere = newValue
                defaults?.set(newValue, forKey: Self.shownKey)
            }
            if !newValue { pair?.pauseAudio() }
        }
    }
    /// The toggle without a device store (tests, test hooks).
    @ObservationIgnored private var requestedHere = true

    /// The Mac's Cards tab is wide enough for the editor and the preview side by side.
    public var fits = true

    /// The preview's pane is on screen: only then does it read and render.
    public var visible = false {
        didSet {
            guard visible != oldValue else { return }
            if visible { refresh() } else { token &+= 1; pending?.cancel(); pair?.pauseAudio() }
        }
    }

    // MARK: Seams

    @ObservationIgnored let backend: any NoteTypePreviewBackend
    @ObservationIgnored public var defaults: UserDefaults? = .standard
    /// The draft and card type to show, read when a render starts.
    @ObservationIgnored public var subject: @MainActor () -> Subject? = { nil }
    /// The collection's media, bound as review binds it.
    @ObservationIgnored public var resolver: @MainActor () async -> MediaPathResolver? = { nil }
    /// Puts a payload on the page; false when it was superseded there.
    @ObservationIgnored var present: (@MainActor (CardPayload) async throws -> Bool)?
    /// The note a way in asked for (W14: the note Note Types was opened from).
    @ObservationIgnored public var preferredNote: [Int64: Int64] = [:]
    /// How long a render waits after the last edit (W9's pause).
    @ObservationIgnored var typingDelay: Duration = .milliseconds(300)

    @ObservationIgnored private var fillEmptyChoice: [Int64: Bool] = [:]
    /// The note shown before the list was read again (`notesChanged`).
    @ObservationIgnored private var keptNote: Int64?
    /// Raised whenever the list goes: a read started before is not taken.
    @ObservationIgnored private var notesGeneration: UInt64 = 0
    @ObservationIgnored private var token: UInt64 = 0
    @ObservationIgnored private var pending: Task<Void, Never>?
    /// What `shown` was made from: a side switch of the same input reuses it.
    @ObservationIgnored private var shownKey: RenderKey?
    @ObservationIgnored private var notesTask: Task<[Int64]?, Never>?
    @ObservationIgnored private var generation: UInt64 = 0
    @ObservationIgnored private var loading: Task<CardPagePair?, any Error>?
    @ObservationIgnored private var loaded = false

    static let shownKey = "noteTypes.cards.previewShown"

    private struct RenderKey: Hashable {
        let draft: NotetypeStructureDraft
        let note: Int64?
        let target: UInt32
        let fillEmpty: Bool
    }

    public init(backend: any NoteTypePreviewBackend = CoreNoteTypePreviewBackend()) {
        self.backend = backend
    }

    // MARK: Following the section

    /// The editor's part changed: Front and Back choose the side shown.
    public func follow(part: NoteTypeTemplatePart) {
        switch part {
        case .front: side = .question
        case .back: side = .answer
        case .styling: break
        }
    }

    /// The draft was edited: render again once typing pauses.
    public func draftEdited() { refresh(after: typingDelay) }

    /// The selection, the card type or the tab changed: render at once.
    public func subjectChanged() { refresh() }

    /// Notes of the shown type were added, removed or edited elsewhere.
    public func notesChanged() {
        // The type stays: the note shown is found again in the new list.
        // A second change before the list is read again keeps the note from the first.
        if let shown = noteIndex.flatMap({ notes?[safe: $0] }) { keptNote = shown }
        notes = nil
        notesGeneration &+= 1
        notesTask?.cancel()
        notesTask = nil
        shownKey = nil
        refresh()
    }

    // MARK: The note menu (W9)

    /// "<i> of <k>" when the type has notes.
    public var notePosition: (index: Int, count: Int)? {
        guard let notes, let noteIndex, !notes.isEmpty, subject()?.notetype == notesOf else { return nil }
        return (noteIndex + 1, notes.count)
    }

    public var canStep: (previous: Bool, next: Bool) {
        guard let position = notePosition else { return (false, false) }
        return (position.index > 1, position.index < position.count)
    }

    public func step(by delta: Int) {
        guard let notes, let index = noteIndex, notes.indices.contains(index + delta) else { return }
        noteIndex = index + delta
        refresh()
    }

    /// Fill Empty Fields: on for a type without notes until turned off (V5 rule 3).
    public var fillEmpty: Bool {
        guard let id = subject()?.notetype else { return false }
        return fillEmptyChoice[id] ?? (notesOf == id && notes?.isEmpty == true)
    }

    public func setFillEmpty(_ on: Bool) {
        guard let id = subject()?.notetype else { return }
        fillEmptyChoice[id] = on
        refresh()
    }

    public func setClozeNumber(_ number: UInt32) {
        guard number >= 1, number != clozeNumber else { return }
        clozeNumber = number
        refresh()
    }

    // MARK: Rendering

    /// Starts a render after `delay`; a newer call supersedes it, and its
    /// result is dropped if a newer call came while it ran (V2).
    func refresh(after delay: Duration = .zero) {
        token &+= 1
        let token = token
        pending?.cancel()
        guard visible else { return }
        pending = Task {
            if delay > .zero {
                guard (try? await Task.sleep(for: delay)) != nil else { return }
            }
            await render(token)
        }
    }

    private func render(_ token: UInt64) async {
        guard token == self.token, let subject = subject() else { return }
        guard let notes = await noteList(subject.notetype), token == self.token else { return }
        if noteIndex == nil || noteIndex.map({ !notes.indices.contains($0) }) == true {
            noteIndex = Self.startIndex(notes, preferred: preferredNote[subject.notetype])
        }
        let note = noteIndex.flatMap { notes[safe: $0] }
        let target = subject.cloze ? clozeNumber - 1 : UInt32(subject.position)
        let key = RenderKey(draft: subject.draft, note: note, target: target, fillEmpty: fillEmpty)
        let result: NoteTypePreviewRender
        if key == shownKey, let shown {
            result = shown
        } else {
            do {
                result = try await backend.render(subject.draft, note: note, target: target, fillEmpty: key.fillEmpty)
            } catch {
                if token == self.token { problem = .unavailable }
                return
            }
        }
        guard token == self.token else { return }
        if case .templateError(_, let side) = result.outcome {
            // The page keeps the last render; the error shows over it in the app's own words.
            problem = .templateError(side: side)
            shownKey = nil
            return
        }
        do {
            guard try await put(result.payload(side)), token == self.token else { return }
        } catch {
            if token == self.token { problem = .unavailable }
            return
        }
        shown = result
        shownKey = key
        problem = nil
    }

    /// W9: the note Note Types was opened from when it is one of the type's, else the newest.
    static func startIndex(_ notes: [Int64], preferred: Int64?) -> Int? {
        if let preferred, let at = notes.firstIndex(of: preferred) { return at }
        return notes.isEmpty ? nil : notes.count - 1
    }

    private func noteList(_ notetype: Int64) async -> [Int64]? {
        if notesOf == notetype, let notes { return notes }
        if notesOf != notetype {
            notesTask?.cancel()
            notesTask = nil
            notesOf = notetype
            notes = nil
            noteIndex = nil
            keptNote = nil
            clozeNumber = 1
            shownKey = nil
            notesGeneration &+= 1
        }
        let generation = notesGeneration
        let task = notesTask ?? Task { [backend] in try? await backend.noteIds(notetype) }
        notesTask = task
        let read = await task.value
        guard notesOf == notetype, generation == notesGeneration else { return nil }
        notesTask = nil
        // A failed read is not "no notes": nothing is cached, and the next render reads again.
        guard let read else {
            problem = .unavailable
            return nil
        }
        notes = read
        if let kept = keptNote {
            noteIndex = read.firstIndex(of: kept) ?? noteIndex.map { min($0, read.count - 1) }
            keptNote = nil
        }
        return read
    }

    private func put(_ payload: CardPayload) async throws -> Bool {
        if let present { return try await present(payload) }
        let token = token
        guard let pair = try await readyPair(), token == self.token else { return false }
        let update = try await pair.prepare(payload)
        guard token == self.token else { return false }
        try pair.present(update)
        return true
    }

    private func applyAppearance() {
        guard let pair else { return }
        let value = appearance
        Task { try? await pair.setAppearance(value) }
    }

    /// The collection closed or was replaced.
    public func close() {
        token &+= 1
        generation &+= 1
        pending?.cancel()
        notesTask?.cancel()
        notesTask = nil
        loading = nil
        pair?.close()
        pair = nil
        loaded = false
        shown = nil
        shownKey = nil
        problem = nil
        notesOf = nil
        notes = nil
        noteIndex = nil
        keptNote = nil
        notesGeneration &+= 1
        clozeNumber = 1
        fillEmptyChoice = [:]
        preferredNote = [:]
    }

    /// The pair, made and loaded once; callers meanwhile wait for the same load.
    private func readyPair() async throws -> CardPagePair? {
        if let pair, loaded { return pair }
        if let loading { return try await loading.value }
        let generation = generation
        let work = Task { () throws -> CardPagePair? in
            if pair == nil {
                guard let resolver = await resolver(), generation == self.generation else { return nil }
                // Card media never starts without a tap here (V12); links leave as in review.
                let made = CardPagePair(configuration: .preview, resolver: resolver, appearance: appearance)
                made.configureLinks(preferExternalBrowser: true, approvals: CardSchemeApprovals())
                pair = made
            }
            guard let pair else { return nil }
            try await pair.load()
            guard generation == self.generation else { return nil }
            loaded = true
            return pair
        }
        loading = work
        defer { if generation == self.generation { loading = nil } }
        return try await work.value
    }

    /// Waits for the render under way (tests).
    func settled() async {
        for _ in 0..<20 { await Task.yield() }
        await pending?.value
        for _ in 0..<20 { await Task.yield() }
    }
}

/// Where the Cards tab puts its preview (N22.3, N22.7).
public nonisolated enum NoteTypePreviewLayout {
    /// The card types column's width on the Mac (its ideal).
    public static let listWidth: CGFloat = 240
    /// The narrowest the code editor stays usable beside the preview.
    public static let editorMinimum: CGFloat = 360
    /// The preview's narrowest: its bar's controls and a card at its own size.
    public static let previewMinimum: CGFloat = 300
    public static let previewIdeal: CGFloat = 380

    /// Mac: the preview shows beside the editor when asked for and the tab
    /// is wide enough for the lists, a usable editor and the preview;
    /// narrower, it hides by itself and comes back when there is room.
    public static func macShows(requested: Bool, width: CGFloat) -> Bool {
        requested && width >= listWidth + editorMinimum + previewMinimum
    }
}
