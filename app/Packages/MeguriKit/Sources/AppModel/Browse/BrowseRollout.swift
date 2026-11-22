/// Browse stays out of the shipped app until its results table (U270a)
/// lands: while `shown` is false the Browse row, View › Browse (⇧⌘B) and
/// the Browse screen a deep link would open are absent, and the shell
/// behaves as it did before Browse. Hook launches and the app-hosted tests
/// turn it on (`MeguriApp`), so the routes, drives and tests exercise
/// Browse. U270a removes this switch.
public enum BrowseRollout {
    public static var shown = false
}
