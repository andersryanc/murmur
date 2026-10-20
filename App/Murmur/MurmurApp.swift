import MurmurCore
import SwiftUI

@main
struct MurmurApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        MenuBarExtra {
            MenuContent(model: model)
        } label: {
            Image(systemName: model.phase == .recording ? "waveform.circle.fill" : "waveform")
        }
        .menuBarExtraStyle(.menu)
    }
}

private struct MenuContent: View {
    @Bindable var model: AppModel

    var body: some View {
        Text(model.statusText)
        if let timing = model.lastTiming {
            Text(timing.summary)
        }

        if !model.accessibilityGranted {
            Button("Grant Accessibility Access…") { Permissions.openAccessibilitySettings() }
        }
        if !model.microphoneGranted {
            Button("Grant Microphone Access…") { Permissions.openMicrophoneSettings() }
        }
        if model.cleanup == .claude && !model.hasAPIKey {
            Button("No Anthropic API key found. Check again") { model.refreshAPIKey() }
        }

        Divider()

        if model.history.isEmpty {
            Text("No dictations yet")
        } else {
            Section("Recent: click to copy") {
                ForEach(model.history.prefix(5)) { dictation in
                    Button(preview(dictation.text)) { model.copy(dictation) }
                }
            }
        }

        Divider()

        Picker("Push-to-Talk Key", selection: $model.hotkey) {
            ForEach(Hotkey.allCases) { Text($0.label).tag($0) }
        }
        Picker("Cleanup", selection: $model.cleanup) {
            ForEach(CleanupMode.allCases) { Text($0.label).tag($0) }
        }
        if model.cleanup == .local {
            Picker("Local Model", selection: $model.localModel) {
                // Keep the current choice listed even if Ollama doesn't have it yet.
                ForEach(Array(Set(model.localModels + [model.localModel])).sorted(), id: \.self) { name in
                    Text(model.localModels.contains(name) ? name : "\(name) (not installed)").tag(name)
                }
            }
            Button("Refresh Local Models") { Task { await model.refreshLocalModels() } }
        }
        Picker("Microphone", selection: $model.microphoneUID) {
            Text(model.defaultInput.map { "System Default (\($0.name))" } ?? "System Default")
                .tag(String?.none)
            Divider()
            ForEach(model.inputDevices) { device in
                Text(device.name).tag(Optional(device.uid))
            }
            // A saved mic that's unplugged right now: keep it selected so it's used again on reconnect.
            if let uid = model.microphoneUID, !model.inputDevices.contains(where: { $0.uid == uid }) {
                Text("Disconnected Microphone (using system default)").tag(Optional(uid))
            }
        }
        Picker("Sound Effects", selection: $model.soundTheme) {
            ForEach(SoundTheme.allCases) { Text($0.label).tag(Optional($0)) }
            Divider()
            Text("Off").tag(SoundTheme?.none)
        }
        Toggle("Launch at Login", isOn: $model.launchAtLogin)

        Divider()

        Button("Quit Murmur") { NSApplication.shared.terminate(nil) }
            .keyboardShortcut("q")
    }

    private func preview(_ text: String) -> String {
        let flat = text.replacingOccurrences(of: "\n", with: " ")
        return flat.count > 48 ? String(flat.prefix(48)) + "…" : flat
    }
}
