import Dispatch
import Foundation
import Glibc
import MurmurKit

let usage = """
murmur: push-to-talk dictation for Linux (Hyprland)

  murmur daemon                Load the speech model and wait for commands. Run it from
                               Hyprland's autostart (exec-once).
  murmur start | stop          Start or finish a dictation. Bind to a key's press and release.
  murmur toggle                Start, or finish if already recording. For a single key binding.
  murmur cancel                Throw away the current recording.
  murmur status                ready, recording or processing.
  murmur transcribe <file.wav> [--context plain|chat|email|notes|code] [--no-cleanup]
                               Transcribe and clean up a WAV file, without the daemon.
  murmur bench [--model <ollama model>] [--runs 3]
                               Time the cleanup on fixed test dictations and print the results.
  murmur doctor                Check that the tools, model and API key Murmur needs are present.

Settings: \(Config.url.path)
Dictionary: \(PersonalDictionary.defaultURL.path)
Log: \(Paths.log.path)
"""

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

func loadTranscriber(_ config: Config) -> Transcriber {
    do {
        return try Transcriber(modelDirectory: config.modelDirectory, threads: config.threads)
    } catch {
        fail("\(error)")
    }
}

@MainActor
func runDaemon() async -> Never {
    signal(SIGPIPE, SIG_IGN)
    if (try? ControlSocket.send("status")) != nil { fail("\(MurmurSocketError.alreadyRunning)") }
    let config = Config.load()
    Log.info("Loading speech model from \(config.modelDirectory.path)")
    let transcriber = loadTranscriber(config)
    // The first decode is slow while ONNX Runtime warms up; do it now, not on the first dictation.
    _ = transcriber.transcribe([Float](repeating: 0, count: Int(sttSampleRate)))

    let daemon = Daemon(transcriber: transcriber)
    let socket: any DispatchSourceRead
    do {
        socket = try ControlSocket.listen({ daemon.handle($0) }, watch: { daemon.watch($0) })
    } catch {
        fail("\(error)")
    }

    // Remove the socket on the way out.
    var signals: [any DispatchSourceSignal] = []
    for number in [SIGINT, SIGTERM] {
        signal(number, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: number, queue: .main)
        source.setEventHandler {
            socket.cancel()
            unlink(Paths.socket.path)
            exit(0)
        }
        source.resume()
        signals.append(source)
    }
    Log.info("Ready. Listening on \(Paths.socket.path)")

    while true {
        try? await Task.sleep(for: .seconds(3600))
        withExtendedLifetime((socket, signals)) {}
    }
}

func transcribe(_ arguments: [String]) async {
    guard let path = arguments.first(where: { !$0.hasPrefix("--") }) else { fail(usage) }
    let config = Config.load()
    let context = arguments.firstIndex(of: "--context").flatMap { arguments.indices.contains($0 + 1) ? arguments[$0 + 1] : nil }
        .flatMap(TargetContext.init(rawValue:)) ?? .plain

    let audio: (samples: [Float], sampleRate: Int)
    do {
        audio = try WAV.decode(Data(contentsOf: URL(fileURLWithPath: path)))
    } catch {
        fail("Couldn't read \(path): \(error)")
    }

    let clock = ContinuousClock()
    var start = clock.now
    let transcriber = loadTranscriber(config)
    print("Model loaded in \(clock.now - start)")
    _ = transcriber.transcribe([Float](repeating: 0, count: Int(sttSampleRate)))

    start = clock.now
    let raw = transcriber.transcribe(audio.samples, sampleRate: audio.sampleRate)
    print("Speech (\(clock.now - start)): \(raw)")
    guard !arguments.contains("--no-cleanup") else { return }

    let dictionary = (try? PersonalDictionary.load()) ?? PersonalDictionary()
    let corrected = dictionary.applyReplacements(to: raw)
    do {
        guard let cleaner = try config.makeCleaner(vocabulary: dictionary.vocabulary) else {
            print("Cleanup is off in \(Config.url.path)")
            return
        }
        start = clock.now
        let cleaned = try await cleaner.clean(corrected, context: context)
        print("Cleanup [\(cleaner.id), \(context.rawValue)] (\(clock.now - start)): \(cleaned)")
    } catch {
        fail("Cleanup failed: \(error)")
    }
}

/// Cleans each of MurmurKit's `benchCases` `--runs` times. Consecutive cases use different
/// styles, so with Ollama the first run of each pays for reading that style's prompt and the
/// rest show the cached speed; both are reported.
func bench(_ arguments: [String]) async {
    func option(_ name: String) -> String? {
        arguments.firstIndex(of: name).flatMap { arguments.indices.contains($0 + 1) ? arguments[$0 + 1] : nil }
    }
    let config = Config.load()
    let runs = max(Int(option("--runs") ?? "3") ?? 3, 1)
    let dictionary = (try? PersonalDictionary.load()) ?? PersonalDictionary()
    let cleaner: any TextCleaner
    do {
        if let model = option("--model") {
            cleaner = OllamaCleaner(model: model, vocabulary: dictionary.vocabulary)
        } else if let configured = try config.makeCleaner(vocabulary: dictionary.vocabulary) {
            cleaner = configured
        } else {
            fail("Cleanup is off in \(Config.url.path); pass --model to bench an Ollama model")
        }
    } catch {
        fail("\(error)")
    }

    let clock = ContinuousClock()
    func seconds(_ duration: Duration) -> Double { Double(duration / .milliseconds(1)) / 1000 }
    print("Cleaner: \(cleaner.id)")
    var start = clock.now
    do {
        try await cleaner.prepare()
        _ = try await cleaner.clean("warm up", context: .plain)
    } catch {
        fail("Cleanup failed: \(error)")
    }
    print(String(format: "Load and warm-up: %.1f s\n", seconds(clock.now - start)))

    var firsts: [Double] = [], repeats: [Double] = []
    for (context, text) in benchCases {
        var times: [Double] = []
        var output = ""
        for _ in 0..<runs {
            start = clock.now
            do {
                output = try await cleaner.clean(dictionary.applyReplacements(to: text), context: context)
            } catch {
                fail("Cleanup failed: \(error)")
            }
            times.append(seconds(clock.now - start))
        }
        firsts.append(times[0])
        repeats += times.dropFirst()
        let rest = times.dropFirst().sorted()
        let repeatText = rest.isEmpty ? "" : String(format: ", then %.2f s", rest[rest.count / 2])
        print(String(format: "[%@] %.2f s", context.rawValue, times[0]) + repeatText)
        print(output.split(separator: "\n", omittingEmptySubsequences: false).map { "  \($0)" }.joined(separator: "\n"))
        print()
    }
    func median(_ values: [Double]) -> Double { values.sorted()[values.count / 2] }
    var summary = String(format: "Median: %.2f s after a style change", median(firsts))
    if !repeats.isEmpty { summary += String(format: ", %.2f s repeated", median(repeats)) }
    print(summary)
}

func doctor() async {
    let config = Config.load()
    var problems = 0
    func check(_ ok: Bool, _ label: String, fix: String) {
        print("\(ok ? "✓" : "✗") \(label)\(ok ? "" : "\n    \(fix)")")
        if !ok { problems += 1 }
    }

    let tools: [(tool: String, purpose: String, package: String)] = [
        ("pw-record", "records the microphone", "pipewire"),
        ("pw-play", "plays the start/stop sounds", "pipewire"),
        ("wl-copy", "copies to the clipboard", "wl-clipboard"),
        ("wtype", "sends the paste keystroke", "wtype"),
        ("hyprctl", "finds the focused window", "hyprland"),
        ("notify-send", "shows errors", "libnotify"),
    ]
    for (tool, purpose, package) in tools {
        check(Shell.has(tool), "\(tool): \(purpose)", fix: "Install it: sudo pacman -S --needed \(package)")
    }

    let model = config.modelDirectory
    check(FileManager.default.fileExists(atPath: model.appending(path: "encoder.int8.onnx").path),
          "Speech model at \(model.path)", fix: "Run Linux/scripts/setup.sh")

    switch config.cleanup {
    case .claude:
        check(APIKey.load() != nil, "Anthropic API key (cleanup: claude)",
              fix: "secret-tool store --label='Murmur' service \(APIKey.keychainService) account \(APIKey.keychainAccount)")
    case .ollama:
        let models = await OllamaCleaner.installedModels()
        check(models.contains(config.ollamaModel), "Ollama model \(config.ollamaModel) (cleanup: ollama)",
              fix: "Start Ollama and run: ollama pull \(config.ollamaModel)")
    case .off:
        print("· Cleanup is off")
    }

    let running = (try? ControlSocket.send("status")) != nil
    check(running, "Daemon running", fix: "Run `murmur daemon`, or add `exec-once = murmur daemon` to Hyprland's autostart")
    print(problems == 0 ? "\nAll good." : "\n\(problems) problem(s).")
}

let arguments = Array(CommandLine.arguments.dropFirst())
switch arguments.first {
case "daemon":
    await runDaemon()
case "start", "stop", "toggle", "cancel", "status":
    do {
        print(try ControlSocket.send(arguments[0]))
    } catch {
        if arguments[0] == "start" || arguments[0] == "toggle" { Desktop.notify("\(error)") }
        fail("\(error)")
    }
case "transcribe":
    await transcribe(Array(arguments.dropFirst()))
case "bench":
    await bench(Array(arguments.dropFirst()))
case "doctor":
    await doctor()
default:
    print(usage)
}
