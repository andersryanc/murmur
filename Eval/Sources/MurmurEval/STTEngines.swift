import AVFoundation
import FluidAudio
import Foundation
import Speech

/// One speech-to-text backend. The app will use this same seam to swap engines.
protocol STTEngine {
    var id: String { get }
    /// Download / compile models. Not counted toward per-clip latency.
    func prepare() async throws
    func transcribe(_ audio: URL) async throws -> String
}

// MARK: - Parakeet (NVIDIA Parakeet TDT on the Neural Engine, via FluidAudio)

final class ParakeetEngine: STTEngine {
    let id: String
    private let version: AsrModelVersion
    private var manager: AsrManager?

    init(id: String, version: AsrModelVersion) {
        self.id = id
        self.version = version
    }

    func prepare() async throws {
        let models = try await AsrModels.downloadAndLoad(version: version)
        let manager = AsrManager(config: .default)
        try await manager.loadModels(models)
        self.manager = manager
    }

    func transcribe(_ audio: URL) async throws -> String {
        guard let manager else { throw EvalError("\(id) was not prepared") }
        var state = TdtDecoderState.make(decoderLayers: await manager.decoderLayerCount)
        let result = try await manager.transcribe(audio, decoderState: &state)
        return result.text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

// MARK: - Apple SpeechAnalyzer (macOS 26 on-device model)

final class AppleSpeechEngine: STTEngine {
    let id = "apple"
    private var locale = Locale(identifier: "en-US")

    func prepare() async throws {
        guard let supported = await SpeechTranscriber.supportedLocale(equivalentTo: locale) else {
            throw EvalError("SpeechTranscriber does not support \(locale.identifier)")
        }
        locale = supported
        let probe = SpeechTranscriber(locale: locale, preset: .transcription)
        if let request = try await AssetInventory.assetInstallationRequest(supporting: [probe]) {
            print("  downloading Apple speech assets for \(locale.identifier)…")
            try await request.downloadAndInstall()
        }
    }

    func transcribe(_ audio: URL) async throws -> String {
        // A fresh transcriber + analyzer per clip: an analyzer can only be finished once.
        let transcriber = SpeechTranscriber(locale: locale, preset: .transcription)
        let analyzer = SpeechAnalyzer(modules: [transcriber])

        let collector = Task {
            var text = ""
            for try await result in transcriber.results where result.isFinal {
                text += String(result.text.characters)
            }
            return text
        }

        let file = try AVAudioFile(forReading: audio)
        if let lastSample = try await analyzer.analyzeSequence(from: file) {
            try await analyzer.finalizeAndFinish(through: lastSample)
        } else {
            await analyzer.cancelAndFinishNow()
        }
        return try await collector.value.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

// MARK: - Registry

enum Engines {
    static let all: [String: @Sendable () -> STTEngine] = [
        "parakeet": { ParakeetEngine(id: "parakeet", version: .v3) },
        "parakeet-v2": { ParakeetEngine(id: "parakeet-v2", version: .v2) },
        "parakeet-ultra": { ParakeetEngine(id: "parakeet-ultra", version: .ultra) },
        "apple": { AppleSpeechEngine() },
    ]
    static let defaults = ["parakeet", "apple"]

    static func make(_ ids: [String]) throws -> [STTEngine] {
        try ids.map { id in
            guard let factory = all[id] else {
                throw EvalError("Unknown engine '\(id)'. Available: \(all.keys.sorted().joined(separator: ", "))")
            }
            return factory()
        }
    }
}
