import Foundation
import MurmurCore

/// Where the eval keeps its inputs and outputs. Resolved from this source file's location so the
/// CLI works no matter which directory `swift run` is invoked from.
enum Paths {
    static let evalRoot = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()  // MurmurEval
        .deletingLastPathComponent()  // Sources
        .deletingLastPathComponent()  // Eval
    static let clips = evalRoot.appending(path: "clips")
    static let results = evalRoot.appending(path: "results")
}

/// Sidecar metadata stored next to each recording as `<name>.json`.
struct ClipMeta: Codable {
    /// Exactly what was said, word for word. Used to score raw STT accuracy (WER).
    var reference: String?
    /// The text you would want typed. Used to eyeball (and later grade) the cleanup output.
    var intended: String?
    var context: TargetContext = .plain
}

struct Clip {
    let name: String
    let audio: URL
    var meta: ClipMeta

    static let audioExtensions: Set<String> = ["wav", "m4a", "caf", "aiff", "mp3"]

    /// Every audio file in `clips/`, paired with its sidecar if one exists.
    static func loadAll(from dir: URL = Paths.clips) throws -> [Clip] {
        let files = try FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
        return try files
            .filter { audioExtensions.contains($0.pathExtension.lowercased()) }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
            .map { audio in
                let name = audio.deletingPathExtension().lastPathComponent
                let sidecar = dir.appending(path: "\(name).json")
                let meta = FileManager.default.fileExists(atPath: sidecar.path)
                    ? try JSONDecoder().decode(ClipMeta.self, from: Data(contentsOf: sidecar))
                    : ClipMeta()
                return Clip(name: name, audio: audio, meta: meta)
            }
    }

    static func saveMeta(_ meta: ClipMeta, name: String, in dir: URL = Paths.clips) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(meta).write(to: dir.appending(path: "\(name).json"))
    }
}
