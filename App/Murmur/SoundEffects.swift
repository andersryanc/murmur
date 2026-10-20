import AVFoundation
import MurmurCore

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
        start = Self.player(theme.startSound())
        stop = Self.player(theme.stopSound())
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
