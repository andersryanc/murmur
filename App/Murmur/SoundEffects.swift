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
    /// Pure, round tones: a faint grace note, then the main note. D4 → A4 to start,
    /// A4 → D4 to stop, with a quick fade into a soft lingering tail.
    case soft

    var id: String { rawValue }
    var label: String { rawValue.capitalized }

    fileprivate var start: Voice {
        switch self {
        case .wood: .wood(notes: [293.66, 440.0])
        case .chime: .chime(notes: [1318.5, 1975.5])
        case .bubble: .bubble(swoopFrom: 0.72)
        case .soft: .soft(grace: 293.66, main: 440.0, gap: 0.022, graceGain: 0.07, tailLevel: 0.035, tailDecay: 20, length: 0.18)
        }
    }

    fileprivate var stop: Voice {
        switch self {
        case .wood: .wood(notes: [440.0, 293.66])
        case .chime: .chime(notes: [1975.5, 1318.5])
        case .bubble: .bubble(swoopFrom: 1.38)
        case .soft: .soft(grace: 440.0, main: 293.66, gap: 0.04, graceGain: 0.1, tailLevel: 0.02, tailDecay: 6, length: 0.22)
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

/// How one sound is built: a few notes, each with its own start time and level, sharing an
/// envelope, harmonics and pitch glide.
private struct Voice {
    struct Note {
        var frequency: Double
        /// Seconds from the start of the sound.
        var onset: Double
        var gain: Double = 1
    }

    var notes: [Note]
    /// Total length of the sound; the last 8 ms fade out so it never ends on a click.
    var length: Double
    /// Exponential decay rate; higher fades faster.
    var decay: Double
    /// Optional second, slower stage: the note falls quickly, then lingers at `tailLevel`
    /// (relative to its peak) and fades at `tailDecay`.
    var tailLevel: Double = 0
    var tailDecay: Double = 0
    /// Relative amplitudes of the fundamental, 2nd, 3rd… harmonics.
    var harmonics: [Double]
    /// Each note starts at `glideFrom` × its pitch and settles at a rate of `glideSpeed`.
    var glideFrom: Double = 1
    var glideSpeed: Double = 70
    var attack: Double = 0.003
    var gain: Double = 0.5

    /// Equal-level notes, one every `spacing` seconds.
    private static func sequence(_ frequencies: [Double], spacing: Double) -> [Note] {
        frequencies.enumerated().map { Note(frequency: $1, onset: spacing * Double($0)) }
    }

    static func wood(notes: [Double]) -> Voice {
        Voice(notes: sequence(notes, spacing: 0.05), length: 0.05 * Double(notes.count) + 0.09, decay: 32,
              harmonics: [1, 0.28, 0.08], glideFrom: 1.025, glideSpeed: 70, gain: 0.55)
    }

    static func chime(notes: [Double]) -> Voice {
        Voice(notes: sequence(notes, spacing: 0.075), length: 0.075 * Double(notes.count) + 0.12, decay: 28,
              harmonics: [1, 0.18])
    }

    static func bubble(swoopFrom: Double) -> Voice {
        Voice(notes: [Note(frequency: 392.0, onset: 0)], length: 0.16, decay: 26,
              harmonics: [1, 0.12], glideFrom: swoopFrom, glideSpeed: 38, gain: 0.6)
    }

    static func soft(grace: Double, main: Double, gap: Double, graceGain: Double,
                     tailLevel: Double, tailDecay: Double, length: Double) -> Voice {
        Voice(
            notes: [Note(frequency: grace, onset: 0, gain: graceGain), Note(frequency: main, onset: gap)],
            length: length, decay: 180, tailLevel: tailLevel, tailDecay: tailDecay,
            harmonics: [1, 0.04], attack: 0.004, gain: 0.6
        )
    }

    private static let sampleRate = 44_100.0

    /// Renders the voice as a 16-bit mono WAV file.
    func render() -> Data {
        let sr = Self.sampleRate
        let total = Int(sr * length)
        var samples = [Float](repeating: 0, count: total)

        for note in notes {
            let offset = Int(sr * note.onset)
            guard offset < total else { continue }
            var phase = 0.0
            for i in 0..<(total - offset) {
                let t = Double(i) / sr
                let glide = 1 + (glideFrom - 1) * exp(-t * glideSpeed)
                phase += 2 * .pi * note.frequency * glide / sr
                let rise = min(t / attack, 1)
                let fall = (1 - tailLevel) * exp(-t * decay) + tailLevel * exp(-t * tailDecay)
                var wave = 0.0
                for (n, amplitude) in harmonics.enumerated() { wave += amplitude * sin(Double(n + 1) * phase) }
                samples[offset + i] += Float(wave * rise * fall * note.gain * gain)
            }
        }

        let fade = min(Int(sr * 0.008), total)
        for i in 0..<fade { samples[total - 1 - i] *= Float(i) / Float(fade) }
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
