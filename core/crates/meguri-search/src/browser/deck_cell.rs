//! Cards-mode Deck cell (C7.4). The caller resolves and validates both decks.
pub fn cards(current: &str, home: Option<&str>) -> String {
    match home {
        Some(home) => format!("{current} ({home})"),
        None => current.to_owned(),
    }
}
