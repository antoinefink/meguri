import Foundation

/// U206's host controls read the feature's catalog here, even when the
/// containing form belongs to Editor or Browse (`image-occlusion-strings` S1).
public enum IONoteText {
    public static var pickerTitle: String { String(localized: "io.picker.title", defaultValue: "Choose an Image", bundle: .module) }
    public static var photoLibrary: String { String(localized: "io.picker.photoLibrary", defaultValue: "Photo Library", bundle: .module) }
    public static var takePhoto: String { String(localized: "io.picker.takePhoto", defaultValue: "Take Photo", bundle: .module) }
    public static var chooseFile: String { String(localized: "io.picker.chooseFile", defaultValue: "Choose File…", bundle: .module) }
    public static var paste: String { String(localized: "io.picker.paste", defaultValue: "Paste Image", bundle: .module) }
    public static var dropHint: String { String(localized: "io.picker.dropHint", defaultValue: "Drop an image here.", bundle: .module) }
    public static var noPasteImage: String { String(localized: "io.picker.noImageOnPasteboard", defaultValue: "There’s no image to paste.", bundle: .module) }
    public static var maskEditor: String { String(localized: "io.maskEditor.toggle", defaultValue: "Mask Editor", bundle: .module) }
    public static var changeImage: String { String(localized: "io.changeImage", defaultValue: "Change Image…", bundle: .module) }
    public static var changeTitle: String { String(localized: "io.changeImage.confirm.title", defaultValue: "Change the image?", bundle: .module) }
    public static var changeMessage: String { String(localized: "io.changeImage.confirm.message", defaultValue: "The masks keep their positions on the new image.", bundle: .module) }
    public static var confirmChange: String { String(localized: "io.changeImage.confirm.change", defaultValue: "Change Image", bundle: .module) }
    public static var noOcclusions: String { String(localized: "io.add.noOcclusions", defaultValue: "No occlusions created", bundle: .module) }
    public static var noRoles: String { String(localized: "io.editor.noRoles", defaultValue: "This note type has too few fields for image occlusion. You can still edit its other fields.", bundle: .module) }
    public static var noImage: String { String(localized: "io.editor.noImage", defaultValue: "This note has no image.", bundle: .module) }
    public static var drawNew: String { String(localized: "io.editor.drawNewMasks", defaultValue: "Draw New Masks", bundle: .module) }
    public static func missingImage(_ name: String) -> String {
        String(format: String(localized: "io.editor.imageMissing", defaultValue: "“%@” isn’t in the media folder.", bundle: .module), name)
    }
    public static func unusableImage(_ name: String) -> String {
        String(format: String(localized: "io.editor.imageUnusable", defaultValue: "“%@” can’t be shown in the mask editor.", bundle: .module), name)
    }
    public static func unreadableImage(_ name: String) -> String {
        String(format: String(localized: "io.picker.unreadable", defaultValue: "“%@” can’t be opened.", bundle: .module), name)
    }
    public static func notImage(_ name: String) -> String {
        String(format: String(localized: "io.picker.notImage", defaultValue: "“%@” isn’t an image.", bundle: .module), name)
    }
    public static func unreadableMasks(_ error: String, copying: Bool) -> String {
        let format = copying
            ? String(localized: "io.editor.unreadable.copy", defaultValue: "The masks of this note can’t be read (%@). Draw new masks to add it.", bundle: .module)
            : String(localized: "io.editor.unreadable.noShape", defaultValue: "This note’s masks can’t be read (%@). You can still edit its other fields.", bundle: .module)
        return String(format: format, error)
    }
}
