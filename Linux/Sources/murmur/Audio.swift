import Foundation
import MurmurKit

/// Records the microphone with PipeWire's `pw-record` into a 16 kHz mono float WAV file.
@MainActor
final class Recorder {
    private var process: Process?
    private var file: URL?
    private var meter: Task<Void, Never>?

    /// Called about every 50 ms while recording with a 0...1 loudness value, for the pill's
    /// level meter.
    var onLevel: (@MainActor (Float) -> Void)?

    func start(microphone: String?) throws {
        guard Shell.has("pw-record") else { throw MurmurError("pw-record isn't installed") }
        Paths.ensure(Paths.runtime)
        // A file per recording, so the next one can start while the last is still being read.
        let file = Paths.runtime.appending(path: "recording-\(UUID().uuidString).wav")

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        var arguments = ["pw-record", "--rate", "\(Int(sttSampleRate))", "--channels", "1", "--format", "f32"]
        if let microphone { arguments += ["--target", microphone] }
        process.arguments = arguments + [file.path]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        self.process = process
        self.file = file

        if let onLevel {
            meter = Task {
                let reader = LevelReader(file: file)
                while !Task.isCancelled {
                    try? await Task.sleep(for: .milliseconds(50))
                    if let level = reader.next() { onLevel(level) }
                }
            }
        }
    }

    /// Stops recording at once and returns the samples once pw-record has finished the file.
    /// Empty if nothing was recorded. A new recording can start before the samples arrive.
    func stop() -> Task<[Float], Never> {
        meter?.cancel()
        meter = nil
        guard let process, let file else { return Task { [] } }
        self.process = nil
        self.file = nil
        // SIGINT lets pw-record finish the WAV header.
        process.interrupt()
        return Task {
            for _ in 0..<200 where process.isRunning { try? await Task.sleep(for: .milliseconds(10)) }
            if process.isRunning { process.terminate() }
            defer { try? FileManager.default.removeItem(at: file) }
            guard let data = try? Data(contentsOf: file) else {
                Log.info("pw-record wrote nothing (exit \(process.isRunning ? -1 : process.terminationStatus))")
                return []
            }
            return (try? WAV.decode(data).samples) ?? []
        }
    }
}

/// Follows the WAV file pw-record is writing and measures the audio added since the last call.
final class LevelReader {
    private let file: URL
    private var handle: FileHandle?
    private var pending: [UInt8] = []
    private var inData = false

    init(file: URL) {
        self.file = file
    }

    deinit { try? handle?.close() }

    /// Loudness of the new audio, or nil if none has arrived yet.
    func next() -> Float? {
        if handle == nil { handle = try? FileHandle(forReadingFrom: file) }
        guard let handle, let data = try? handle.readToEnd(), !data.isEmpty else { return nil }
        pending += data

        // Skip the header: the samples start after the "data" chunk's tag and size.
        if !inData {
            guard pending.count >= 20,
                  let tag = (12...(pending.count - 8)).first(where: { pending[$0..<$0 + 4] == [0x64, 0x61, 0x74, 0x61] })
            else { return nil }
            pending.removeFirst(tag + 8)
            inData = true
        }

        let usable = pending.count / 4 * 4
        guard usable > 0 else { return nil }
        let samples = pending[0..<usable].withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
        pending.removeFirst(usable)
        return Self.level(samples)
    }

    /// Maps roughly -50 dB...0 dB to 0...1, the same scale as the Mac's level meter.
    static func level(_ samples: [Float]) -> Float {
        let rms = sqrt(samples.reduce(0) { $0 + $1 * $1 } / Float(max(samples.count, 1)))
        let db = 20 * log10(max(rms, 1e-6))
        return min(max((db + 50) / 50, 0), 1)
    }
}

/// Just enough WAV reading for pw-record's output and test clips: 16-bit or float PCM, any channel
/// count (mixed down to mono), with or without WAVE_FORMAT_EXTENSIBLE.
enum WAV {
    static func decode(_ data: Data) throws -> (samples: [Float], sampleRate: Int) {
        let bytes = [UInt8](data)
        func u16(_ at: Int) -> Int { Int(bytes[at]) | Int(bytes[at + 1]) << 8 }
        func u32(_ at: Int) -> Int { u16(at) | u16(at + 2) << 16 }
        guard bytes.count >= 12, bytes[0..<4] == [0x52, 0x49, 0x46, 0x46], bytes[8..<12] == [0x57, 0x41, 0x56, 0x45]
        else { throw MurmurError("Not a WAV file") }

        var format = 0, channels = 1, sampleRate = 0, bits = 0
        var offset = 12
        while offset + 8 <= bytes.count {
            let id = String(decoding: bytes[offset..<offset + 4], as: UTF8.self)
            var size = u32(offset + 4)
            let body = offset + 8
            if id == "fmt " {
                format = u16(body)
                channels = max(u16(body + 2), 1)
                sampleRate = u32(body + 4)
                bits = u16(body + 14)
                // WAVE_FORMAT_EXTENSIBLE: the real format is the first two bytes of the subformat GUID.
                if format == 0xFFFE, size >= 26 { format = u16(body + 24) }
            } else if id == "data" {
                // A recording that was cut off may have a missing or wrong size.
                if size == 0 || body + size > bytes.count { size = bytes.count - body }
                return (mixDown(bytes[body..<body + size], format: format, bits: bits, channels: channels), sampleRate)
            }
            offset = body + size + (size & 1)
        }
        throw MurmurError("WAV file has no audio data")
    }

    private static func mixDown(_ raw: ArraySlice<UInt8>, format: Int, bits: Int, channels: Int) -> [Float] {
        let data = Data(raw)
        let interleaved: [Float]
        switch (format, bits) {
        case (3, 32):
            interleaved = data.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
        case (1, 16):
            interleaved = data.withUnsafeBytes { $0.bindMemory(to: Int16.self).map { Float(Int16(littleEndian: $0)) / 32768 } }
        default:
            return []
        }
        guard channels > 1 else { return interleaved }
        return stride(from: 0, to: interleaved.count - channels + 1, by: channels).map { start in
            interleaved[start..<start + channels].reduce(0, +) / Float(channels)
        }
    }
}

/// Plays the start and stop sounds with `pw-play`, the same synthesized sounds as the Mac app.
/// Each theme is rendered to a WAV file the first time it's used.
enum Sounds {
    enum Cue: String { case start, stop }

    static func play(_ cue: Cue, theme: SoundTheme?) {
        guard let theme else { return }
        let url = Paths.runtime.appending(path: "sounds/\(theme.rawValue)-\(cue.rawValue).wav")
        if !FileManager.default.fileExists(atPath: url.path) {
            Paths.ensure(url.deletingLastPathComponent())
            try? (cue == .start ? theme.startSound() : theme.stopSound()).write(to: url)
        }
        Shell.spawn("pw-play", ["--volume", "0.35", url.path])
    }
}
