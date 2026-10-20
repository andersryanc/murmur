import Foundation

/// Your words and fixes, shared by the app and the eval.
///
/// - **Words** are correct spellings (names, jargon, product names). The cleanup model gets them
///   as preferred spellings, so it can fix near-misses from context.
/// - **Replacements** are exact "when I say X, write Y" rules, applied to the transcript before
///   cleanup. They work even with cleanup off.
///
/// Stored as plain, human-editable JSON:
/// ```json
/// {
///   "replacements" : [ { "from" : "cube control", "to" : "kubectl" } ],
///   "words" : [ "Plyhdrn", "Kubernetes" ]
/// }
/// ```
public struct PersonalDictionary: Codable, Equatable, Sendable {
    public var words: [Word]
    public var replacements: [Replacement]

    public init(words: [Word] = [], replacements: [Replacement] = []) {
        self.words = words
        self.replacements = replacements
    }

    /// One correct spelling. Encoded as a bare string; the id only exists for editing in lists.
    public struct Word: Codable, Equatable, Identifiable, Sendable {
        public var id = UUID()
        public var text: String

        public init(_ text: String) { self.text = text }

        public init(from decoder: any Decoder) throws {
            text = try decoder.singleValueContainer().decode(String.self)
        }

        public func encode(to encoder: any Encoder) throws {
            var container = encoder.singleValueContainer()
            try container.encode(text)
        }

        public static func == (a: Word, b: Word) -> Bool { a.text == b.text }
    }

    public struct Replacement: Codable, Equatable, Identifiable, Sendable {
        public var id = UUID()
        /// What the speech recognizer writes when you say it, e.g. "cube control".
        public var from: String
        /// What should be typed instead, e.g. "kubectl".
        public var to: String

        public init(from: String, to: String) {
            self.from = from
            self.to = to
        }

        private enum CodingKeys: String, CodingKey { case from, to }

        public static func == (a: Replacement, b: Replacement) -> Bool { a.from == b.from && a.to == b.to }
    }

    // MARK: - Using it

    /// Spellings for the cleanup model: your words, plus every replacement's output so the model
    /// keeps those exactly as written (e.g. doesn't capitalize "kubectl").
    public var vocabulary: [String] {
        var seen = Set<String>()
        return (words.map(\.text) + replacements.map(\.to))
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty && seen.insert($0.lowercased()).inserted }
    }

    /// Applies every replacement to `text`. Matching ignores case, only matches whole words
    /// (so "art" never changes "start"), and treats spaces, hyphens and commas between words as
    /// interchangeable, since recognizers vary ("cube control", "Cube-control", "cube, control").
    /// Longer phrases are applied first, so a specific rule wins over a shorter one inside it.
    public func applyReplacements(to text: String) -> String {
        let rules = replacements
            .map { (from: $0.from.trimmingCharacters(in: .whitespacesAndNewlines), to: $0.to) }
            .filter { !$0.from.isEmpty }
            .sorted { $0.from.count > $1.from.count }

        var result = text
        for rule in rules {
            guard let regex = Self.pattern(for: rule.from) else { continue }
            let range = NSRange(result.startIndex..., in: result)
            result = regex.stringByReplacingMatches(
                in: result, range: range, withTemplate: NSRegularExpression.escapedTemplate(for: rule.to)
            )
        }
        return result
    }

    private static func pattern(for phrase: String) -> NSRegularExpression? {
        let words = phrase
            .components(separatedBy: CharacterSet.whitespaces.union(CharacterSet(charactersIn: "-,")))
            .filter { !$0.isEmpty }
            .map(NSRegularExpression.escapedPattern(for:))
        guard !words.isEmpty else { return nil }
        let body = words.joined(separator: #"[\s\-,]+"#)
        return try? NSRegularExpression(
            pattern: #"(?<![\p{L}\p{N}])"# + body + #"(?![\p{L}\p{N}])"#,
            options: [.caseInsensitive]
        )
    }

    // MARK: - File

    /// `~/Library/Application Support/Murmur/dictionary.json` on macOS;
    /// `$XDG_CONFIG_HOME/murmur/dictionary.json` (usually `~/.config/murmur/`) on Linux.
    public static var defaultURL: URL {
        #if os(macOS)
        URL.applicationSupportDirectory.appending(path: "Murmur/dictionary.json")
        #else
        let config = ProcessInfo.processInfo.environment["XDG_CONFIG_HOME"].flatMap { $0.isEmpty ? nil : $0 }
            .map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appending(path: ".config")
        return config.appending(path: "murmur/dictionary.json")
        #endif
    }

    /// The dictionary at `url`, or an empty one if the file doesn't exist yet.
    public static func load(from url: URL = defaultURL) throws -> PersonalDictionary {
        guard FileManager.default.fileExists(atPath: url.path) else { return PersonalDictionary() }
        return try JSONDecoder().decode(PersonalDictionary.self, from: Data(contentsOf: url))
    }

    public func save(to url: URL = defaultURL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        try encoder.encode(self).write(to: url, options: .atomic)
    }
}
