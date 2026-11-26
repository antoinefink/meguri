# Editor

- macOS non-Browse IO uses one retained field page with inert native Image/Tags row slots. Configure slots before initial field focus; resizing must not rebuild fields, flush HTML or write a note.
- Native row metrics match NoteFormSelectionRows and page.html. Keep runtime layout messages and EditorBridgeMessage validation in step. Commits remain in stored-ordinal order even when the visual order differs.
- Image readiness can enable the canvas while text is being typed. Preserve the original host's text responder, and let any later deliberate key/click supersede restoration. Never add a form-wide image-paste shortcut.
