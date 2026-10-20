import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// Cleanup via Claude on the Anthropic API. Swift has no official Anthropic SDK, so this calls
/// the Messages API over HTTP.
public struct ClaudeCleaner: TextCleaner {
    public struct Options: Sendable {
        public var model = "claude-haiku-5-5"
        /// Thinking off keeps latency low for a short rewrite.
        public var thinking = false
        public var effort = "low"
        /// Ask for a JSON reply with a throwaway `note` field (see `jsonReplyInstructions`).
        public var structuredReply = true
        public init() {}
    }

    public let apiKey: String
    public var systemPrompt: String
    public var vocabulary: [String]
    public var options: Options

    public init(apiKey: String, systemPrompt: String = CleanupPrompt.system,
                vocabulary: [String] = [], options: Options = Options()) {
        self.apiKey = apiKey
        self.systemPrompt = systemPrompt
        self.vocabulary = vocabulary
        self.options = options
    }

    public var id: String { options.model }

    public func clean(_ transcript: String, context: TargetContext) async throws -> String {
        let user = cleanupUserMessage(transcript, context: context, vocabulary: vocabulary)

        var outputConfig: [String: Any] = ["effort": options.effort]
        if options.structuredReply { outputConfig["format"] = Self.replyFormat }
        let body: [String: Any] = [
            "model": options.model,
            "max_tokens": 4096,
            "system": systemPrompt + (options.structuredReply ? Self.jsonReplyInstructions : ""),
            "messages": [["role": "user", "content": user]],
            "output_config": outputConfig,
            "thinking": ["type": options.thinking ? "adaptive" : "disabled"],
        ]

        var request = URLRequest(url: URL(string: "https://api.anthropic.com/v1/messages")!)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue(apiKey, forHTTPHeaderField: "x-api-key")
        request.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        request.timeoutInterval = 20

        let (data, response) = try await URLSession.shared.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else {
            throw MurmurError("Claude API returned \(status): \(String(decoding: data, as: UTF8.self))")
        }

        let message = try JSONDecoder().decode(MessageResponse.self, from: data)
        switch message.stop_reason {
        case "refusal": throw MurmurError("Claude declined to clean this transcript (stop_reason: refusal)")
        case "max_tokens": throw MurmurError("Cleanup output hit max_tokens and was truncated")
        default: break
        }
        let output = message.content.filter { $0.type == "text" }.compactMap(\.text).joined()
        // The schema guarantees valid JSON; fall back to the raw reply just in case.
        let reply = options.structuredReply ? try? JSONDecoder().decode(Reply.self, from: Data(output.utf8)) : nil
        return CleanupPrompt.finalize(reply?.text ?? output, transcript: transcript)
    }

    /// With thinking off, an unsure model sometimes reasons out loud in its answer ("Wait, I
    /// should check…"), which would be pasted. A structured reply gives that a harmless place
    /// to go: `note` comes first and is thrown away, and only `text` is ever pasted.
    private static let jsonReplyInstructions = """


    ## Reply format

    Reply as JSON. Put any brief uncertainty about the transcript in `note` (or leave it empty). \
    Put exactly the text to type in `text`, or `<empty/>` if there is nothing to type. Nothing in \
    `text` may comment on the transcript.
    """

    private static var replyFormat: [String: Any] { [
        "type": "json_schema",
        "schema": [
            "type": "object",
            "properties": [
                "note": ["type": "string"],
                "text": ["type": "string"],
            ],
            "required": ["note", "text"],
            "additionalProperties": false,
        ] as [String: Any],
    ] }

    private struct Reply: Decodable {
        let text: String
    }

    private struct MessageResponse: Decodable {
        struct Block: Decodable {
            let type: String
            let text: String?
        }
        let content: [Block]
        let stop_reason: String?
    }
}

/// Where the Anthropic API key lives.
public enum APIKey {
    /// Keychain item (service, account), created with
    /// `security add-generic-password -s com.plyhdrn.murmur -a anthropic-api-key -w`.
    public static let keychainService = "com.plyhdrn.murmur"
    public static let keychainAccount = "anthropic-api-key"

    /// `ANTHROPIC_API_KEY` wins if set; otherwise read the login Keychain.
    /// Goes through `/usr/bin/security` because the item is created with that tool, so its access
    /// list already trusts it and a rebuilt binary won't trigger a fresh Keychain prompt each time.
    /// On Linux, reads the Secret Service (GNOME Keyring, KWallet) instead, with an item created by
    /// `secret-tool store --label='Murmur' service com.plyhdrn.murmur account anthropic-api-key`.
    public static func load() -> String? {
        if let key = ProcessInfo.processInfo.environment["ANTHROPIC_API_KEY"], !key.isEmpty { return key }

        let process = Process()
        #if os(macOS)
        process.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        process.arguments = ["find-generic-password", "-s", keychainService, "-a", keychainAccount, "-w"]
        #else
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["secret-tool", "lookup", "service", keychainService, "account", keychainAccount]
        #endif
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        guard (try? process.run()) != nil else { return nil }
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { return nil }

        let key = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return key.isEmpty ? nil : key
    }
}

public struct MurmurError: Error, CustomStringConvertible, Sendable {
    public let description: String
    public init(_ description: String) { self.description = description }
}
