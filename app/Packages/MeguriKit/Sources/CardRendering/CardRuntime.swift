import Foundation

/// The card runtime (U66, specs/render/card-runtime.md): the page document,
/// the base stylesheet and the scripts every card page loads. Page surfaces
/// (U67's page pair, previews) build their pages from this and serve its
/// assets under `/_anki/`.
///
/// Everything here is plain data, so the media handler can serve it off the
/// main thread (R1857).
public nonisolated enum CardRuntime {
    /// The device class a page renders for. It decides the `<html>` and
    /// `<body>` platform classes (R475, R476) and which desktop globals the
    /// page world gets (R505, R513).
    public enum Platform: String, Sendable, CaseIterable {
        case mac
        /// Every iPhone, the foldable included in both states (R475).
        case iPhone = "iphone"
        case iPad = "ipad"
    }

    /// The app's appearance, which is the card's night mode (R488).
    public enum Appearance: String, Sendable, CaseIterable {
        case light
        case dark
    }

    /// The path prefix of the bundled runtime assets on the media origin
    /// (R496).
    public static let assetPrefix = "/_anki/"

    /// The content world `interactionScript` runs in. Card scripts cannot
    /// reach it (KTD11).
    public static let interactionWorldName = "meguri.card"

    /// The handler, in the interaction world only, that receives the
    /// runtime's signals (C14): `{signal, side?, card?, generation?, ...}`.
    public static let signalHandlerName = "meguriCard"

    /// The handler, in the interaction world only, that receives U12's
    /// visible confirmations (`VisibleConfirmation.messageName`): the
    /// `settled` and `visible` signals again, as `{token, card, phase}`.
    public static let confirmationHandlerName = "meguriVisible"

    /// AnkiMobile's handler. Native registers it in the page world on iOS
    /// and iPadOS only (C9.3, R511).
    public static let ankiMobileHandlerName = "cb"

    // MARK: Classes

    /// The `<html>` classes (C2.2, R475): `night-mode` first in dark
    /// appearance, then the platform classes.
    public static func htmlClasses(platform: Platform, appearance: Appearance) -> String {
        let platformClasses: [String] =
            switch platform {
            case .mac: ["mac", "safari"]
            case .iPhone: ["iphone", "ios", "mobile", "safari"]
            case .iPad: ["ipad", "ios", "mobile", "safari"]
            }
        return ((appearance == .dark ? ["night-mode"] : []) + platformClasses).joined(separator: " ")
    }

    /// The `<body>` classes (C3.1, R476) for the card whose template has
    /// ordinal `cardOrdinal` (0 for the first template). Without an ordinal,
    /// the classes a page starts with before its first side (C2.2).
    public static func bodyClasses(platform: Platform, appearance: Appearance, cardOrdinal: Int? = nil) -> String {
        var classes: [String] = []
        if let cardOrdinal { classes += ["card", "card\(cardOrdinal + 1)"] }
        if platform == .mac { classes.append("isMac") }
        if appearance == .dark {
            classes += ["nightMode", "night_mode"]
            if platform == .mac { classes.append("macos-dark-mode") }
        }
        return classes.joined(separator: " ")
    }

    // MARK: The page document

    /// The page document (C2.1): the skeleton with its classes, the base
    /// element for `mediaRoot` (the media origin's root, ending in `/`), the
    /// base stylesheet and the runtime, and `reviewExtra` after `#qa`
    /// (C2.3). `reviewExtra` is HTML and goes in as it is.
    public static func pageDocument(
        platform: Platform, appearance: Appearance, mediaRoot: URL, reviewExtra: String = ""
    ) -> String {
        let replacements = [
            "{{html-classes}}": htmlClasses(platform: platform, appearance: appearance),
            "{{theme}}": appearance.rawValue,
            "{{base}}": attributeEscaped(mediaRoot.absoluteString),
            "{{platform}}": platform.rawValue,
            "{{body-classes}}": bodyClasses(platform: platform, appearance: appearance),
        ]
        var document = Resources.text("page.html")
        for (marker, value) in replacements {
            document = document.replacingOccurrences(of: marker, with: value)
        }
        // Last, so that markers inside the template's own HTML stay as the
        // collection holds them.
        return document.replacingOccurrences(of: "{{review-extra}}", with: reviewExtra)
    }

    // MARK: Assets

    /// A bundled runtime asset.
    public struct Asset: Sendable {
        public let data: Data
        public let mimeType: String
    }

    /// The asset served at `path` under `assetPrefix` (`"runtime.js"` for
    /// `/_anki/runtime.js`), or nil when there is none.
    public static func asset(at path: String) -> Asset? {
        guard let (file, mimeType) = assets[path] else { return nil }
        return Asset(data: Resources.data(file), mimeType: mimeType)
    }

    /// The paths `asset(at:)` serves.
    public static var assetPaths: [String] { assets.keys.sorted() }

    private static let assets: [String: (file: String, mimeType: String)] = [
        "base.css": ("base.css", "text/css"),
        "runtime.js": ("runtime.js", "text/javascript"),
        "jquery-3.7.1.min.js": ("jquery-3.7.1.min.js", "text/javascript"),
    ]

    /// The isolated-world script (C14). Native adds it to every card page at
    /// document start, main frame only, in `interactionWorldName`, with
    /// `signalHandlerName` and `confirmationHandlerName` registered for that
    /// world.
    public static var interactionScript: String { Resources.text("interaction.js") }

    private static func attributeEscaped(_ text: String) -> String {
        text.replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "<", with: "&lt;")
    }

    /// The files under `Resources/CardRuntime/`, copied flat into the
    /// module's bundle.
    private enum Resources {
        static func data(_ name: String) -> Data {
            let dot = name.lastIndex(of: ".")!
            guard
                let url = Bundle.module.url(
                    forResource: String(name[..<dot]), withExtension: String(name[name.index(after: dot)...])),
                let data = try? Data(contentsOf: url)
            else {
                // A missing resource is a packaging error, never a runtime
                // condition.
                preconditionFailure("CardRuntime resource \(name) is missing from the bundle")
            }
            return data
        }

        static func text(_ name: String) -> String {
            String(decoding: data(name), as: UTF8.self)
        }
    }
}
