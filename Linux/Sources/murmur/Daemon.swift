import Foundation
import MurmurKit

/// Runs the push-to-talk loop: start → record → stop → transcribe → clean → paste. Commands
/// arrive over the control socket from Hyprland key bindings (`murmur start` on press,
/// `murmur stop` on release).
@MainActor
final class Daemon {
    enum Phase: String {
        case ready, recording, processing, cancelling
    }

    /// Holds shorter than this are treated as accidental taps.
    private static let minimumHold = Duration.milliseconds(300)

    private var phase = Phase.ready {
        didSet { watchers.send("state \(phase.rawValue)") }
    }
    private var config = Config.load()
    private let transcriber: Transcriber
    private let recorder = Recorder()
    private let watchers = Watchers()
    private var recordingStarted = ContinuousClock.now
    /// Looked up when recording starts, so the paste goes where you were when you began.
    private var windowClass: Task<String?, Never>?

    init(transcriber: Transcriber) {
        self.transcriber = transcriber
        recorder.onLevel = { [weak self] level in
            guard let self, phase == .recording else { return }
            watchers.send("level \((level * 100).rounded() / 100)")
        }
    }

    /// Sends events to a client that asked to watch, starting with the current state.
    func watch(_ fd: Int32) {
        watchers.add(fd, greeting: "state \(phase.rawValue)")
    }

    /// Shows `message` in the pill when the Omarchy plugin is watching, or as a notification.
    private func notice(_ message: String) {
        // Sending first drops watchers that have gone away (e.g. the shell restarted).
        watchers.send("notice \(message)")
        if watchers.isEmpty { Desktop.notify(message) }
    }

    func handle(_ command: String) -> String {
        switch command {
        case "start": start()
        case "stop": stop()
        case "toggle": phase == .recording ? stop() : start()
        case "cancel": cancel()
        case "status": phase.rawValue
        default: "unknown command: \(command)"
        }
    }

    private func start() -> String {
        guard phase == .ready else { return "busy: \(phase.rawValue)" }
        config = Config.load()
        do {
            try recorder.start(microphone: config.microphone)
        } catch {
            Log.info("Couldn't start pw-record: \(error)")
            notice("Couldn't start the microphone")
            return "error: couldn't start pw-record"
        }
        phase = .recording
        recordingStarted = .now
        windowClass = Task { await Desktop.activeWindowClass() }
        Sounds.play(.start, theme: config.soundTheme)
        return "recording"
    }

    private func stop() -> String {
        guard phase == .recording else { return "not recording" }
        phase = .processing
        Sounds.play(.stop, theme: config.soundTheme)
        let held = ContinuousClock.now - recordingStarted
        Task {
            await finish(held: held)
            phase = .ready
        }
        return "processing"
    }

    private func cancel() -> String {
        guard phase == .recording else { return "not recording" }
        phase = .cancelling
        Sounds.play(.stop, theme: config.soundTheme)
        Task {
            _ = await recorder.stop()
            phase = .ready
        }
        return "cancelled"
    }

    private func finish(held: Duration) async {
        let samples = await recorder.stop()
        let target = await windowClass?.value

        // Too short, or no voice in it (a tap of the key): speech models invent words from
        // silence, so don't transcribe at all.
        guard held >= Self.minimumHold, VoiceActivity.containsSpeech(samples) else {
            Log.info("No speech in \(held.formatted(.units(allowed: [.milliseconds]))) recording, skipped")
            return
        }

        let clock = ContinuousClock()
        let speechStart = clock.now
        let transcriber = transcriber
        let raw = await Task.detached { transcriber.transcribe(samples) }.value
        let speech = clock.now - speechStart
        guard !raw.isEmpty else { return }

        // Replacements run first and always, so they work with cleanup off and the cleanup
        // model sees the corrected terms.
        let dictionary: PersonalDictionary
        do {
            dictionary = try PersonalDictionary.load()
        } catch {
            Log.info("Ignoring \(PersonalDictionary.defaultURL.path): \(error)")
            dictionary = PersonalDictionary()
        }
        let corrected = dictionary.applyReplacements(to: raw)
        let context = TargetContext(windowClass: target)

        var text = corrected
        var timing = "speech \(Self.ms(speech))"
        do {
            if let cleaner = try config.makeCleaner(vocabulary: dictionary.vocabulary) {
                let cleanupStart = clock.now
                text = try await cleaner.clean(corrected, context: context)
                timing += ", cleanup \(Self.ms(clock.now - cleanupStart)) (\(cleaner.id))"
            }
        } catch {
            Log.info("Cleanup failed, pasting raw text: \(error)")
            notice("Cleanup failed, pasted raw text")
        }
        guard !text.isEmpty else { return }

        let pasted = await Desktop.paste(text, windowClass: target)
        if !pasted { notice("Copied. Paste with Ctrl+V") }
        Log.info("Dictated into \(target ?? "nothing") [\(context.rawValue)], \(timing): \(text)")
    }

    private static func ms(_ duration: Duration) -> String {
        "\(Int(duration / .milliseconds(1))) ms"
    }
}
