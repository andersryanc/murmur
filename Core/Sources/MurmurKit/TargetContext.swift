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

    /// Best guess from the focused window's class on Linux (`hyprctl activewindow -j`), e.g.
    /// "Slack", "Alacritty", "obsidian". Matches by substring, which also catches Chromium web
    /// apps, whose class carries the site's address (e.g. "chrome-app.slack.com__client-Default").
    public init(windowClass: String?) {
        guard let name = windowClass?.lowercased(), !name.isEmpty else { self = .plain; return }
        self = Self.knownWindowClasses.first(where: { name.contains($0.fragment) })?.context ?? .plain
    }

    /// Checked in order, so a more specific fragment must come before a shorter one inside it.
    private static let knownWindowClasses: [(fragment: String, context: TargetContext)] = [
        // Chat
        ("slack", .chat), ("discord", .chat), ("vesktop", .chat), ("signal", .chat),
        ("telegram", .chat), ("whatsapp", .chat), ("element", .chat), ("teams", .chat),
        // Email
        ("thunderbird", .email), ("mail.google.com", .email), ("hey.com", .email), ("geary", .email),
        ("evolution", .email), ("outlook", .email), ("superhuman", .email), ("betterbird", .email),
        // Notes and docs
        ("obsidian", .notes), ("notion", .notes), ("logseq", .notes), ("typora", .notes),
        ("joplin", .notes), ("docs.google.com", .notes),
        // Code: editors, terminals, AI coding tools
        ("code", .code), ("cursor", .code), ("zed", .code), ("jetbrains", .code), ("neovide", .code),
        ("alacritty", .code), ("kitty", .code), ("ghostty", .code), ("foot", .code),
        ("wezterm", .code), ("konsole", .code), ("gnome-terminal", .code), ("claude", .code),
    ]

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
