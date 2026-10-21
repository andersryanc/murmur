import Foundation
import MurmurKit

/// Runs the push-to-talk loop: start → record → stop → transcribe → clean → paste. Commands
/// arrive over the control socket from Hyprland key bindings (`murmur start` on press,
/// `murmur stop` on release).
///
/// A new recording can start while earlier ones are still being processed. Finished recordings
/// wait in a queue and are processed one at a time, so they paste in the order they were spoken.
@MainActor
final class Daemon {
    enum Phase: String {
        case ready, recording, processing
    }

    /// Holds shorter than this are treated as accidental taps.
    private static let minimumHold = Duration.milliseconds(300)

    private var recording = false {
        didSet { publish() }
    }
    /// Recordings stopped but not yet pasted, including the one being processed.
    private var queued = 0 {
        didSet { publish() }
    }
    /// The last recording in the queue; the next one waits for it.
    private var lastInQueue: Task<Void, Never>?
    private var phase: Phase {
        recording ? .recording : queued > 0 ? .processing : .ready
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
            guard let self, recording else { return }
            watchers.send("level \((level * 100).rounded() / 100)")
        }
    }

    /// Sends events to a client that asked to watch, starting with the current state.
    func watch(_ fd: Int32) {
        watchers.add(fd, greeting: ["queue \(queued)", "state \(phase.rawValue)"])
    }

    /// The queue length goes first, so a watcher already knows it when the state changes.
    private func publish() {
        watchers.send("queue \(queued)")
        watchers.send("state \(phase.rawValue)")
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
        case "toggle": recording ? stop() : start()
        case "cancel": cancel()
        case "status": phase.rawValue
        default: "unknown command: \(command)"
        }
    }

    private func start() -> String {
        guard !recording else { return "already recording" }
        config = Config.load()
        do {
            try recorder.start(microphone: config.microphone)
        } catch {
            Log.info("Couldn't start pw-record: \(error)")
            notice("Couldn't start the microphone")
            return "error: couldn't start pw-record"
        }
        recording = true
        recordingStarted = .now
        windowClass = Task { await Desktop.activeWindowClass() }
        Sounds.play(.start, theme: config.soundTheme)
        return "recording"
    }

    private func stop() -> String {
        guard recording else { return "not recording" }
        Sounds.play(.stop, theme: config.soundTheme)
        let samples = recorder.stop()
        let held = ContinuousClock.now - recordingStarted
        let target = windowClass
        let config = config
        let previous = lastInQueue
        queued += 1
        recording = false
        lastInQueue = Task {
            let samples = await samples.value
            await previous?.value
            await finish(samples, held: held, target: await target?.value, config: config)
            queued -= 1
        }
        return queued > 1 ? "queued" : "processing"
    }

    private func cancel() -> String {
        guard recording else { return "not recording" }
        Sounds.play(.stop, theme: config.soundTheme)
        _ = recorder.stop()
        recording = false
        return "cancelled"
    }

    private func finish(_ samples: [Float], held: Duration, target: String?, config: Config) async {
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
