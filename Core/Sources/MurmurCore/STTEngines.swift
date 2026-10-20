import AVFoundation
import FluidAudio
import Foundation
import Speech

/// One speech-to-text backend: 16 kHz mono Float samples in, text out.
public protocol STTEngine: Sendable {
    var id: String { get }
    /// Download / compile models. Slow the first time; not counted toward per-clip latency.
    func prepare() async throws
    func transcribe(_ samples: [Float]) async throws -> String
}

extension STTEngine {
    /// Any audio file AVFoundation can read, resampled to 16 kHz mono.
    public func transcribe(file url: URL) async throws -> String {
        try await transcribe(AudioConverter().resampleAudioFile(url))
    }
}

// MARK: - Parakeet (NVIDIA Parakeet TDT on the Neural Engine, via FluidAudio)

public actor ParakeetEngine: STTEngine {
    public nonisolated let id: String
    private let version: AsrModelVersion
    private var manager: AsrManager?

    public init(id: String = "parakeet", version: AsrModelVersion = .v3) {
        self.id = id
        self.version = version
    }

    public func prepare() async throws {
        guard manager == nil else { return }
        let models = try await AsrModels.downloadAndLoad(version: version)
        let manager = AsrManager(config: .default)
        try await manager.loadModels(models)
        self.manager = manager
    }

    public func transcribe(_ samples: [Float]) async throws -> String {
        guard let manager else { throw MurmurError("\(id) was not prepared") }
        var state = TdtDecoderState.make(decoderLayers: await manager.decoderLayerCount)
        let result = try await manager.transcribe(samples, decoderState: &state)
        return result.text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

// MARK: - Apple SpeechAnalyzer (macOS 26 on-device model)

public actor AppleSpeechEngine: STTEngine {
    public nonisolated let id = "apple"
    private var locale = Locale(identifier: "en-US")

    public init() {}

    public func prepare() async throws {
        guard let supported = await SpeechTranscriber.supportedLocale(equivalentTo: locale) else {
            throw MurmurError("SpeechTranscriber does not support \(locale.identifier)")
        }
        locale = supported
        let probe = SpeechTranscriber(locale: locale, preset: .transcription)
        if let request = try await AssetInventory.assetInstallationRequest(supporting: [probe]) {
            try await request.downloadAndInstall()
        }
    }

    public func transcribe(_ samples: [Float]) async throws -> String {
        // SpeechAnalyzer reads from a file most simply, so round-trip through a temp WAV.
        let url = FileManager.default.temporaryDirectory.appending(path: "murmur-\(UUID().uuidString).wav")
        defer { try? FileManager.default.removeItem(at: url) }
        try writeWAV(samples, to: url)

        // A fresh transcriber + analyzer per call: an analyzer can only be finished once.
        let transcriber = SpeechTranscriber(locale: locale, preset: .transcription)
        let analyzer = SpeechAnalyzer(modules: [transcriber])
        let collector = Task {
            var text = ""
            for try await result in transcriber.results where result.isFinal {
                text += String(result.text.characters)
            }
            return text
        }

        let file = try AVAudioFile(forReading: url)
        if let lastSample = try await analyzer.analyzeSequence(from: file) {
            try await analyzer.finalizeAndFinish(through: lastSample)
        } else {
            await analyzer.cancelAndFinishNow()
        }
        return try await collector.value.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

/// Writes 16 kHz mono Float samples as a WAV file.
public func writeWAV(_ samples: [Float], to url: URL) throws {
    let format = AVAudioFormat(standardFormatWithSampleRate: sttSampleRate, channels: 1)!
    guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(samples.count)) else {
        throw MurmurError("Could not allocate an audio buffer")
    }
    buffer.frameLength = AVAudioFrameCount(samples.count)
    samples.withUnsafeBufferPointer { buffer.floatChannelData![0].update(from: $0.baseAddress!, count: samples.count) }
    let file = try AVAudioFile(forWriting: url, settings: format.settings)
    try file.write(from: buffer)
}
