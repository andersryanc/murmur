import Foundation

/// The kind of app the dictation is headed for. The cleanup pass formats differently for each.
public enum TargetContext: String, Codable, CaseIterable, Sendable {
    case plain, chat, email, notes, code

    /// Best guess from the frontmost app's bundle identifier.
    public init(bundleID: String?) {
        guard let id = bundleID?.lowercased() else { self = .plain; return }
        if let match = Self.knownApps.first(where: { id.hasPrefix($0.key) }) {
            self = match.value
        } else {
            self = .plain
        }
    }

    private static let knownApps: [String: TargetContext] = [
        // Chat
        "com.tinyspeck.slackmacgap": .chat,
        "com.apple.mobilesms": .chat,
        "com.hnc.discord": .chat,
        "net.whatsapp.whatsapp": .chat,
        "ru.keepcoder.telegram": .chat,
        "com.microsoft.teams": .chat,
        // Email
        "com.apple.mail": .email,
        "com.microsoft.outlook": .email,
        "com.superhuman.electron": .email,
        "com.readdle.smartemail": .email,
        // Notes and docs
        "com.apple.notes": .notes,
        "md.obsidian": .notes,
        "notion.id": .notes,
        "net.shinyfrog.bear": .notes,
        "com.lukilabs.lukiapp": .notes,  // Craft
        // Code: editors, terminals, AI coding tools
        "com.apple.dt.xcode": .code,
        "com.microsoft.vscode": .code,
        "com.todesktop.230313mzl4w4u92": .code,  // Cursor
        "dev.zed.zed": .code,
        "com.apple.terminal": .code,
        "com.googlecode.iterm2": .code,
        "com.mitchellh.ghostty": .code,
        "dev.warp.warp-stable": .code,
        "com.jetbrains": .code,
        "com.anthropic.claudefordesktop": .code,
    ]
}
