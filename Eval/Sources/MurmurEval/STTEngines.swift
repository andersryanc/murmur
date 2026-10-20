import MurmurCore

/// Engines the eval can compare, by command-line name.
enum Engines {
    static let all: [String: @Sendable () -> any STTEngine] = [
        "parakeet": { ParakeetEngine(id: "parakeet", version: .v3) },
        "parakeet-v2": { ParakeetEngine(id: "parakeet-v2", version: .v2) },
        "parakeet-ultra": { ParakeetEngine(id: "parakeet-ultra", version: .ultra) },
        "apple": { AppleSpeechEngine() },
    ]
    static let defaults = ["parakeet", "apple"]

    static func make(_ ids: [String]) throws -> [any STTEngine] {
        try ids.map { id in
            guard let factory = all[id] else {
                throw EvalError("Unknown engine '\(id)'. Available: \(all.keys.sorted().joined(separator: ", "))")
            }
            return factory()
        }
    }
}
