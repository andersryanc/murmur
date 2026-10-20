import Foundation
import FoundationModels

/// The cleanup pass: raw transcript in, text-you-meant-to-type out. Every backend gets the same
/// system prompt (`CleanupPrompt.system`) and the same user message, so they compare fairly.
public protocol TextCleaner: Sendable {
    var id: String { get }
    /// Load the model ahead of the first dictation. Optional; the default does nothing.
    func prepare() async throws
    func clean(_ transcript: String, context: TargetContext) async throws -> String
}

extension TextCleaner {
    public func prepare() async throws {}
}

/// The user turn every cleaner sends: target context, optional vocabulary, then the transcript
/// in tags so the model treats it as text to rewrite rather than a request to answer.
public func cleanupUserMessage(_ transcript: String, context: TargetContext, vocabulary: [String]) -> String {
    var user = "<context>\(context.rawValue)</context>\n"
    if !vocabulary.isEmpty {
        user += "<vocabulary>\n\(vocabulary.joined(separator: "\n"))\n</vocabulary>\n"
    }
    user += "<transcript>\n\(transcript)\n</transcript>"
    return user
}

// MARK: - Apple Intelligence on-device model (Foundation Models)

/// Apple's ~3B on-device model, built into macOS 26 with Apple Intelligence turned on.
/// Nothing to download or run, and nothing leaves the Mac.
public actor AppleCleaner: TextCleaner {
    public nonisolated let id = "apple-intelligence"
    private let systemPrompt: String
    private let vocabulary: [String]
    /// A session loaded and ready for the next dictation. Each dictation gets a fresh one so
    /// earlier transcripts don't pile up in the model's context.
    private var nextSession: LanguageModelSession?

    public init(systemPrompt: String = CleanupPrompt.system, vocabulary: [String] = []) {
        self.systemPrompt = systemPrompt
        self.vocabulary = vocabulary
    }

    public static var isAvailable: Bool {
        if case .available = SystemLanguageModel.default.availability { return true }
        return false
    }

    public static var unavailableReason: String? {
        switch SystemLanguageModel.default.availability {
        case .available: nil
        case .unavailable(let reason): "Apple Intelligence unavailable: \(reason)"
        }
    }

    public func prepare() async throws {
        guard Self.isAvailable else { throw MurmurError(Self.unavailableReason ?? "Apple Intelligence unavailable") }
        if nextSession == nil { nextSession = makeSession() }
    }

    public func clean(_ transcript: String, context: TargetContext) async throws -> String {
        let session = nextSession ?? makeSession()
        nextSession = makeSession()
        let response = try await session.respond(
            to: cleanupUserMessage(transcript, context: context, vocabulary: vocabulary),
            options: GenerationOptions(sampling: .greedy)
        )
        return response.content.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func makeSession() -> LanguageModelSession {
        let session = LanguageModelSession(instructions: systemPrompt)
        session.prewarm()
        return session
    }
}

// MARK: - Ollama (any local model)

/// A model served by Ollama on this Mac (`ollama serve`, default port 11434).
public struct OllamaCleaner: TextCleaner {
    public let model: String
    public var systemPrompt: String
    public var vocabulary: [String]
    public var baseURL: URL

    public var id: String { "ollama:\(model)" }

    public init(model: String, systemPrompt: String = CleanupPrompt.system, vocabulary: [String] = [],
                baseURL: URL = URL(string: "http://127.0.0.1:11434")!) {
        self.model = model
        self.systemPrompt = systemPrompt
        self.vocabulary = vocabulary
        self.baseURL = baseURL
    }

    /// Loads the model into memory and keeps it there, so the first dictation isn't slow.
    public func prepare() async throws {
        _ = try await chat(["model": model, "messages": [], "keep_alive": "30m"])
    }

    public func clean(_ transcript: String, context: TargetContext) async throws -> String {
        var body: [String: Any] = [
            "model": model,
            "messages": [
                ["role": "system", "content": systemPrompt],
                ["role": "user", "content": cleanupUserMessage(transcript, context: context, vocabulary: vocabulary)],
            ],
            "stream": false,
            "keep_alive": "30m",
            "options": ["temperature": 0],
            // Reasoning models (Qwen 3, DeepSeek R1…) would otherwise think before every rewrite.
            "think": false,
        ]
        let response: [String: Any]
        do {
            response = try await chat(body)
        } catch let error as MurmurError where error.description.contains("does not support thinking") {
            body["think"] = nil
            response = try await chat(body)
        }
        let content = (response["message"] as? [String: Any])?["content"] as? String ?? ""
        return Self.stripThinking(content).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func chat(_ body: [String: Any]) async throws -> [String: Any] {
        var request = URLRequest(url: baseURL.appending(path: "api/chat"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        request.timeoutInterval = 60

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await URLSession.shared.data(for: request)
        } catch {
            throw MurmurError("Can't reach Ollama at \(baseURL.absoluteString). Is it running?")
        }
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else {
            throw MurmurError("Ollama returned \(status): \(String(decoding: data, as: UTF8.self))")
        }
        return (try JSONSerialization.jsonObject(with: data) as? [String: Any]) ?? [:]
    }

    /// Drops a leading `<think>…</think>` block if a model emits one anyway.
    static func stripThinking(_ text: String) -> String {
        guard let end = text.range(of: "</think>") else { return text }
        return String(text[end.upperBound...])
    }

    /// Models already pulled into Ollama.
    public static func installedModels(baseURL: URL = URL(string: "http://127.0.0.1:11434")!) async -> [String] {
        guard let (data, _) = try? await URLSession.shared.data(from: baseURL.appending(path: "api/tags")),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let models = json["models"] as? [[String: Any]] else { return [] }
        return models.compactMap { $0["name"] as? String }.sorted()
    }
}
