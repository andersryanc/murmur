import Foundation
import MurmurCore

struct EngineResult: Codable {
    let engine: String
    let raw: String
    let sttSeconds: Double
    let wer: Double?
    var cleaned: String?
    var cleanupSeconds: Double?
    var error: String?
}

struct ClipResult: Codable {
    let clip: String
    let context: TargetContext
    let audioSeconds: Double
    let reference: String?
    let intended: String?
    var engines: [EngineResult]
}

// MARK: - Word error rate

/// Lowercase, drop punctuation (keeping apostrophes inside words), split on whitespace.
func normalizedWords(_ text: String) -> [String] {
    let kept = text.lowercased().unicodeScalars.map { scalar -> Character in
        CharacterSet.alphanumerics.contains(scalar) || scalar == "'" ? Character(scalar) : " "
    }
    return String(kept).split(separator: " ").map { $0.trimmingCharacters(in: CharacterSet(charactersIn: "'")) }
        .filter { !$0.isEmpty }
}

/// (substitutions + deletions + insertions) / reference length, via word-level edit distance.
func wordErrorRate(hypothesis: String, reference: String) -> Double {
    let ref = normalizedWords(reference)
    let hyp = normalizedWords(hypothesis)
    guard !ref.isEmpty else { return hyp.isEmpty ? 0 : 1 }
    var previous = Array(0...hyp.count)
    for i in 1...ref.count {
        var current = [i] + Array(repeating: 0, count: hyp.count)
        for j in stride(from: 1, through: hyp.count, by: 1) {
            let cost = ref[i - 1] == hyp[j - 1] ? 0 : 1
            current[j] = min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + cost)
        }
        previous = current
    }
    return Double(previous[hyp.count]) / Double(ref.count)
}

// MARK: - Output

enum Report {
    /// Writes `results/<timestamp>.md` (for reading) and `.json` (for diffing runs later).
    static func write(_ results: [ClipResult], engines: [String]) throws -> URL {
        try FileManager.default.createDirectory(at: Paths.results, withIntermediateDirectories: true)
        let stamp = ISO8601DateFormatter().string(from: Date()).replacingOccurrences(of: ":", with: "-")
        let base = Paths.results.appending(path: stamp)

        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(results).write(to: base.appendingPathExtension("json"))

        let markdown = render(results, engines: engines)
        let mdURL = base.appendingPathExtension("md")
        try markdown.write(to: mdURL, atomically: true, encoding: .utf8)
        return mdURL
    }

    static func summaryTable(_ results: [ClipResult], engines: [String]) -> String {
        var lines = ["| Engine | Mean WER | Mean STT latency | Mean cleanup latency |",
                     "|---|---|---|---|"]
        for engine in engines {
            let rows = results.flatMap(\.engines).filter { $0.engine == engine && $0.error == nil }
            let wers = rows.compactMap(\.wer)
            let stt = rows.map(\.sttSeconds)
            let cleanup = rows.compactMap(\.cleanupSeconds)
            lines.append("| \(engine) | \(wers.isEmpty ? "–" : pct(mean(wers))) | \(stt.isEmpty ? "–" : ms(mean(stt))) | \(cleanup.isEmpty ? "–" : ms(mean(cleanup))) |")
        }
        return lines.joined(separator: "\n")
    }

    private static func render(_ results: [ClipResult], engines: [String]) -> String {
        var out = "# Murmur eval: \(Date().formatted(date: .abbreviated, time: .shortened))\n\n"
        out += "WER is measured on the **raw** STT output against each clip's verbatim `reference`.\n\n"
        out += summaryTable(results, engines: engines) + "\n"

        for clip in results {
            out += "\n---\n\n## \(clip.clip)\n\n"
            out += "`\(clip.context.rawValue)` · \(String(format: "%.1f", clip.audioSeconds))s audio\n\n"
            if let reference = clip.reference { out += "**Reference (verbatim):** \(reference)\n\n" }
            if let intended = clip.intended { out += "**Intended:**\n\n\(quote(intended))\n\n" }
            for engine in clip.engines {
                out += "### \(engine.engine)\n\n"
                if let error = engine.error {
                    out += "> ⚠️ \(error)\n\n"
                    continue
                }
                let wer = engine.wer.map { " · WER \(pct($0))" } ?? ""
                out += "Raw, \(ms(engine.sttSeconds))\(wer):\n\n\(quote(engine.raw))\n\n"
                if let cleaned = engine.cleaned {
                    out += "Cleaned, \(ms(engine.cleanupSeconds ?? 0)):\n\n\(quote(cleaned))\n\n"
                }
            }
        }
        return out
    }

    private static func quote(_ text: String) -> String {
        text.isEmpty ? "> _(empty)_" : text.split(separator: "\n", omittingEmptySubsequences: false)
            .map { "> \($0)" }.joined(separator: "\n")
    }
}

func mean(_ values: [Double]) -> Double { values.reduce(0, +) / Double(max(values.count, 1)) }
func pct(_ value: Double) -> String { String(format: "%.1f%%", value * 100) }
func ms(_ seconds: Double) -> String { String(format: "%.0f ms", seconds * 1000) }

struct EvalError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}
