import AppModel
import Foundation
import Testing

@MainActor struct MediaFolderTests {
    @Test func onlyFilesInsideTheFolderResolveAndOnlyImagesOpen() throws {
        let base = FileManager.default.temporaryDirectory.appending(path: "media-folder-\(UUID().uuidString)")
        let folder = base.appending(path: "collection.media")
        let outside = base.appending(path: "outside.png")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: base) }
        try Data().write(to: folder.appending(path: "picture.png"))
        try Data().write(to: folder.appending(path: "sound.mp3"))
        try Data().write(to: outside)
        try FileManager.default.createSymbolicLink(at: folder.appending(path: "link.png"), withDestinationURL: outside)
        let media = MediaFolder(directory: folder)

        #expect(media.file("picture.png") != nil)
        #expect(media.file("sound.mp3") != nil)
        #expect(media.file("missing.png") == nil)
        #expect(media.file("link.png") == nil)
        #expect(MediaFolder.file("link.png", in: folder) == nil)
        #expect(MediaFolder.file("picture.png", in: folder) != nil)
        #expect(media.image("picture.png") != nil)
        #expect(media.image("sound.mp3") == nil)
        #expect(MediaFolder(directory: nil).file("picture.png") == nil)
    }
}
