import AVFoundation
import FluidAudio
import os

/// Push-to-talk microphone capture: `start()` on key down, `stop()` on key up returns
/// 16 kHz mono samples ready for an `STTEngine`.
///
/// The tap runs on a realtime audio thread, so this type is deliberately not main-actor
/// isolated and guards its sample buffer with a lock.
public final class AudioCapture: @unchecked Sendable {
    private var engine = AVAudioEngine()
    private let state = OSAllocatedUnfairLock(initialState: [Float]())
    private var inputSampleRate: Double = 48_000
    private var running = false

    /// Called from the audio thread with a 0...1 loudness value, for the HUD's level meter.
    public var onLevel: (@Sendable (Float) -> Void)?

    public init() {}

    /// Starts recording from the device with Core Audio UID `deviceUID`, or from the system
    /// default input when it's nil or that device isn't connected.
    public func start(deviceUID: String? = nil) throws {
        guard !running else { return }
        state.withLock { $0.removeAll(keepingCapacity: true) }

        // A fresh engine each time, so a device change (or a new system default) takes effect
        // and the input format is re-read for that device.
        engine = AVAudioEngine()
        let input = engine.inputNode
        if let deviceUID, var deviceID = AudioDevices.deviceID(forUID: deviceUID), let unit = input.audioUnit {
            let status = AudioUnitSetProperty(
                unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                &deviceID, UInt32(MemoryLayout<AudioDeviceID>.size)
            )
            if status != noErr { throw MurmurError("Couldn't switch to the selected microphone (\(status))") }
        }
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            throw MurmurError("No microphone input is available")
        }
        inputSampleRate = format.sampleRate

        let onLevel = self.onLevel
        let state = self.state
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
            guard let channels = buffer.floatChannelData else { return }
            let frames = Int(buffer.frameLength)
            let channelCount = Int(buffer.format.channelCount)

            // Downmix to mono at the input rate; resample once at the end.
            var mono = [Float](repeating: 0, count: frames)
            for channel in 0..<channelCount {
                let data = channels[channel]
                for i in 0..<frames { mono[i] += data[i] }
            }
            if channelCount > 1 {
                let scale = 1 / Float(channelCount)
                for i in 0..<frames { mono[i] *= scale }
            }
            let chunk = mono
            state.withLock { $0.append(contentsOf: chunk) }

            if let onLevel {
                let rms = sqrt(chunk.reduce(0) { $0 + $1 * $1 } / Float(max(frames, 1)))
                // Map roughly -50 dB...0 dB to 0...1.
                let db = 20 * log10(max(rms, 1e-6))
                onLevel(min(max((db + 50) / 50, 0), 1))
            }
        }

        engine.prepare()
        try engine.start()
        running = true
    }

    /// Stops the mic and returns everything captured since `start()`, at 16 kHz mono.
    public func stop() throws -> [Float] {
        guard running else { return [] }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        running = false

        let captured = state.withLock { samples in
            defer { samples.removeAll() }
            return samples
        }
        guard !captured.isEmpty else { return [] }
        return try AudioConverter().resample(captured, from: inputSampleRate)
    }
}
