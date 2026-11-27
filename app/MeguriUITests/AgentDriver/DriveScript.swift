import Foundation

/// An agent drive script (U8, R1965): a JSON object with a name and a list of
/// steps, run by `AgentDriverTests` under `tools/mg drive`. The format is
/// documented in tools/drive-scripts/AGENTS.md. Each step object holds exactly
/// one action key, plus an optional `transition` name (lowercase letters,
/// digits and '-') that marks the step for motion capture.
struct DriveScript: Equatable {
    struct Launch: Equatable {
        /// Relative to the repository root, or absolute.
        var fixture: String?
        var now: String?
        var timeZone: String?
        var locale: String?
        var animations: Bool?
        var reduceMotion: Bool?
        var reduceTransparency: Bool?
        var windowSize: String?
        var ankiConnectPort: Int?
        var migakuPort: Int?
        var route: String?
        /// Extra raw launch arguments, appended last.
        var arguments: [String] = []
    }

    enum Direction: String, CaseIterable {
        case up, down, left, right
    }

    enum Action: Equatable {
        case launch(Launch)
        case tap(id: String)
        case type(id: String, text: String)
        case swipe(id: String, direction: Direction)
        case scroll(id: String, direction: Direction)
        case wait(id: String, timeout: TimeInterval, gone: Bool)
        case waitActivity(label: String, timeout: TimeInterval)
        case snapshot(name: String)
        /// Mac: `minimum`, `default`, `large` or `<width>x<height>`.
        case resize(size: String)
        /// iOS: Home then activate the same still-running app.
        case foregroundCycle
        /// `<op>:<argument>`, for controls with no unique identifier and
        /// for device state (`customOps`; tools/drive-scripts/AGENTS.md).
        case custom(String)

        var kind: String {
            switch self {
            case .launch: "launch"
            case .tap: "tap"
            case .type: "type"
            case .swipe: "swipe"
            case .scroll: "scroll"
            case .wait, .waitActivity: "wait"
            case .snapshot: "snapshot"
            case .resize: "resize"
            case .foregroundCycle: "foregroundCycle"
            case .custom: "do"
            }
        }
    }

    struct Step: Equatable {
        var action: Action
        var transition: String?
    }

    var name: String
    var steps: [Step]

    static let customOps = ["label", "press", "pressLabel", "keys", "point", "sleep", "orientation", "appearance"]
    static let actionKeys = ["launch", "tap", "type", "swipe", "scroll", "wait", "snapshot", "resize", "foregroundCycle", "do"]
    static let defaultWaitTimeout: TimeInterval = 10

    init(name: String, steps: [Step]) {
        self.name = name
        self.steps = steps
    }

    init(contentsOf url: URL) throws(DriveScriptError) {
        let data: Data
        do {
            data = try Data(contentsOf: url)
        } catch {
            throw DriveScriptError("cannot read \(url.path(percentEncoded: false)): \(error.localizedDescription)")
        }
        try self.init(json: data)
    }

    init(json data: Data) throws(DriveScriptError) {
        let object: Any
        do {
            object = try JSONSerialization.jsonObject(with: data)
        } catch {
            throw DriveScriptError("the script is not JSON: \(error.localizedDescription)")
        }
        guard let root = object as? [String: Any] else {
            throw DriveScriptError("the script must be a JSON object with \"name\" and \"steps\"")
        }
        let unknown = Set(root.keys).subtracting(["name", "description", "platforms", "steps"])
        guard unknown.isEmpty else {
            throw DriveScriptError("unknown top-level key(s): \(unknown.sorted().joined(separator: ", "))")
        }
        guard let name = root["name"] as? String, !name.isEmpty else {
            throw DriveScriptError("\"name\" must be a non-empty string")
        }
        guard let rawSteps = root["steps"] as? [Any], !rawSteps.isEmpty else {
            throw DriveScriptError("\"steps\" must be a non-empty array")
        }
        var steps: [Step] = []
        for (offset, raw) in rawSteps.enumerated() {
            steps.append(try Self.step(raw, index: offset + 1))
        }
        guard case .launch = steps[0].action else {
            throw DriveScriptError("step 1 must be a launch step, found \(steps[0].action.kind)")
        }
        self.init(name: name, steps: steps)
    }

    // MARK: Parsing

    private static func step(_ raw: Any, index: Int) throws(DriveScriptError) -> Step {
        let at = "step \(index)"
        guard let stepObject = raw as? [String: Any] else {
            throw DriveScriptError("\(at): a step must be a JSON object")
        }
        let keys = Set(stepObject.keys)
        let actions = keys.intersection(actionKeys)
        let unknown = keys.subtracting(actionKeys).subtracting(["transition"])
        guard unknown.isEmpty else {
            throw DriveScriptError("\(at): unknown key(s) \(unknown.sorted().joined(separator: ", ")); actions: \(actionKeys.joined(separator: ", "))")
        }
        guard actions.count == 1, let key = actions.first, let value = stepObject[key] else {
            throw DriveScriptError("\(at): needs exactly one action key (\(actionKeys.joined(separator: ", "))), found \(actions.sorted())")
        }
        var transition: String?
        if let raw = stepObject["transition"] {
            guard let name = raw as? String, !name.isEmpty else {
                throw DriveScriptError("\(at): \"transition\" must be a non-empty string")
            }
            // It names the step's contact sheet and clip files.
            guard name.wholeMatch(of: /[a-z0-9][a-z0-9-]*/) != nil else {
                throw DriveScriptError("\(at): transition names use lowercase letters, digits and '-', got '\(name)'")
            }
            transition = name
        }
        let action: Action
        switch key {
        case "launch":
            action = .launch(try launch(value, at))
        case "tap":
            action = .tap(id: try identifier(value, at, key))
        case "type":
            let fields = try object(value, at, key, required: ["id", "text"], optional: [])
            action = .type(id: try string(fields["id"], at, "type.id"), text: try string(fields["text"], at, "type.text", allowEmpty: true))
        case "swipe", "scroll":
            let fields = try object(value, at, key, required: ["id", "direction"], optional: [])
            let id = try string(fields["id"], at, "\(key).id")
            let raw = try string(fields["direction"], at, "\(key).direction")
            guard let direction = Direction(rawValue: raw) else {
                throw DriveScriptError("\(at): \(key).direction must be one of \(Direction.allCases.map(\.rawValue).joined(separator: ", ")), got '\(raw)'")
            }
            action = key == "swipe" ? .swipe(id: id, direction: direction) : .scroll(id: id, direction: direction)
        case "wait":
            if let id = value as? String {
                action = .wait(id: try identifier(id, at, key), timeout: defaultWaitTimeout, gone: false)
            } else if let fields = value as? [String: Any], fields["activity"] != nil {
                let fields = try object(value, at, key, required: ["activity"], optional: ["timeout"])
                let timeout = try number(fields["timeout"], at, "wait.timeout") ?? defaultWaitTimeout
                guard timeout > 0 else { throw DriveScriptError("\(at): wait.timeout must be positive") }
                action = .waitActivity(label: try string(fields["activity"], at, "wait.activity"), timeout: timeout)
            } else {
                let fields = try object(value, at, key, required: ["id"], optional: ["timeout", "gone"])
                let timeout = try number(fields["timeout"], at, "wait.timeout") ?? defaultWaitTimeout
                guard timeout > 0 else { throw DriveScriptError("\(at): wait.timeout must be positive") }
                action = .wait(id: try string(fields["id"], at, "wait.id"), timeout: timeout, gone: try bool(fields["gone"], at, "wait.gone") ?? false)
            }
        case "snapshot":
            let name = try string(value, at, key)
            guard name.wholeMatch(of: /[a-z0-9][a-z0-9-]*/) != nil else {
                throw DriveScriptError("\(at): snapshot names use lowercase letters, digits and '-', got '\(name)'")
            }
            action = .snapshot(name: name)
        case "do":
            let command = try string(value, at, key)
            let parts = command.split(separator: ":", maxSplits: 1).map(String.init)
            guard parts.count == 2, customOps.contains(parts[0]), !parts[1].isEmpty else {
                throw DriveScriptError("\(at): do takes <op>:<argument> with op one of \(customOps.joined(separator: ", ")), got '\(command)'")
            }
            action = .custom(command)
        case "foregroundCycle":
            guard keys == ["foregroundCycle"], isBoolean(value), value as? Bool == true else {
                throw DriveScriptError("\(at): foregroundCycle takes only literal true and no other keys")
            }
            action = .foregroundCycle
        default: // resize
            let size = try string(value, at, key)
            guard ["minimum", "default", "large"].contains(size) || size.wholeMatch(of: /[1-9][0-9]*x[1-9][0-9]*/) != nil else {
                throw DriveScriptError("\(at): resize takes minimum, default, large or <width>x<height>, got '\(size)'")
            }
            action = .resize(size: size)
        }
        return Step(action: action, transition: transition)
    }

    private static func launch(_ value: Any, _ at: String) throws(DriveScriptError) -> Launch {
        let names = [
            "fixture", "now", "timeZone", "locale", "animations", "reduceMotion", "reduceTransparency",
            "windowSize", "ankiConnectPort", "migakuPort", "route", "arguments",
        ]
        let fields = try object(value, at, "launch", required: [], optional: names)
        var launch = Launch()
        launch.fixture = try optionalString(fields["fixture"], at, "launch.fixture")
        launch.now = try optionalString(fields["now"], at, "launch.now")
        launch.timeZone = try optionalString(fields["timeZone"], at, "launch.timeZone")
        launch.locale = try optionalString(fields["locale"], at, "launch.locale")
        launch.animations = try bool(fields["animations"], at, "launch.animations")
        launch.reduceMotion = try bool(fields["reduceMotion"], at, "launch.reduceMotion")
        launch.reduceTransparency = try bool(fields["reduceTransparency"], at, "launch.reduceTransparency")
        launch.windowSize = try optionalString(fields["windowSize"], at, "launch.windowSize")
        launch.ankiConnectPort = try integer(fields["ankiConnectPort"], at, "launch.ankiConnectPort")
        launch.migakuPort = try integer(fields["migakuPort"], at, "launch.migakuPort")
        launch.route = try optionalString(fields["route"], at, "launch.route")
        if let raw = fields["arguments"] {
            guard let arguments = raw as? [String] else {
                throw DriveScriptError("\(at): launch.arguments must be an array of strings")
            }
            launch.arguments = arguments
        }
        return launch
    }

    private static func object(
        _ value: Any, _ at: String, _ key: String, required: [String], optional: [String]
    ) throws(DriveScriptError) -> [String: Any] {
        guard let fields = value as? [String: Any] else {
            throw DriveScriptError("\(at): \(key) must be a JSON object")
        }
        let unknown = Set(fields.keys).subtracting(required + optional)
        guard unknown.isEmpty else {
            throw DriveScriptError("\(at): \(key) has unknown key(s) \(unknown.sorted().joined(separator: ", "))")
        }
        let missing = required.filter { fields[$0] == nil }
        guard missing.isEmpty else {
            throw DriveScriptError("\(at): \(key) needs \(missing.joined(separator: ", "))")
        }
        return fields
    }

    private static func identifier(_ value: Any, _ at: String, _ key: String) throws(DriveScriptError) -> String {
        guard let id = value as? String else {
            throw DriveScriptError("\(at): \(key) takes an accessibility identifier string")
        }
        return try string(id, at, key)
    }

    private static func string(_ value: Any?, _ at: String, _ key: String, allowEmpty: Bool = false) throws(DriveScriptError) -> String {
        guard let string = value as? String, allowEmpty || !string.isEmpty else {
            throw DriveScriptError("\(at): \(key) must be a\(allowEmpty ? "" : " non-empty") string")
        }
        return string
    }

    private static func optionalString(_ value: Any?, _ at: String, _ key: String) throws(DriveScriptError) -> String? {
        guard let value else { return nil }
        return try string(value, at, key)
    }

    /// JSONSerialization reads `true` and `1` alike as NSNumber; booleans
    /// must be JSON booleans and numbers must not be.
    private static func isBoolean(_ value: Any) -> Bool {
        (value as? NSNumber).map { CFGetTypeID($0) == CFBooleanGetTypeID() } ?? false
    }

    private static func bool(_ value: Any?, _ at: String, _ key: String) throws(DriveScriptError) -> Bool? {
        guard let value else { return nil }
        guard isBoolean(value), let bool = value as? Bool else {
            throw DriveScriptError("\(at): \(key) must be true or false")
        }
        return bool
    }

    private static func number(_ value: Any?, _ at: String, _ key: String) throws(DriveScriptError) -> Double? {
        guard let value else { return nil }
        guard !isBoolean(value), let number = value as? NSNumber else {
            throw DriveScriptError("\(at): \(key) must be a number")
        }
        return number.doubleValue
    }

    private static func integer(_ value: Any?, _ at: String, _ key: String) throws(DriveScriptError) -> Int? {
        guard let double = try number(value, at, key) else { return nil }
        guard double == double.rounded(), let integer = Int(exactly: double) else {
            throw DriveScriptError("\(at): \(key) must be a whole number")
        }
        return integer
    }
}

struct DriveScriptError: Error, Equatable, CustomStringConvertible {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    var description: String { message }
}
