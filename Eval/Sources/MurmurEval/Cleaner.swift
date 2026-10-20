import Foundation

/// The LLM post-processing pass: raw transcript in, text-you-meant-to-type out.
/// Swift has no official Anthropic SDK, so this calls the Messages API over HTTP.
struct ClaudeCleaner {
    struct Options {
        var model = "claude-haiku-5-5"
        /// Thinking off keeps latency low for a short rewrite; `--thinking` turns it on to compare.
        var thinking = false
        var effort = "low"
    }

    let apiKey: String
    let systemPrompt: String
    let vocabulary: [String]
    var options = Options()

    /// Keychain item the key is stored under (service, account).
    static let keychainService = "com.plyhdrn.murmur"
    static let keychainAccount = "anthropic-api-key"

    static func fromEnvironment(options: Options) throws -> ClaudeCleaner? {
        guard let key = apiKey() else { return nil }
        let prompt = try String(contentsOf: Paths.cleanupPrompt, encoding: .utf8)
        return ClaudeCleaner(apiKey: key, systemPrompt: prompt, vocabulary: loadVocabulary(), options: options)
    }

    /// `ANTHROPIC_API_KEY` wins if set; otherwise read the login Keychain.
    /// Goes through `/usr/bin/security` because the item is created with that tool, so its access
    /// list already trusts it and a rebuilt binary won't trigger a fresh Keychain prompt each time.
    static func apiKey() -> String? {
        if let key = ProcessInfo.processInfo.environment["ANTHROPIC_API_KEY"], !key.isEmpty { return key }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        process.arguments = ["find-generic-password", "-s", keychainService, "-a", keychainAccount, "-w"]
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

    func clean(_ transcript: String, context: TargetContext) async throws -> String {
        var user = "<context>\(context.rawValue)</context>\n"
        if !vocabulary.isEmpty {
            user += "<vocabulary>\n\(vocabulary.joined(separator: "\n"))\n</vocabulary>\n"
        }
        user += "<transcript>\n\(transcript)\n</transcript>"

        var body: [String: Any] = [
            "model": options.model,
            "max_tokens": 4096,
            "system": systemPrompt,
            "messages": [["role": "user", "content": user]],
            "output_config": ["effort": options.effort],
        ]
        body["thinking"] = options.thinking ? ["type": "adaptive"] : ["type": "disabled"]

        var request = URLRequest(url: URL(string: "https://api.anthropic.com/v1/messages")!)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "content-type")
        request.setValue(apiKey, forHTTPHeaderField: "x-api-key")
        request.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        request.timeoutInterval = 60

        let (data, response) = try await URLSession.shared.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else {
            throw EvalError("Claude API returned \(status): \(String(decoding: data, as: UTF8.self))")
        }

        let message = try JSONDecoder().decode(MessageResponse.self, from: data)
        switch message.stop_reason {
        case "refusal":
            throw EvalError("Claude declined to clean this transcript (stop_reason: refusal)")
        case "max_tokens":
            throw EvalError("Cleanup output hit max_tokens and was truncated")
        default:
            break
        }
        return message.content
            .filter { $0.type == "text" }
            .compactMap(\.text)
            .joined()
            .trimmingCharacters(in: .whitespacesAndNewlines)
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
