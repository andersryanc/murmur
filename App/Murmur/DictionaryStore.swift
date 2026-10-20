import AppKit
import MurmurCore
import Observation

/// The personal dictionary as the app sees it: loaded from `dictionary.json`, saved on every
/// edit in the Dictionary window, and re-read if the file is changed outside the app (a text
/// editor, a sync service), so the file is always the source of truth.
@Observable
final class DictionaryStore {
    var dictionary: PersonalDictionary {
        didSet {
            guard dictionary != oldValue else { return }
            save()
        }
    }

    /// Set when the file couldn't be read or written, shown in the Dictionary window.
    private(set) var error: String?

    let url: URL
    @ObservationIgnored private var loadedModificationDate: Date?

    init(url: URL = PersonalDictionary.defaultURL) {
        self.url = url
        dictionary = PersonalDictionary()
        reload()
    }

    /// Re-reads the file if it changed since we last loaded or saved it. Cheap: one stat call.
    func reloadIfChangedOnDisk() {
        guard modificationDate() != loadedModificationDate else { return }
        reload()
    }

    func showInFinder() {
        if !FileManager.default.fileExists(atPath: url.path) { save() }
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }

    private func reload() {
        do {
            let loaded = try PersonalDictionary.load(from: url)
            loadedModificationDate = modificationDate()
            if loaded != dictionary { dictionary = loaded }
            error = nil
        } catch {
            self.error = "Couldn't read \(url.lastPathComponent): \(error.localizedDescription)"
        }
    }

    private func save() {
        do {
            try dictionary.save(to: url)
            loadedModificationDate = modificationDate()
            error = nil
        } catch {
            self.error = "Couldn't save \(url.lastPathComponent): \(error.localizedDescription)"
        }
    }

    private func modificationDate() -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: url.path))?[.modificationDate] as? Date
    }
}
