import Foundation

let usage = """
murmur-eval: compare speech-to-text engines and the Claude cleanup pass on your own voice.

USAGE
  swift run MurmurEval record <name> [--context plain|chat|email|notes|code]
      Record a clip from the mic into clips/<name>.wav, then type what you said.

  swift run MurmurEval run [--engines parakeet,apple] [--no-cleanup] [--thinking] [--effort low]
      Transcribe every clip in clips/ with each engine, clean each transcript with Claude,
      and write a report to results/.

  swift run MurmurEval clean "<raw transcript>" [--context notes]
      Run only the cleanup pass on a string, to iterate on prompts/cleanup.md quickly.

Engines: \(Engines.all.keys.sorted().joined(separator: ", ")) (default: \(Engines.defaults.joined(separator: ",")))
Cleanup uses claude-haiku-5-5. The key comes from ANTHROPIC_API_KEY or the Keychain item com.plyhdrn.murmur.
"""

/// Minimal flag parsing: `--key value`, `--flag`, and positionals.
struct Args {
    var positionals: [String] = []
    var options: [String: String] = [:]
    var flags: Set<String> = []

    init(_ raw: [String], valued: Set<String>) {
        var it = raw.makeIterator()
        while let arg = it.next() {
            if arg.hasPrefix("--") {
                let key = String(arg.dropFirst(2))
                if valued.contains(key), let value = it.next() { options[key] = value } else { flags.insert(key) }
            } else {
                positionals.append(arg)
            }
        }
    }

    func context() throws -> TargetContext {
        guard let raw = options["context"] else { return .plain }
        guard let context = TargetContext(rawValue: raw) else {
            throw EvalError("Unknown context '\(raw)'. Use one of: \(TargetContext.allCases.map(\.rawValue).joined(separator: ", "))")
        }
        return context
    }
}

func prompt(_ question: String) -> String? {
    print(question, terminator: " ")
    let answer = readLine()?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    return answer.isEmpty ? nil : answer
}

func timed<T>(_ work: () async throws -> T) async rethrows -> (T, Double) {
    let start = ContinuousClock.now
    let value = try await work()
    let elapsed = ContinuousClock.now - start
    return (value, Double(elapsed.components.seconds) + Double(elapsed.components.attoseconds) / 1e18)
}

// MARK: - Commands

func record(_ args: Args) async throws {
    guard let name = args.positionals.first else { throw EvalError("record needs a clip name") }
    let context = try args.context()
    try FileManager.default.createDirectory(at: Paths.clips, withIntermediateDirectories: true)
    let url = Paths.clips.appending(path: "\(name).wav")

    let seconds = try await Recorder.record(to: url)
    print("Saved \(url.lastPathComponent) (\(String(format: "%.1f", seconds))s)\n")

    var meta = ClipMeta(context: context)
    meta.reference = prompt("Type exactly what you said, word for word (Return to skip):")
    meta.intended = prompt("Type what you'd want it to come out as (Return to skip):")
    try Clip.saveMeta(meta, name: name)
    print("Saved \(name).json")
}

func run(_ args: Args) async throws {
    let engineIDs = args.options["engines"]?.split(separator: ",").map(String.init) ?? Engines.defaults
    let engines = try Engines.make(engineIDs)
    let clips = try Clip.loadAll()
    guard !clips.isEmpty else { throw EvalError("No clips in \(Paths.clips.path). Record one with `record <name>`.") }

    var cleaner: ClaudeCleaner?
    if !args.flags.contains("no-cleanup") {
        var options = ClaudeCleaner.Options()
        options.thinking = args.flags.contains("thinking")
        if let effort = args.options["effort"] { options.effort = effort }
        cleaner = try ClaudeCleaner.fromEnvironment(options: options)
        if cleaner == nil { print("⚠️  No Anthropic API key found (env or Keychain), so the cleanup pass is skipped.\n") }
    }

    for engine in engines {
        print("Preparing \(engine.id)…")
        try await engine.prepare()
        // Warm-up so the first clip's latency doesn't include Neural Engine model compilation.
        _ = try? await engine.transcribe(clips[0].audio)
    }

    var results: [ClipResult] = []
    for clip in clips {
        print("\n▸ \(clip.name)")
        var clipResult = ClipResult(clip: clip.name, context: clip.meta.context,
                                    audioSeconds: audioDuration(clip.audio),
                                    reference: clip.meta.reference, intended: clip.meta.intended, engines: [])
        for engine in engines {
            do {
                let (raw, sttSeconds) = try await timed { try await engine.transcribe(clip.audio) }
                let wer = clip.meta.reference.map { wordErrorRate(hypothesis: raw, reference: $0) }
                var result = EngineResult(engine: engine.id, raw: raw, sttSeconds: sttSeconds, wer: wer)
                print("  \(engine.id) [\(ms(sttSeconds))\(wer.map { ", WER \(pct($0))" } ?? "")]: \(raw)")

                if let cleaner {
                    do {
                        let (cleaned, cleanupSeconds) = try await timed {
                            try await cleaner.clean(raw, context: clip.meta.context)
                        }
                        result.cleaned = cleaned
                        result.cleanupSeconds = cleanupSeconds
                        print("    ↳ cleaned [\(ms(cleanupSeconds))]: \(cleaned.replacingOccurrences(of: "\n", with: " ⏎ "))")
                    } catch {
                        result.error = "Cleanup failed: \(error)"
                        print("    ↳ cleanup failed: \(error)")
                    }
                }
                clipResult.engines.append(result)
            } catch {
                clipResult.engines.append(EngineResult(engine: engine.id, raw: "", sttSeconds: 0, wer: nil,
                                                       error: "Transcription failed: \(error)"))
                print("  \(engine.id) failed: \(error)")
            }
        }
        results.append(clipResult)
    }

    let report = try Report.write(results, engines: engineIDs)
    print("\n" + Report.summaryTable(results, engines: engineIDs))
    print("\nReport: \(report.path)")
}

func clean(_ args: Args) async throws {
    guard let text = args.positionals.first else { throw EvalError("clean needs a transcript string") }
    var options = ClaudeCleaner.Options()
    options.thinking = args.flags.contains("thinking")
    if let effort = args.options["effort"] { options.effort = effort }
    guard let cleaner = try ClaudeCleaner.fromEnvironment(options: options) else {
        throw EvalError("No Anthropic API key found. Store one in the Keychain (see README) or set ANTHROPIC_API_KEY.")
    }
    let (cleaned, seconds) = try await timed { try await cleaner.clean(text, context: try args.context()) }
    print(cleaned)
    print("\n(\(ms(seconds)))")
}

// MARK: - Entry

var argv = Array(CommandLine.arguments.dropFirst())
let command = argv.isEmpty ? "help" : argv.removeFirst()
let args = Args(argv, valued: ["context", "engines", "effort"])

do {
    switch command {
    case "record": try await record(args)
    case "run": try await run(args)
    case "clean": try await clean(args)
    default: print(usage)
    }
} catch {
    FileHandle.standardError.write(Data("error: \(error)\n".utf8))
    exit(1)
}
