//! The `tts` filter (`specs/render/filters.md` F6). What review does with
//! the markup is the render pipeline's (U62, R967).

/// F6: the text wrapped in TTS markup, the options exactly as written after
/// `tts `.
pub fn tts(text: &str, options: &str) -> String {
    format!("[anki:tts lang={options}]{text}[/anki:tts]")
}
