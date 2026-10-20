import Foundation
import MurmurKit

/// Settings from `~/.config/murmur/config.json`, re-read at the start of every dictation. Every
/// field is optional:
/// ```json
/// {
///   "cleanup": "claude",
///   "ollamaModel": "qwen3.5:9b",
///   "sounds": "wood",
///   "microphone": "alsa_input.usb-Blue_Yeti-00.analog-stereo",
///   "threads": 4
/// }
/// ```
struct Config: Decodable {
    enum Cleanup: String, Decodable {
        /// Claude Haiku on the Anthropic API.
        case claude
        /// `ollamaModel` served by Ollama on this machine.
        case ollama
        /// Paste Parakeet's transcript as-is (Dictionary replacements still apply).
        case off
    }

    var cleanup = Cleanup.claude
    var ollamaModel = "qwen3.5:9b"
    /// wood, soft, chime, bubble, or off.
    var sounds = "wood"
    /// PipeWire node name of the microphone (`pw-record --list-targets`), or nil for the default.
    var microphone: String?
    /// Folder holding the sherpa-onnx Parakeet model.
    var model: String?
    /// CPU threads for speech-to-text.
    var threads = 4

    static var url: URL { Paths.config.appending(path: "config.json") }

    var soundTheme: SoundTheme? { SoundTheme(rawValue: sounds) }

    var modelDirectory: URL {
        model.map { URL(fileURLWithPath: ($0 as NSString).expandingTildeInPath) }
            ?? Paths.data.appending(path: "models/\(Transcriber.defaultModel)")
    }

    init() {}

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        cleanup = try c.decodeIfPresent(Cleanup.self, forKey: .cleanup) ?? cleanup
        ollamaModel = try c.decodeIfPresent(String.self, forKey: .ollamaModel) ?? ollamaModel
        sounds = try c.decodeIfPresent(String.self, forKey: .sounds) ?? sounds
        microphone = try c.decodeIfPresent(String.self, forKey: .microphone)
        model = try c.decodeIfPresent(String.self, forKey: .model)
        threads = try c.decodeIfPresent(Int.self, forKey: .threads) ?? threads
    }

    private enum CodingKeys: String, CodingKey {
        case cleanup, ollamaModel, sounds, microphone, model, threads
    }

    /// The config file, or the defaults if there isn't one. A broken file is logged and ignored.
    static func load() -> Config {
        guard let data = try? Data(contentsOf: url) else { return Config() }
        do {
            return try JSONDecoder().decode(Config.self, from: data)
        } catch {
            Log.info("Ignoring \(url.path): \(error)")
            return Config()
        }
    }

    /// The cleaner for this config, or nil to paste the raw transcript. Throws when Claude is
    /// chosen but there's no API key.
    func makeCleaner(vocabulary: [String]) throws -> (any TextCleaner)? {
        switch cleanup {
        case .off:
            return nil
        case .ollama:
            return OllamaCleaner(model: ollamaModel, vocabulary: vocabulary)
        case .claude:
            guard let key = APIKey.load() else { throw MurmurError("No Anthropic API key") }
            return ClaudeCleaner(apiKey: key, vocabulary: vocabulary)
        }
    }
}
