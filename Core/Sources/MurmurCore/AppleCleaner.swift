import Foundation
import FoundationModels

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
        return CleanupPrompt.finalize(response.content, transcript: transcript)
    }

    private func makeSession() -> LanguageModelSession {
        let session = LanguageModelSession(instructions: systemPrompt)
        session.prewarm()
        return session
    }
}
