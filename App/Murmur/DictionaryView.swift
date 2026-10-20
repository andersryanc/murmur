import MurmurCore
import SwiftUI

/// The Dictionary window: your words (spelling hints for cleanup) and replacements (exact
/// "when I say X, write Y" rules). Every edit is saved to `dictionary.json` immediately.
struct DictionaryView: View {
    @Bindable var store: DictionaryStore

    private enum Tab: String, CaseIterable, Identifiable {
        case words = "Words"
        case replacements = "Replacements"
        var id: String { rawValue }
    }

    @State private var tab: Tab = .words
    @State private var search = ""
    @State private var selectedWords = Set<UUID>()
    @State private var selectedReplacements = Set<UUID>()
    @FocusState private var focused: UUID?

    var body: some View {
        VStack(spacing: 0) {
            VStack(spacing: 10) {
                Picker("", selection: $tab) {
                    ForEach(Tab.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 280)

                Text(explanation)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 14)

            switch tab {
            case .words: wordsList
            case .replacements: replacementsTable
            }

            Divider()
            bottomBar
        }
        .searchable(text: $search, placement: .toolbar, prompt: tab == .words ? "Search words" : "Search replacements")
        .frame(minWidth: 520, minHeight: 400)
        .onAppear { store.reloadIfChangedOnDisk() }
    }

    private var explanation: String {
        switch tab {
        case .words:
            "Names, jargon and other words Murmur should spell your way. The cleanup step uses them to fix near-misses."
        case .replacements:
            "Exact fixes applied to every dictation, even with cleanup off. Matching ignores case and only replaces whole words."
        }
    }

    // MARK: - Words

    private var filteredWords: [PersonalDictionary.Word] {
        let words = store.dictionary.words
        guard !search.isEmpty else { return words }
        return words.filter { $0.text.localizedCaseInsensitiveContains(search) }
    }

    private var wordsList: some View {
        List(selection: $selectedWords) {
            ForEach(filteredWords) { word in
                TextField("Word or name", text: wordBinding(word.id))
                    .textFieldStyle(.plain)
                    .focused($focused, equals: word.id)
            }
        }
        .overlay {
            if store.dictionary.words.isEmpty {
                emptyState("No words yet", detail: "Add names, product names and jargon, like “Plyhdrn” or “Kubernetes”.")
            }
        }
        .onDeleteCommand(perform: removeSelected)
    }

    private func wordBinding(_ id: UUID) -> Binding<String> {
        Binding(
            get: { store.dictionary.words.first { $0.id == id }?.text ?? "" },
            set: { text in
                if let index = store.dictionary.words.firstIndex(where: { $0.id == id }) {
                    store.dictionary.words[index].text = text
                }
            }
        )
    }

    // MARK: - Replacements

    private var filteredReplacements: [PersonalDictionary.Replacement] {
        let rules = store.dictionary.replacements
        guard !search.isEmpty else { return rules }
        return rules.filter { $0.from.localizedCaseInsensitiveContains(search) || $0.to.localizedCaseInsensitiveContains(search) }
    }

    private var replacementsTable: some View {
        Table(filteredReplacements, selection: $selectedReplacements) {
            TableColumn("When I say") { rule in
                TextField("cube control", text: replacementBinding(rule.id, \.from))
                    .textFieldStyle(.plain)
                    .focused($focused, equals: rule.id)
            }
            TableColumn("Write") { rule in
                TextField("kubectl", text: replacementBinding(rule.id, \.to))
                    .textFieldStyle(.plain)
            }
        }
        .overlay {
            if store.dictionary.replacements.isEmpty {
                emptyState("No replacements yet", detail: "Add a phrase the way it gets transcribed and what you want instead, like “cube control” → “kubectl”.")
            }
        }
        .onDeleteCommand(perform: removeSelected)
    }

    private func replacementBinding(
        _ id: UUID, _ field: WritableKeyPath<PersonalDictionary.Replacement, String>
    ) -> Binding<String> {
        Binding(
            get: { store.dictionary.replacements.first { $0.id == id }?[keyPath: field] ?? "" },
            set: { text in
                if let index = store.dictionary.replacements.firstIndex(where: { $0.id == id }) {
                    store.dictionary.replacements[index][keyPath: field] = text
                }
            }
        )
    }

    // MARK: - Bottom bar

    private var bottomBar: some View {
        HStack(spacing: 8) {
            ControlGroup {
                Button(action: add) { Image(systemName: "plus") }
                    .help(tab == .words ? "Add a word" : "Add a replacement")
                Button(action: removeSelected) { Image(systemName: "minus") }
                    .help("Remove the selected entries")
                    .disabled(tab == .words ? selectedWords.isEmpty : selectedReplacements.isEmpty)
            }
            .frame(width: 64)

            if let error = store.error {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .lineLimit(1)
            } else {
                Text(countText)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Button("Show in Finder") { store.showInFinder() }
        }
        .font(.callout)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private var countText: String {
        let words = store.dictionary.words.count
        let rules = store.dictionary.replacements.count
        return "\(words) word\(words == 1 ? "" : "s"), \(rules) replacement\(rules == 1 ? "" : "s")"
    }

    private func emptyState(_ title: String, detail: String) -> some View {
        ContentUnavailableView {
            Label(title, systemImage: tab == .words ? "character.book.closed" : "arrow.left.arrow.right")
        } description: {
            Text(detail)
        } actions: {
            Button(tab == .words ? "Add Word" : "Add Replacement", action: add)
        }
    }

    // MARK: - Actions

    private func add() {
        search = ""
        switch tab {
        case .words:
            let word = PersonalDictionary.Word("")
            store.dictionary.words.append(word)
            selectedWords = [word.id]
            focusSoon(word.id)
        case .replacements:
            let rule = PersonalDictionary.Replacement(from: "", to: "")
            store.dictionary.replacements.append(rule)
            selectedReplacements = [rule.id]
            focusSoon(rule.id)
        }
    }

    private func removeSelected() {
        switch tab {
        case .words:
            store.dictionary.words.removeAll { selectedWords.contains($0.id) }
            selectedWords.removeAll()
        case .replacements:
            store.dictionary.replacements.removeAll { selectedReplacements.contains($0.id) }
            selectedReplacements.removeAll()
        }
    }

    /// The new row's text field exists only after the next layout pass.
    private func focusSoon(_ id: UUID) {
        DispatchQueue.main.async { focused = id }
    }
}
