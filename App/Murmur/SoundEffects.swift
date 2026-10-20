import AVFoundation

/// A named pair of start/stop sounds. All are synthesized in code at launch, so there are no
/// audio assets to ship and every sound is Murmur's own rather than a stock system sound.
enum SoundTheme: String, CaseIterable, Identifiable {
    /// Low, soft wooden bloops: D4 → A4 to start, A4 → D4 to stop.
    case wood
    /// Bright bell-like taps an octave and a half higher: E6 → B6, B6 → E6.
    case chime
    /// A single G4 note that swoops up into pitch to start and down into it to stop.
    case bubble

    var id: String { rawValue }
    var label: String { rawValue.capitalized }

    fileprivate var start: Voice {
        switch self {
        case .wood: .wood(notes: [293.66, 440.0])
        case .chime: .chime(notes: [1318.5, 1975.5])
        case .bubble: .bubble(swoopFrom: 0.72)
        }
    }

    fileprivate var stop: Voice {
        switch self {
        case .wood: .wood(notes: [440.0, 293.66])
        case .chime: .chime(notes: [1975.5, 1318.5])
        case .bubble: .bubble(swoopFrom: 1.38)
        }
    }
}

/// Plays the start and stop sounds for the chosen theme.
final class SoundEffects {
    var theme: SoundTheme = .wood {
        didSet { if theme != oldValue { load() } }
    }

    private var start: AVAudioPlayer?
    private var stop: AVAudioPlayer?

    init(theme: SoundTheme = .wood) {
        self.theme = theme
        load()
    }

    func playStart() { play(start) }
    func playStop() { play(stop) }

    private func load() {
        start = Self.player(theme.start.render())
        stop = Self.player(theme.stop.render())
    }

    private func play(_ player: AVAudioPlayer?) {
        guard let player else { return }
        player.currentTime = 0
        player.play()
    }

    private static func player(_ wav: Data) -> AVAudioPlayer? {
        let player = try? AVAudioPlayer(data: wav)
        player?.volume = 0.35
        player?.prepareToPlay()
        return player
    }
}

// MARK: - Synthesis

/// How one sound is built: a run of notes, each with its own envelope, harmonics and pitch glide.
private struct Voice {
    var notes: [Double]
    /// Time between note onsets.
    var noteLength: Double
    /// How long the last note rings out after its slot.
    var tail: Double
    /// Exponential decay rate; higher fades faster.
    var decay: Double
    /// Relative amplitudes of the fundamental, 2nd, 3rd… harmonics.
    var harmonics: [Double]
    /// Each note starts at `glideFrom` × its pitch and settles at a rate of `glideSpeed`.
    var glideFrom: Double = 1
    var glideSpeed: Double = 70
    var gain: Double = 0.5

    static func wood(notes: [Double]) -> Voice {
        Voice(notes: notes, noteLength: 0.05, tail: 0.09, decay: 32,
              harmonics: [1, 0.28, 0.08], glideFrom: 1.025, glideSpeed: 70, gain: 0.55)
    }

    static func chime(notes: [Double]) -> Voice {
        Voice(notes: notes, noteLength: 0.075, tail: 0.12, decay: 28, harmonics: [1, 0.18])
    }

    static func bubble(swoopFrom: Double) -> Voice {
        Voice(notes: [392.0], noteLength: 0.0, tail: 0.16, decay: 26,
              harmonics: [1, 0.12], glideFrom: swoopFrom, glideSpeed: 38, gain: 0.6)
    }

    private static let sampleRate = 44_100.0

    /// Renders the voice as a 16-bit mono WAV file.
    func render() -> Data {
        let sr = Self.sampleRate
        let total = Int(sr * (noteLength * Double(notes.count) + tail))
        var samples = [Float](repeating: 0, count: total)

        for (index, frequency) in notes.enumerated() {
            let offset = Int(sr * noteLength * Double(index))
            var phase = 0.0
            for i in 0..<(total - offset) {
                let t = Double(i) / sr
                let glide = 1 + (glideFrom - 1) * exp(-t * glideSpeed)
                phase += 2 * .pi * frequency * glide / sr
                let attack = min(t / 0.003, 1)
                let envelope = attack * exp(-t * decay)
                var wave = 0.0
                for (n, amplitude) in harmonics.enumerated() { wave += amplitude * sin(Double(n + 1) * phase) }
                samples[offset + i] += Float(wave * envelope * gain)
            }
        }
        return Self.encodeWAV(samples)
    }

    private static func encodeWAV(_ samples: [Float]) -> Data {
        let pcm = samples.map { Int16(max(-1, min(1, $0)) * Float(Int16.max)) }
        let dataSize = UInt32(pcm.count * 2)
        var data = Data()
        func append<T: FixedWidthInteger>(_ value: T) { withUnsafeBytes(of: value.littleEndian) { data.append(contentsOf: $0) } }

        data.append(contentsOf: "RIFF".utf8); append(36 + dataSize)
        data.append(contentsOf: "WAVE".utf8)
        data.append(contentsOf: "fmt ".utf8); append(UInt32(16))
        append(UInt16(1))                    // PCM
        append(UInt16(1))                    // mono
        append(UInt32(sampleRate))
        append(UInt32(sampleRate) * 2)       // byte rate
        append(UInt16(2))                    // block align
        append(UInt16(16))                   // bits per sample
        data.append(contentsOf: "data".utf8); append(dataSize)
        pcm.forEach { append($0) }
        return data
    }
}
