import AppKit
import MurmurCore
import Observation
import ServiceManagement

/// One finished dictation, kept in the menu so you can copy it again.
struct Dictation: Identifiable {
    let id = UUID()
    let text: String
    let raw: String
    let appName: String?
    let date = Date()
}

/// How the raw transcript gets cleaned up before pasting.
enum CleanupMode: String, CaseIterable, Identifiable {
    /// Paste Parakeet's transcript as-is. It has basic punctuation and casing, but keeps filler
    /// words and doesn't apply self-corrections or list formatting.
    case off
    /// Claude Haiku 5.5 on the Anthropic API.
    case claude
    /// A model served by Ollama on this Mac.
    case local

    var id: String { rawValue }

    var label: String {
        switch self {
        case .off: "Off: Paste Raw Transcript"
        case .claude: "Claude Haiku (Cloud)"
        case .local: "Local Model (Ollama)"
        }
    }
}

/// How long each stage of the last dictation took, shown in the menu.
struct Timing {
    let speech: Duration
    let cleanup: Duration?
    let cleaner: String?

    var summary: String {
        var text = "Last: speech \(Self.ms(speech))"
        if let cleanup, let cleaner { text += " · cleanup \(Self.ms(cleanup)) (\(cleaner))" }
        return text
    }

    private static func ms(_ duration: Duration) -> String {
        "\(Int(duration / .milliseconds(1))) ms"
    }
}

/// Runs the whole push-to-talk loop: key down → record → key up → transcribe → clean → paste.
@Observable
final class AppModel {
    enum Phase: Equatable {
        case loading(String)
        case ready
        case recording
        case processing
        case failed(String)
    }

    private(set) var phase: Phase = .loading("Starting…")
    /// Recent loudness values for the HUD's level meter, newest last.
    private(set) var levels: [Float] = Array(repeating: 0, count: AppModel.levelCount)
    private(set) var history: [Dictation] = []
    private(set) var microphoneGranted = Permissions.microphoneGranted
    private(set) var accessibilityGranted = Permissions.accessibilityGranted
    private(set) var hasAPIKey = false
    /// Shown in the HUD for a moment after a dictation, e.g. when cleanup failed.
    private(set) var notice: String?
    private(set) var lastTiming: Timing?
    /// Models pulled into Ollama, for the local model picker.
    private(set) var localModels: [String] = []
    /// Connected microphones, kept current as devices come and go.
    private(set) var inputDevices: [AudioInputDevice] = []
    private(set) var defaultInput: AudioInputDevice?

    var hotkey: Hotkey {
        didSet {
            UserDefaults.standard.set(hotkey.rawValue, forKey: "hotkey")
            hotkeys.hotkey = hotkey
        }
    }

    var cleanup: CleanupMode {
        didSet {
            UserDefaults.standard.set(cleanup.rawValue, forKey: "cleanup")
            if cleanup == .local { prepareLocalModel() }
        }
    }

    var localModel: String {
        didSet {
            UserDefaults.standard.set(localModel, forKey: "localModel")
            prepareLocalModel()
        }
    }

    /// Core Audio UID of the chosen microphone, or nil to follow the system default input.
    var microphoneUID: String? {
        didSet { UserDefaults.standard.set(microphoneUID, forKey: "microphoneUID") }
    }

    /// Start/stop sounds, or nil for silence. Picking a theme plays its start sound as a preview.
    var soundTheme: SoundTheme? {
        didSet {
            UserDefaults.standard.set(soundTheme?.rawValue ?? "off", forKey: "soundTheme")
            if let soundTheme {
                sounds.theme = soundTheme
                sounds.playStart()
            }
        }
    }

    var launchAtLogin: Bool {
        get { SMAppService.mainApp.status == .enabled }
        set {
            do {
                if newValue { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
            } catch {
                flash("Couldn't change Login Items: \(error.localizedDescription)")
            }
        }
    }

    static let levelCount = 14
    /// Holds shorter than this are treated as accidental taps.
    private static let minimumHold: TimeInterval = 0.3

    private let engine = ParakeetEngine()
    private let capture = AudioCapture()
    private let hotkeys = HotkeyMonitor()
    private var hud: HUDController?
    private var recordingStarted: Date?
    private var targetApp: NSRunningApplication?
    private var chordCancelled = false
    private let sounds = SoundEffects()
    /// Your words and replacements, edited in the Dictionary window.
    let dictionary = DictionaryStore()

    init() {
        let defaults = UserDefaults.standard
        hotkey = defaults.string(forKey: "hotkey").flatMap(Hotkey.init) ?? .fn
        // `cleanupEnabled` was the original on/off toggle; carry an "off" choice forward.
        let legacyOff = defaults.object(forKey: "cleanupEnabled") as? Bool == false
        cleanup = defaults.string(forKey: "cleanup").flatMap(CleanupMode.init) ?? (legacyOff ? .off : .claude)
        localModel = defaults.string(forKey: "localModel") ?? "qwen3.5:9b"
        microphoneUID = defaults.string(forKey: "microphoneUID")
        // `soundEffects` was the original on/off toggle; carry an "off" choice forward.
        if let saved = defaults.string(forKey: "soundTheme") {
            soundTheme = SoundTheme(rawValue: saved)
        } else {
            soundTheme = defaults.object(forKey: "soundEffects") as? Bool == false ? nil : .wood
        }
        sounds.theme = soundTheme ?? .wood

        hotkeys.hotkey = hotkey
        hotkeys.onPress = { [weak self] in self?.startRecording() }
        hotkeys.onRelease = { [weak self] in self?.finishRecording() }
        hotkeys.onChord = { [weak self] in self?.cancelForChord() }

        capture.onLevel = { [weak self] level in
            Task { @MainActor in self?.push(level: level) }
        }

        refreshInputDevices()
        AudioDevices.observeChanges { [weak self] in
            Task { @MainActor in self?.refreshInputDevices() }
        }

        hud = HUDController(model: self)
        Task { await start() }
    }

    var statusText: String {
        switch phase {
        case .loading(let message): message
        case .ready: "Hold \(hotkey.shortLabel) to dictate"
        case .recording: "Listening…"
        case .processing: "Transcribing…"
        case .failed(let message): message
        }
    }

    // MARK: - Startup

    private func start() async {
        hasAPIKey = APIKey.load() != nil

        if !microphoneGranted {
            microphoneGranted = await Permissions.requestMicrophone()
        }
        if !accessibilityGranted {
            Permissions.promptForAccessibility()
        }
        hotkeys.install()
        watchAccessibility()
        await refreshLocalModels()
        if cleanup == .local { prepareLocalModel() }

        phase = .loading("Loading speech model…")
        do {
            try await engine.prepare()
            // Warm the Neural Engine so the first real dictation isn't slow.
            _ = try? await engine.transcribe([Float](repeating: 0, count: Int(sttSampleRate)))
            phase = .ready
        } catch {
            phase = .failed("Speech model failed to load: \(error.localizedDescription)")
        }
    }

    /// Accessibility is granted in System Settings while the app is running, and the hotkey's
    /// event tap can only be created once it is, so poll and reinstall when it flips.
    private func watchAccessibility() {
        Task {
            while true {
                try? await Task.sleep(for: .seconds(1))
                let granted = Permissions.accessibilityGranted
                if granted != accessibilityGranted {
                    accessibilityGranted = granted
                    if granted { hotkeys.install() }
                }
                microphoneGranted = Permissions.microphoneGranted
            }
        }
    }

    func refreshAPIKey() {
        hasAPIKey = APIKey.load() != nil
    }

    func refreshLocalModels() async {
        localModels = await OllamaCleaner.installedModels()
    }

    /// Loads the local model into memory now, so the next dictation doesn't wait for it.
    private func prepareLocalModel() {
        let cleaner = OllamaCleaner(model: localModel)
        Task {
            do {
                try await cleaner.prepare()
            } catch {
                flash("Local model \(localModel) isn't ready: \(error)")
            }
        }
    }

    private func refreshInputDevices() {
        inputDevices = AudioDevices.inputs()
        defaultInput = AudioDevices.defaultInput()
    }

    // MARK: - Push to talk

    private func startRecording() {
        guard phase == .ready else { return }
        guard microphoneGranted else {
            flash("Murmur needs microphone access")
            return
        }
        do {
            try capture.start(deviceUID: microphoneUID)
        } catch {
            flash("Couldn't start the microphone: \(error)")
            return
        }
        targetApp = NSWorkspace.shared.frontmostApplication
        recordingStarted = Date()
        chordCancelled = false
        notice = nil
        levels = Array(repeating: 0, count: Self.levelCount)
        phase = .recording
        hud?.show()
        if soundTheme != nil { sounds.playStart() }
    }

    /// Recording just ended, whether it will be transcribed or thrown away.
    private func endChime() {
        if soundTheme != nil { sounds.playStop() }
    }

    private func cancelForChord() {
        guard phase == .recording else { return }
        chordCancelled = true
        endChime()
        _ = try? capture.stop()
        phase = .ready
        hud?.hide()
    }

    private func finishRecording() {
        guard phase == .recording, !chordCancelled else { return }
        let held = Date().timeIntervalSince(recordingStarted ?? Date())
        endChime()
        let samples = (try? capture.stop()) ?? []

        // Too short, or no voice in it (a tap of the key): speech models invent words from
        // silence, so don't transcribe at all.
        guard held >= Self.minimumHold, VoiceActivity.containsSpeech(samples) else {
            phase = .ready
            hud?.hide()
            return
        }

        phase = .processing
        let context = TargetContext(bundleID: targetApp?.bundleIdentifier)
        let appName = targetApp?.localizedName
        Task { await process(samples, context: context, appName: appName) }
    }

    private func process(_ samples: [Float], context: TargetContext, appName: String?) async {
        defer {
            if phase == .processing { phase = .ready }
            hud?.hide(after: notice == nil ? 0 : 2.5)
        }

        let clock = ContinuousClock()
        let raw: String
        let speechStart = clock.now
        do {
            raw = try await engine.transcribe(samples)
        } catch {
            notice = "Transcription failed"
            return
        }
        let speech = clock.now - speechStart
        guard !raw.isEmpty else { return }

        // Replacements run first and always, so they work with cleanup off and the cleanup
        // model sees the corrected terms.
        dictionary.reloadIfChangedOnDisk()
        let personal = dictionary.dictionary
        let corrected = personal.applyReplacements(to: raw)

        var text = corrected
        var cleanupTime: Duration?
        let cleaner = makeCleaner(vocabulary: personal.vocabulary)
        if let cleaner {
            let cleanupStart = clock.now
            do {
                text = try await cleaner.clean(corrected, context: context)
                cleanupTime = clock.now - cleanupStart
            } catch {
                notice = "Cleanup failed, pasted raw text"
            }
        }
        lastTiming = Timing(speech: speech, cleanup: cleanupTime, cleaner: cleanupTime == nil ? nil : cleaner?.id)
        guard !text.isEmpty else { return }

        switch Paster.paste(text) {
        case .pasted: break
        case .copiedOnly: notice = "Copied. Press ⌘V to paste"
        }
        history.insert(Dictation(text: text, raw: raw, appName: appName), at: 0)
        if history.count > 10 { history.removeLast() }
    }

    /// The cleaner for the current mode, or nil to paste the raw transcript.
    private func makeCleaner(vocabulary: [String]) -> (any TextCleaner)? {
        switch cleanup {
        case .off:
            return nil
        case .claude:
            guard let key = APIKey.load() else {
                hasAPIKey = false
                notice = "No API key, pasted raw text"
                return nil
            }
            hasAPIKey = true
            return ClaudeCleaner(apiKey: key, vocabulary: vocabulary)
        case .local:
            return OllamaCleaner(model: localModel, vocabulary: vocabulary)
        }
    }

    // MARK: - Helpers

    private func push(level: Float) {
        guard phase == .recording else { return }
        levels.removeFirst()
        levels.append(level)
    }

    private func flash(_ message: String) {
        notice = message
        hud?.show()
        hud?.hide(after: 2.5)
    }

    func copy(_ dictation: Dictation) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(dictation.text, forType: .string)
    }
}
