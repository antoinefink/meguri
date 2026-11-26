import Foundation
import Instrumentation
internal import MeguriCoreBindings

public enum StockNotetype: Sendable, Hashable {
    case basic
    case basicAndReversed
    case basicOptionalReversed
    case basicTyping
    case cloze
    case imageOcclusion
    init(_ value: MeguriCoreBindings.StockNotetype) {
        switch value {
        case .basic: self = .basic
        case .basicAndReversed: self = .basicAndReversed
        case .basicOptionalReversed: self = .basicOptionalReversed
        case .basicTyping: self = .basicTyping
        case .cloze: self = .cloze
        case .imageOcclusion: self = .imageOcclusion
        }
    }
    var raw: MeguriCoreBindings.StockNotetype {
        switch self {
        case .basic: .basic
        case .basicAndReversed: .basicAndReversed
        case .basicOptionalReversed: .basicOptionalReversed
        case .basicTyping: .basicTyping
        case .cloze: .cloze
        case .imageOcclusion: .imageOcclusion
        }
    }
}

public struct NewNotetypeTemplate: Sendable, Hashable {
    public let name: String
    public let front: String
    public let back: String
    public let browserFront: String
    public let browserBack: String
    public init(name: String, front: String, back: String, browserFront: String, browserBack: String) {
        self.name = name
        self.front = front
        self.back = back
        self.browserFront = browserFront
        self.browserBack = browserBack
    }
    init(_ value: MeguriCoreBindings.NewNotetypeTemplate) {
        name = value.name
        front = value.front
        back = value.back
        browserFront = value.browserFront
        browserBack = value.browserBack
    }
    var raw: MeguriCoreBindings.NewNotetypeTemplate { .init(name: name, front: front, back: back, browserFront: browserFront, browserBack: browserBack) }
}

public struct AddNotetypeRequest: Sendable, Hashable {
    public let name: String
    public let cloze: Bool
    public let fields: [String]
    public let templates: [NewNotetypeTemplate]
    public let css: String?
    public init(name: String, cloze: Bool, fields: [String], templates: [NewNotetypeTemplate], css: String?) {
        self.name = name
        self.cloze = cloze
        self.fields = fields
        self.templates = templates
        self.css = css
    }
    init(_ value: MeguriCoreBindings.AddNotetypeRequest) {
        name = value.name
        cloze = value.cloze
        fields = value.fields
        templates = value.templates.map(NewNotetypeTemplate.init)
        css = value.css
    }
    var raw: MeguriCoreBindings.AddNotetypeRequest { .init(name: name, cloze: cloze, fields: fields, templates: templates.map(\.raw), css: css) }
}

public struct NotetypeMutation: Sendable, Hashable {
    public let summary: MutationSummary
    public let id: Int64
    public let name: String
    public let written: Bool
    public init(summary: MutationSummary, id: Int64, name: String, written: Bool) {
        self.summary = summary
        self.id = id
        self.name = name
        self.written = written
    }
    init(_ value: MeguriCoreBindings.NotetypeMutation) {
        summary = MutationSummary(value.summary)
        id = value.id
        name = value.name
        written = value.written
    }
}

public struct NotetypeField: Sendable, Hashable {
    public let ord: UInt32
    public let tag: UInt32?
    public let id: Int64?
    public let name: String
    public let sticky: Bool
    public let rtl: Bool
    public let fontName: String
    public let fontSize: UInt32
    public let description: String
    public let plainText: Bool
    public let collapsed: Bool
    public let excludeFromSearch: Bool
    /// Protected from deletion (a Cloze or Image Occlusion field).
    public let preventDeletion: Bool
    public init(ord: UInt32, id: Int64?, name: String, sticky: Bool, rtl: Bool, fontName: String, fontSize: UInt32, description: String, plainText: Bool, collapsed: Bool, excludeFromSearch: Bool, preventDeletion: Bool = false, tag: UInt32? = nil) {
        self.tag = tag
        self.preventDeletion = preventDeletion
        self.ord = ord
        self.id = id
        self.name = name
        self.sticky = sticky
        self.rtl = rtl
        self.fontName = fontName
        self.fontSize = fontSize
        self.description = description
        self.plainText = plainText
        self.collapsed = collapsed
        self.excludeFromSearch = excludeFromSearch
    }
    init(_ value: MeguriCoreBindings.NotetypeField) {
        tag = value.tag
        ord = value.ord
        id = value.id
        name = value.name
        sticky = value.sticky
        rtl = value.rtl
        fontName = value.fontName
        fontSize = value.fontSize
        description = value.description
        plainText = value.plainText
        collapsed = value.collapsed
        excludeFromSearch = value.excludeFromSearch
        preventDeletion = value.preventDeletion
    }
}

public struct NotetypeTemplate: Sendable, Hashable {
    public let ord: UInt32
    public let id: Int64?
    public let name: String
    public let front: String
    public let back: String
    public let qFormatBrowser: String
    public let aFormatBrowser: String
    public let targetDeckId: Int64
    public let browserFontName: String
    public let browserFontSize: UInt32
    public init(ord: UInt32, id: Int64?, name: String, front: String, back: String, qFormatBrowser: String, aFormatBrowser: String, targetDeckId: Int64, browserFontName: String, browserFontSize: UInt32) {
        self.ord = ord
        self.id = id
        self.name = name
        self.front = front
        self.back = back
        self.qFormatBrowser = qFormatBrowser
        self.aFormatBrowser = aFormatBrowser
        self.targetDeckId = targetDeckId
        self.browserFontName = browserFontName
        self.browserFontSize = browserFontSize
    }
    init(_ value: MeguriCoreBindings.NotetypeTemplate) {
        ord = value.ord
        id = value.id
        name = value.name
        front = value.front
        back = value.back
        qFormatBrowser = value.qFormatBrowser
        aFormatBrowser = value.aFormatBrowser
        targetDeckId = value.targetDeckId
        browserFontName = value.browserFontName
        browserFontSize = value.browserFontSize
    }
}

public struct NotetypeRecord: Sendable, Hashable {
    public let id: Int64
    public let name: String
    public let kind: Int32
    public let originalStockKind: Int32
    /// Logical field positions; definitions retain their raw stored ordinals.
    public let clozeFields: [UInt32]
    public let sortField: UInt32
    public let css: String
    public let base: [UInt8]
    public let fields: [NotetypeField]
    public let templates: [NotetypeTemplate]
    public init(id: Int64, name: String, kind: Int32, sortField: UInt32, css: String, base: [UInt8], fields: [NotetypeField], templates: [NotetypeTemplate], originalStockKind: Int32 = 0, clozeFields: [UInt32] = []) {
        self.id = id
        self.name = name
        self.kind = kind
        self.originalStockKind = originalStockKind
        self.clozeFields = clozeFields
        self.sortField = sortField
        self.css = css
        self.base = base
        self.fields = fields
        self.templates = templates
    }
    init(_ value: MeguriCoreBindings.NotetypeRecord) {
        id = value.id
        name = value.name
        kind = value.kind
        originalStockKind = value.originalStockKind
        clozeFields = value.clozeFields
        sortField = value.sortField
        css = value.css
        base = Array(value.base)
        fields = value.fields.map(NotetypeField.init)
        templates = value.templates.map(NotetypeTemplate.init)
    }
}

public struct NotetypeName: Sendable, Hashable {
    public let id: Int64
    public let name: String
    public init(id: Int64, name: String) {
        self.id = id
        self.name = name
    }
    init(_ value: MeguriCoreBindings.NotetypeName) {
        id = value.id
        name = value.name
    }
}

public struct NotetypeFieldOptions: Sendable, Hashable {
    public let notetypeId: Int64
    public let base: [UInt8]
    public let ord: UInt32
    public let id: Int64?
    public let sticky: Bool
    public let rtl: Bool
    public let fontName: String
    public let fontSize: UInt32
    public let description: String
    public let plainText: Bool
    public let collapsed: Bool
    public let excludeFromSearch: Bool
    public init(notetypeId: Int64, base: [UInt8], ord: UInt32, id: Int64?, sticky: Bool, rtl: Bool, fontName: String, fontSize: UInt32, description: String, plainText: Bool, collapsed: Bool, excludeFromSearch: Bool) {
        self.notetypeId = notetypeId
        self.base = base
        self.ord = ord
        self.id = id
        self.sticky = sticky
        self.rtl = rtl
        self.fontName = fontName
        self.fontSize = fontSize
        self.description = description
        self.plainText = plainText
        self.collapsed = collapsed
        self.excludeFromSearch = excludeFromSearch
    }
    init(_ value: MeguriCoreBindings.NotetypeFieldOptions) {
        notetypeId = value.notetypeId
        base = Array(value.base)
        ord = value.ord
        id = value.id
        sticky = value.sticky
        rtl = value.rtl
        fontName = value.fontName
        fontSize = value.fontSize
        description = value.description
        plainText = value.plainText
        collapsed = value.collapsed
        excludeFromSearch = value.excludeFromSearch
    }
    var raw: MeguriCoreBindings.NotetypeFieldOptions { .init(notetypeId: notetypeId, base: Data(base), ord: ord, id: id, sticky: sticky, rtl: rtl, fontName: fontName, fontSize: fontSize, description: description, plainText: plainText, collapsed: collapsed, excludeFromSearch: excludeFromSearch) }
}

public struct NotetypeTemplateOptions: Sendable, Hashable {
    public let notetypeId: Int64
    public let base: [UInt8]
    public let ord: UInt32
    public let id: Int64?
    public let qFormatBrowser: String
    public let aFormatBrowser: String
    public let targetDeckId: Int64
    public let browserFontName: String
    public let browserFontSize: UInt32
    public init(notetypeId: Int64, base: [UInt8], ord: UInt32, id: Int64?, qFormatBrowser: String, aFormatBrowser: String, targetDeckId: Int64, browserFontName: String, browserFontSize: UInt32) {
        self.notetypeId = notetypeId
        self.base = base
        self.ord = ord
        self.id = id
        self.qFormatBrowser = qFormatBrowser
        self.aFormatBrowser = aFormatBrowser
        self.targetDeckId = targetDeckId
        self.browserFontName = browserFontName
        self.browserFontSize = browserFontSize
    }
    init(_ value: MeguriCoreBindings.NotetypeTemplateOptions) {
        notetypeId = value.notetypeId
        base = Array(value.base)
        ord = value.ord
        id = value.id
        qFormatBrowser = value.qFormatBrowser
        aFormatBrowser = value.aFormatBrowser
        targetDeckId = value.targetDeckId
        browserFontName = value.browserFontName
        browserFontSize = value.browserFontSize
    }
    var raw: MeguriCoreBindings.NotetypeTemplateOptions { .init(notetypeId: notetypeId, base: Data(base), ord: ord, id: id, qFormatBrowser: qFormatBrowser, aFormatBrowser: aFormatBrowser, targetDeckId: targetDeckId, browserFontName: browserFontName, browserFontSize: browserFontSize) }
}

public protocol NotetypeService: Sendable {
    func notetypeNames() async throws(BridgeError) -> [NotetypeName]
    func readNotetype(id: Int64) async throws(BridgeError) -> NotetypeRecord
    func addNotetype(_ request: AddNotetypeRequest) async throws(BridgeError) -> NotetypeMutation
    func addStockNotetype(kind: StockNotetype, name: String?) async throws(BridgeError) -> NotetypeMutation
    func cloneNotetype(id: Int64, name: String?) async throws(BridgeError) -> NotetypeMutation
    func ensureImageOcclusionNotetype() async throws(BridgeError) -> NotetypeMutation
    func renameNotetype(id: Int64, base: [UInt8], name: String) async throws(BridgeError) -> NotetypeMutation
    func setNotetypeCss(id: Int64, base: [UInt8], css: String) async throws(BridgeError) -> NotetypeMutation
    func setNotetypeFieldOptions(_ request: NotetypeFieldOptions) async throws(BridgeError) -> NotetypeMutation
    func setNotetypeTemplateOptions(_ request: NotetypeTemplateOptions) async throws(BridgeError) -> NotetypeMutation
}

public actor CoreNotetypeService: NotetypeService {
    public init() {}
    public func ensureImageOcclusionNotetype(expectedGeneration: UInt64) async throws(BridgeError) -> NotetypeMutation {
        try await InteractiveLane.shared.call(.editorEnsureImageOcclusionNotetype) {
            NotetypeMutation(try MeguriCoreBindings.editorEnsureImageOcclusionNotetype(expectedGeneration: expectedGeneration))
        }
    }
    public func notetypeNames() async throws(BridgeError) -> [NotetypeName] {
        try await MaintenanceLane.shared.call(.notetypeNames) { try MeguriCoreBindings.notetypeNames().map(NotetypeName.init) }
    }
    public func readNotetype(id: Int64) async throws(BridgeError) -> NotetypeRecord {
        try await QueryLane.shared.call(.readNotetype) { NotetypeRecord(try MeguriCoreBindings.readNotetype(id: id)) }
    }
    public func addNotetype(_ request: AddNotetypeRequest) async throws(BridgeError) -> NotetypeMutation {
        try await InteractiveLane.shared.call(.addNotetype) { NotetypeMutation(try MeguriCoreBindings.addNotetype(request: request.raw)) }
    }
    public func addStockNotetype(kind: StockNotetype, name: String?) async throws(BridgeError) -> NotetypeMutation {
        try await InteractiveLane.shared.call(.addStockNotetype) { NotetypeMutation(try MeguriCoreBindings.addStockNotetype(kind: kind.raw, name: name)) }
    }
    public func cloneNotetype(id: Int64, name: String?) async throws(BridgeError) -> NotetypeMutation {
        try await InteractiveLane.shared.call(.cloneNotetype) { NotetypeMutation(try MeguriCoreBindings.cloneNotetype(id: id, name: name)) }
    }
    public func ensureImageOcclusionNotetype() async throws(BridgeError) -> NotetypeMutation {
        try await InteractiveLane.shared.call(.ensureImageOcclusionNotetype) { NotetypeMutation(try MeguriCoreBindings.ensureImageOcclusionNotetype()) }
    }
    public func renameNotetype(id: Int64, base: [UInt8], name: String) async throws(BridgeError) -> NotetypeMutation {
        try await InteractiveLane.shared.call(.renameNotetype) { NotetypeMutation(try MeguriCoreBindings.renameNotetype(id: id, base: Data(base), name: name)) }
    }
    public func setNotetypeCss(id: Int64, base: [UInt8], css: String) async throws(BridgeError) -> NotetypeMutation {
        try await InteractiveLane.shared.call(.setNotetypeCss) { NotetypeMutation(try MeguriCoreBindings.setNotetypeCss(id: id, base: Data(base), css: css)) }
    }
    public func setNotetypeFieldOptions(_ request: NotetypeFieldOptions) async throws(BridgeError) -> NotetypeMutation {
        try await InteractiveLane.shared.call(.setNotetypeFieldOptions) { NotetypeMutation(try MeguriCoreBindings.setNotetypeFieldOptions(request: request.raw)) }
    }
    public func setNotetypeTemplateOptions(_ request: NotetypeTemplateOptions) async throws(BridgeError) -> NotetypeMutation {
        try await InteractiveLane.shared.call(.setNotetypeTemplateOptions) { NotetypeMutation(try MeguriCoreBindings.setNotetypeTemplateOptions(request: request.raw)) }
    }
}
