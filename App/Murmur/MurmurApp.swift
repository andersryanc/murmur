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
