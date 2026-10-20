import AVFoundation
import Foundation

/// Records the default input device to a WAV file until the user presses Return.
/// This is a stand-in for the app's push-to-talk capture: same engine, no hotkey yet.
enum Recorder {
    static func record(to url: URL) async throws -> TimeInterval {
        guard await AVAudioApplication.requestRecordPermission() else {
            throw EvalError("Microphone access was denied. Allow it for your terminal app in System Settings › Privacy & Security › Microphone.")
        }

        let engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        let file = try AVAudioFile(forWriting: url, settings: format.settings)

        input.installTap(onBus: 0, bufferSize: 4096, format: format) { buffer, _ in
            try? file.write(from: buffer)
        }
        try engine.start()
        let started = Date()

        print("● Recording… press Return to stop.")
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
            Thread.detachNewThread {
                _ = readLine()
                done.resume()
            }
        }

        input.removeTap(onBus: 0)
        engine.stop()
        return Date().timeIntervalSince(started)
    }
}

/// Duration of any audio file AVFoundation can read.
func audioDuration(_ url: URL) -> TimeInterval {
    guard let file = try? AVAudioFile(forReading: url) else { return 0 }
    return Double(file.length) / file.processingFormat.sampleRate
}
