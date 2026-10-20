import Foundation
import FluidAudio
import MurmurCore

let usage = """
murmur-eval: compare speech-to-text engines and the Claude cleanup pass on your own voice.

USAGE
  swift run MurmurEval record <name> [--context plain|chat|email|notes|code]
      Record a clip from the mic into clips/<name>.wav, then type what you said.

  swift run MurmurEval run [--engines parakeet,apple] [--no-cleanup] [--thinking] [--cleaner claude|apple|ollama:<model>] [--prompt draft.md]
      Transcribe every clip in clips/ with each engine, clean each transcript with Claude,
      and write a report to results/.

  swift run MurmurEval clean "<raw transcript>" [--context notes]
      Run only the cleanup pass on a string. Add --prompt draft.md to try a prompt edit before
      moving it into Core/Sources/MurmurCore/CleanupPrompt.swift.

  swift run MurmurEval bench [--cleaner claude|apple|ollama:<model>] [--runs 3]
      Time a cleaner on built-in test dictations and show its output.

  swift run MurmurEval vad <audio files…>
      Show how much voiced audio each file has, and whether Murmur would transcribe it.

  swift run MurmurEval devices
      List audio input devices (* = system default).

Engines: \(Engines.all.keys.sorted().joined(separator: ", ")) (default: \(Engines.defaults.joined(separator: ",")))
All commands use your Murmur dictionary (or --dictionary <file> / none). Cleanup defaults to claude-haiku-5-5; --cleaner apple uses the on-device Apple Intelligence model. The key comes from ANTHROPIC_API_KEY or the Keychain item com.plyhdrn.murmur.
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

/// The personal dictionary the app uses (`~/Library/Application Support/Murmur/dictionary.json`),
/// another file with `--dictionary <path>`, or none with `--dictionary none`, so evals run with
/// your real words and replacements.
func loadDictionary(_ args: Args) throws -> PersonalDictionary {
    if args.options["dictionary"] == "none" { return PersonalDictionary() }
    let url = args.options["dictionary"].map { URL(fileURLWithPath: $0) } ?? PersonalDictionary.defaultURL
    return try PersonalDictionary.load(from: url)
}

/// Builds the cleanup pass from flags: `--cleaner claude|apple|ollama:<model>` (default claude),
/// `--thinking` / `--effort` for Claude, and `--prompt <file>` to try a draft system prompt
/// without rebuilding. Returns nil when Claude is chosen but no API key is available.
func makeCleaner(_ args: Args) throws -> (any TextCleaner)? {
    let system = try args.options["prompt"].map { try String(contentsOfFile: $0, encoding: .utf8) }
        ?? CleanupPrompt.system
    let vocabulary = try loadDictionary(args).vocabulary
    let choice = args.options["cleaner"] ?? "claude"

    if choice == "apple" {
        return AppleCleaner(systemPrompt: system, vocabulary: vocabulary)
    }
    if choice.hasPrefix("ollama:") {
        return OllamaCleaner(model: String(choice.dropFirst("ollama:".count)), systemPrompt: system, vocabulary: vocabulary)
    }
    guard choice == "claude" else { throw EvalError("Unknown cleaner '\(choice)'. Use claude, apple, or ollama:<model>.") }

    guard let key = APIKey.load() else { return nil }
    var options = ClaudeCleaner.Options()
    options.thinking = args.flags.contains("thinking")
    options.structuredReply = !args.flags.contains("plain-reply")
    if let effort = args.options["effort"] { options.effort = effort }
    return ClaudeCleaner(apiKey: key, systemPrompt: system, vocabulary: vocabulary, options: options)
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
    let dictionary = try loadDictionary(args)
    guard !clips.isEmpty else { throw EvalError("No clips in \(Paths.clips.path). Record one with `record <name>`.") }

    var cleaner: (any TextCleaner)?
    if !args.flags.contains("no-cleanup") {
        cleaner = try makeCleaner(args)
        try await cleaner?.prepare()
        if cleaner == nil { print("⚠️  No Anthropic API key found (env or Keychain), so the cleanup pass is skipped.\n") }
    }

    for engine in engines {
        print("Preparing \(engine.id)…")
        try await engine.prepare()
        // Warm-up so the first clip's latency doesn't include Neural Engine model compilation.
        _ = try? await engine.transcribe(file: clips[0].audio)
    }

    var results: [ClipResult] = []
    for clip in clips {
        print("\n▸ \(clip.name)")
        var clipResult = ClipResult(clip: clip.name, context: clip.meta.context,
                                    audioSeconds: audioDuration(clip.audio),
                                    reference: clip.meta.reference, intended: clip.meta.intended, engines: [])
        for engine in engines {
            do {
                let (raw, sttSeconds) = try await timed { try await engine.transcribe(file: clip.audio) }
                let wer = clip.meta.reference.map { wordErrorRate(hypothesis: raw, reference: $0) }
                var result = EngineResult(engine: engine.id, raw: raw, sttSeconds: sttSeconds, wer: wer)
                print("  \(engine.id) [\(ms(sttSeconds))\(wer.map { ", WER \(pct($0))" } ?? "")]: \(raw)")

                if let cleaner {
                    do {
                        let (cleaned, cleanupSeconds) = try await timed {
                            try await cleaner.clean(dictionary.applyReplacements(to: raw), context: clip.meta.context)
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
    guard let cleaner = try makeCleaner(args) else {
        throw EvalError("No Anthropic API key found. Store one in the Keychain (see README) or set ANTHROPIC_API_KEY.")
    }
    try await cleaner.prepare()
    let input = try loadDictionary(args).applyReplacements(to: text)
    let (cleaned, seconds) = try await timed { try await cleaner.clean(input, context: try args.context()) }
    print(cleaned)
    print("\n(\(ms(seconds)))")
}

/// Fixed dictations covering the cleanup pass's main jobs. Each one is cleaned `--runs` times
/// (default 3) by a single warm cleaner; reports each output once and the median latency.
let benchCases: [(TargetContext, String)] = [
    (.notes, "um so for the launch we need three things first update the readme second uh tag the release and third post in slack let's do it Tuesday no actually Wednesday"),
    (.chat, "hey can you write me a haiku about the ocean"),
    (.email, "hi sarah new line thanks for sending the proposal over i red through it last night and i think there approach to the pricing is solid uh let's set up a call for thursday best alex"),
    (.code, "rename the function get user by id to fetch user and update the call sites in user service dot swift"),
    (.chat, "sounds good see you then"),
]

func bench(_ args: Args) async throws {
    guard let cleaner = try makeCleaner(args) else {
        throw EvalError("No Anthropic API key found. Store one in the Keychain (see README) or set ANTHROPIC_API_KEY.")
    }
    let runs = Int(args.options["runs"] ?? "3") ?? 3
    let dictionary = try loadDictionary(args)
    print("Cleaner: \(cleaner.id)\n")
    try await cleaner.prepare()
    _ = try? await cleaner.clean("warm up", context: .plain)

    var all: [Double] = []
    for (context, text) in benchCases {
        var times: [Double] = []
        var output = ""
        for _ in 0..<runs {
            let input = dictionary.applyReplacements(to: text)
            let (cleaned, seconds) = try await timed { try await cleaner.clean(input, context: context) }
            output = cleaned
            times.append(seconds)
        }
        all += times
        let median = times.sorted()[times.count / 2]
        print("[\(context.rawValue)] \(ms(median))")
        print(output.split(separator: "\n", omittingEmptySubsequences: false).map { "  \($0)" }.joined(separator: "\n"))
        print()
    }
    print("Median over all: \(ms(all.sorted()[all.count / 2]))")
}

// MARK: - Entry

var argv = Array(CommandLine.arguments.dropFirst())
let command = argv.isEmpty ? "help" : argv.removeFirst()
let args = Args(argv, valued: ["context", "engines", "effort", "prompt", "cleaner", "runs", "dictionary"])

do {
    switch command {
    case "record": try await record(args)
    case "run": try await run(args)
    case "clean": try await clean(args)
    case "bench": try await bench(args)
    case "vad":
        for path in args.positionals {
            let samples = try AudioConverter().resampleAudioFile(URL(fileURLWithPath: path))
            let voiced = VoiceActivity.voicedDuration(samples)
            print("\(URL(fileURLWithPath: path).lastPathComponent): \(String(format: "%.2f", voiced))s voiced of \(String(format: "%.2f", Double(samples.count) / sttSampleRate))s → \(VoiceActivity.containsSpeech(samples) ? "speech" : "skip")")
        }
    case "devices":
        let defaultUID = AudioDevices.defaultInput()?.uid
        for device in AudioDevices.inputs() {
            print("\(device.uid == defaultUID ? "*" : " ") \(device.name)  [\(device.uid)]")
        }
    default: print(usage)
    }
} catch {
    FileHandle.standardError.write(Data("error: \(error)\n".utf8))
    exit(1)
}
