// Scratch helper: inspect and press controls of a background app through AX,
// without moving the cursor or activating the app.
//   mgax trusted | windows <pid> | tree <pid> [depth] | press <pid> <identifier-or-title> | menu <pid> <Menu> <Item>
import AppKit
import ApplicationServices

let args = CommandLine.arguments
func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
    var v: AnyObject?
    return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil
}
func str(_ e: AXUIElement, _ name: String) -> String { (attr(e, name) as? String) ?? "" }
func kids(_ e: AXUIElement) -> [AXUIElement] { (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] }
func walk(_ e: AXUIElement, depth: Int = 0, limit: Int = 40, _ visit: (AXUIElement, Int) -> Bool) -> Bool {
    if visit(e, depth) { return true }
    if depth >= limit { return false }
    for k in kids(e) where walk(k, depth: depth + 1, limit: limit, visit) { return true }
    return false
}
switch args.dropFirst().first {
case "trusted":
    print(AXIsProcessTrusted())
case "windows":
    let pid = Int32(args[2])!
    let list = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
    for w in list where (w[kCGWindowOwnerPID as String] as? Int32) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
        print(w[kCGWindowNumber as String] ?? "", w[kCGWindowName as String] ?? "", w[kCGWindowBounds as String] ?? "", w[kCGWindowIsOnscreen as String] ?? "")
    }
case "tree":
    let app = AXUIElementCreateApplication(Int32(args[2])!)
    let limit = args.count > 3 ? Int(args[3])! : 12
    for win in (attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? [] {
        _ = walk(win, limit: limit) { e, d in
            let role = str(e, kAXRoleAttribute), id = str(e, "AXIdentifier"), t = str(e, kAXTitleAttribute), desc = str(e, kAXDescriptionAttribute), val = (attr(e, kAXValueAttribute) as? String) ?? ""
            print(String(repeating: " ", count: d) + role, id.isEmpty ? "" : "#\(id)", t.isEmpty ? "" : "t=\(t)", desc.isEmpty ? "" : "d=\(desc)", val.isEmpty ? "" : "v=\(val.prefix(50))")
            return false
        }
    }
case "press":
    let app = AXUIElementCreateApplication(Int32(args[2])!)
    let key = args[3]
    var done = false
    for win in (attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? [] where !done {
        done = walk(win) { e, _ in
            guard str(e, "AXIdentifier") == key || str(e, kAXTitleAttribute) == key || str(e, kAXDescriptionAttribute) == key else { return false }
            let r = AXUIElementPerformAction(e, kAXPressAction as CFString)
            print("press", key, r.rawValue, str(e, kAXRoleAttribute))
            return r == .success
        }
    }
    if !done { print("not found", key) }
case "select":
    // Selects a row whose subtree holds the identifier or title.
    let app = AXUIElementCreateApplication(Int32(args[2])!)
    let key = args[3]
    var done = false
    for win in (attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? [] where !done {
        done = walk(win) { e, _ in
            guard str(e, kAXRoleAttribute) == "AXRow" else { return false }
            let holds = walk(e) { c, _ in str(c, "AXIdentifier") == key || str(c, kAXTitleAttribute) == key || (attr(c, kAXValueAttribute) as? String) == key || str(c, kAXDescriptionAttribute) == key }
            guard holds else { return false }
            let r = AXUIElementSetAttributeValue(e, kAXSelectedAttribute as CFString, kCFBooleanTrue)
            print("select", key, r.rawValue)
            return true
        }
    }
    if !done { print("no row", key) }
case "menu":
    let app = AXUIElementCreateApplication(Int32(args[2])!)
    guard let bar = attr(app, kAXMenuBarAttribute) else { print("no menu bar"); break }
    let menus = kids(bar as! AXUIElement)
    if args.count == 3 {
        for m in menus {
            print("==", str(m, kAXTitleAttribute))
            for sub in kids(m) { for i in kids(sub) {
                let enabled = (attr(i, kAXEnabledAttribute) as? Bool) ?? false
                print("  ", str(i, kAXTitleAttribute), enabled ? "" : "(disabled)", str(i, "AXMenuItemCmdChar"))
            } }
        }
        break
    }
    let menuName = args[3], item = args[4]
    for m in menus where str(m, kAXTitleAttribute) == menuName {
        for sub in kids(m) { for i in kids(sub) where str(i, kAXTitleAttribute) == item {
            print("menu", item, AXUIElementPerformAction(i, kAXPressAction as CFString).rawValue)
        } }
    }
case "post":
    DistributedNotificationCenter.default().postNotificationName(.init(args[2]), object: args[3], userInfo: nil, deliverImmediately: true)
    RunLoop.current.run(until: Date().addingTimeInterval(0.2))
default:
    print("usage")
}
