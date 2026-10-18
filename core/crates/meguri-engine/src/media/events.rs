use crate::ExtensionEvent;

/// Published after downloaded files become available to the renderer (U111).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaArrived {
    pub names: Vec<String>,
}
impl ExtensionEvent for MediaArrived {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
