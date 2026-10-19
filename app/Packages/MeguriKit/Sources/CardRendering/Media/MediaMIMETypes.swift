import Foundation

public nonisolated enum MediaMIMETypes {
    public static func type(for name: String) -> String {
        let ext = (name as NSString).pathExtension.lowercased()
        return types[ext] ?? "application/octet-stream"
    }
    public static func blocksScripts(_ mime: String) -> Bool {
        mime.hasPrefix("text/html") || mime.hasPrefix("application/xhtml+xml") || mime == "image/svg+xml"
    }
    private static let types = [
        "html": "text/html; charset=utf-8", "htm": "text/html; charset=utf-8", "xhtml": "application/xhtml+xml; charset=utf-8",
        "svg": "image/svg+xml", "css": "text/css; charset=utf-8", "js": "text/javascript; charset=utf-8", "mjs": "text/javascript; charset=utf-8",
        "json": "application/json; charset=utf-8", "txt": "text/plain; charset=utf-8", "png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg",
        "gif": "image/gif", "webp": "image/webp", "avif": "image/avif", "ttf": "font/ttf", "otf": "font/otf", "woff": "font/woff", "woff2": "font/woff2",
        "mp3": "audio/mpeg", "m4a": "audio/mp4", "aac": "audio/aac", "wav": "audio/wav", "flac": "audio/flac", "mp4": "video/mp4", "m4v": "video/mp4",
        "mov": "video/quicktime", "3gp": "video/3gpp", "ogg": "audio/ogg", "oga": "audio/ogg", "opus": "audio/ogg", "spx": "audio/ogg", "ogv": "video/ogg", "webm": "video/webm",
    ]
}
