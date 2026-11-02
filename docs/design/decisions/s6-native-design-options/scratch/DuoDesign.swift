import Foundation

/// Preview only, never shipped: which iPhone Duo design option this launch shows.
public enum DuoDesign {
    public static let foldWidth: CGFloat = 40
    public static var rows: String { UserDefaults.standard.string(forKey: "DuoRows") ?? "" }
    public static var home: String { UserDefaults.standard.string(forKey: "DuoHome") ?? "" }
    public static var review: String { UserDefaults.standard.string(forKey: "DuoReview") ?? "" }
}
