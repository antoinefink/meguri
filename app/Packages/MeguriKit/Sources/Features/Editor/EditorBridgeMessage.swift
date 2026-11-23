import AppModel
import CoreFoundation
import Foundation

/// Validation is deliberately independent of WebKit delivery and draft
/// mutation. Reject extra keys, numeric booleans and unknown ordinals.
struct EditorBridgeMessage {
    let type: String
    let loadID: UInt64?
    let sequence: UInt64?
    let body: [String: Any]
    let commits: [NoteEditorDraft.Commit]

    init(_ value: Any, ordinals: Set<Int>) throws {
        guard let body = value as? [String: Any], let type = body["type"] as? String else {
            throw EditorRuntimeError.invalidMessage
        }
        let members: Set<String>
        switch type {
        case "ready": members = ["type", "protocol"]
        case "loaded": members = ["type", "loadId", "seq"]
        case "focus", "modeChanged": members = ["type", "loadId", "seq", "ord", "mode"]
        case "blur", "dirty": members = ["type", "loadId", "seq", "ord"]
        case "commit": members = ["type", "loadId", "seq", "ord", "html", "unchanged", "reason"]
        case "focusLeave": members = ["type", "loadId", "seq", "direction"]
        case "collapsedChanged": members = ["type", "loadId", "seq", "ord", "collapsed"]
        case "historyState": members = ["type", "loadId", "seq", "ord", "canUndo", "canRedo"]
        case "contentHeight": members = ["type", "loadId", "seq", "height"]
        case "flush": members = ["type", "loadId", "seq", "commits"]
        // Browse's inspector: where the caret is, and a media reference clicked.
        case "caret": members = ["type", "loadId", "seq", "top", "bottom"]
        case "playMedia", "openMedia": members = ["type", "loadId", "seq", "name"]
        default: throw EditorRuntimeError.invalidMessage
        }
        guard Set(body.keys) == members else { throw EditorRuntimeError.invalidMessage }
        self.body = body; self.type = type
        if type == "ready" {
            guard Self.integer(body["protocol"]) == 1 else { throw EditorRuntimeError.invalidMessage }
            loadID = nil; sequence = nil; commits = []; return
        }
        guard let load = Self.integer(body["loadId"]), load > 0,
              let sequence = Self.integer(body["seq"]), sequence > 0 else { throw EditorRuntimeError.invalidMessage }
        loadID = UInt64(load); self.sequence = UInt64(sequence)
        if members.contains("ord") {
            guard let ord = Self.integer(body["ord"]), ordinals.contains(ord) else { throw EditorRuntimeError.invalidMessage }
        }
        if members.contains("mode") {
            guard let mode = body["mode"] as? String, NoteEditorDraft.Mode(rawValue: mode) != nil else { throw EditorRuntimeError.invalidMessage }
        }
        if members.contains("collapsed"), Self.boolean(body["collapsed"]) == nil { throw EditorRuntimeError.invalidMessage }
        if members.contains("canUndo"), Self.boolean(body["canUndo"]) == nil || Self.boolean(body["canRedo"]) == nil {
            throw EditorRuntimeError.invalidMessage
        }
        if type == "focusLeave" {
            guard let direction = body["direction"] as? String, ["next", "previous"].contains(direction) else {
                throw EditorRuntimeError.invalidMessage
            }
        }
        if type == "caret" {
            guard let top = Self.number(body["top"]), let bottom = Self.number(body["bottom"]), top >= 0, bottom >= top else {
                throw EditorRuntimeError.invalidMessage
            }
        }
        if type == "playMedia" || type == "openMedia" {
            guard let name = body["name"] as? String, MediaName.plain(name) != nil else { throw EditorRuntimeError.invalidMessage }
        }
        if type == "contentHeight" {
            guard let height = Self.number(body["height"]), height >= 0 else { throw EditorRuntimeError.invalidMessage }
        }
        if type == "commit" {
            guard let reason = body["reason"] as? String, ["idle", "blur", "modeSwitch", "collapse"].contains(reason) else {
                throw EditorRuntimeError.invalidMessage
            }
            commits = [try Self.commit(body, ordinals: ordinals)]
        } else if type == "flush" {
            guard let values = body["commits"] as? [[String: Any]],
                  values.allSatisfy({ Set($0.keys) == ["ord", "html", "unchanged"] }) else { throw EditorRuntimeError.invalidMessage }
            commits = try values.map { try Self.commit($0, ordinals: ordinals) }
            guard commits.map(\.ord) == commits.map(\.ord).sorted(), Set(commits.map(\.ord)).count == commits.count else {
                throw EditorRuntimeError.invalidMessage
            }
        } else { commits = [] }
    }

    private static func commit(_ body: [String: Any], ordinals: Set<Int>) throws -> NoteEditorDraft.Commit {
        guard let ord = integer(body["ord"]), ordinals.contains(ord),
              let html = body["html"] as? String, let unchanged = boolean(body["unchanged"]) else {
            throw EditorRuntimeError.invalidMessage
        }
        return .init(ord: ord, html: html, unchanged: unchanged)
    }
    static func number(_ value: Any?) -> Double? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(), number.doubleValue.isFinite else { return nil }
        return number.doubleValue
    }
    static func integer(_ value: Any?) -> Int? {
        guard let value = number(value), value >= 0, value <= 9_007_199_254_740_991, value.rounded(.towardZero) == value else { return nil }
        return Int(value)
    }
    static func boolean(_ value: Any?) -> Bool? {
        guard let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else { return nil }
        return number.boolValue
    }
}

enum EditorRuntimeError: Error {
    case invalidMessage, unavailable, sequenceGap, processTerminated
}

/// A media reference as a field holds it, reduced to a plain file name of
/// the collection's media folder: no folder, no parent, nothing a page
/// could use to reach another file. Nil otherwise.
enum MediaName {
    static func plain(_ reference: String) -> String? {
        let name = reference.removingPercentEncoding ?? reference
        guard !name.isEmpty, name.count <= 255, !name.contains("/"), !name.contains("\\"), name != ".", name != "..",
              !name.hasPrefix("."), !name.unicodeScalars.contains(where: { $0.value < 0x20 }) else { return nil }
        return name
    }
}
