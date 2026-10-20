import CSherpaOnnx
import Foundation
import MurmurKit

/// Parakeet TDT through sherpa-onnx, on the CPU. The same model the Mac app runs on the Neural
/// Engine (v3, multilingual), as int8 ONNX. Loaded once; call `transcribe` from one task at a time.
final class Transcriber: @unchecked Sendable {
    /// The model `scripts/setup.sh` downloads.
    static let defaultModel = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"

    private let recognizer: OpaquePointer

    init(modelDirectory: URL, threads: Int) throws {
        let file = { (name: String) in modelDirectory.appending(path: name).path }
        let paths = ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"].map(file)
        for path in paths where !FileManager.default.fileExists(atPath: path) {
            throw MurmurError("Speech model not found: \(path). Run Linux/scripts/setup.sh to download it.")
        }

        // sherpa-onnx copies the strings while creating the recognizer, so they only need to live
        // until then.
        let strings = (paths + ["nemo_transducer", "cpu", "greedy_search"]).map { strdup($0)! }
        defer { strings.forEach { free($0) } }

        var config = SherpaOnnxOfflineRecognizerConfig()
        config.feat_config.sample_rate = Int32(sttSampleRate)
        config.feat_config.feature_dim = 80
        config.model_config.transducer.encoder = UnsafePointer(strings[0])
        config.model_config.transducer.decoder = UnsafePointer(strings[1])
        config.model_config.transducer.joiner = UnsafePointer(strings[2])
        config.model_config.tokens = UnsafePointer(strings[3])
        config.model_config.model_type = UnsafePointer(strings[4])
        config.model_config.provider = UnsafePointer(strings[5])
        config.model_config.num_threads = Int32(threads)
        config.decoding_method = UnsafePointer(strings[6])

        guard let recognizer = SherpaOnnxCreateOfflineRecognizer(&config) else {
            throw MurmurError("sherpa-onnx couldn't load the speech model from \(modelDirectory.path)")
        }
        self.recognizer = recognizer
    }

    deinit {
        SherpaOnnxDestroyOfflineRecognizer(recognizer)
    }

    /// Mono samples in, text out. sherpa-onnx resamples anything that isn't 16 kHz.
    func transcribe(_ samples: [Float], sampleRate: Int = Int(sttSampleRate)) -> String {
        guard let stream = SherpaOnnxCreateOfflineStream(recognizer) else { return "" }
        defer { SherpaOnnxDestroyOfflineStream(stream) }
        samples.withUnsafeBufferPointer {
            SherpaOnnxAcceptWaveformOffline(stream, Int32(sampleRate), $0.baseAddress, Int32($0.count))
        }
        SherpaOnnxDecodeOfflineStream(recognizer, stream)
        guard let result = SherpaOnnxGetOfflineStreamResult(stream) else { return "" }
        defer { SherpaOnnxDestroyOfflineRecognizerResult(result) }
        guard let text = result.pointee.text else { return "" }
        return String(cString: text).trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
