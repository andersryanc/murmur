import Foundation

/// Hyprland and Wayland: which window has focus, pasting into it, and notifications.
enum Desktop {
    /// Class of the focused window ("Slack", "Alacritty", …), or nil if there is none or this
    /// isn't Hyprland.
    static func activeWindowClass() async -> String? {
        let result = await Shell.run("hyprctl", ["activewindow", "-j"], captureOutput: true)
        guard result.succeeded,
              let json = try? JSONSerialization.jsonObject(with: Data(result.output.utf8)) as? [String: Any],
              let windowClass = json["class"] as? String, !windowClass.isEmpty
        else { return nil }
        return windowClass
    }

    /// Terminals paste with Ctrl+Shift+V; Ctrl+V would send a literal ^V.
    static func isTerminal(_ windowClass: String?) -> Bool {
        guard let name = windowClass?.lowercased() else { return false }
        return terminals.contains { name.contains($0) }
    }

    private static let terminals = [
        "alacritty", "kitty", "ghostty", "foot", "wezterm", "konsole", "gnome-terminal",
        "terminator", "tilix", "xterm", "rio", "warp",
    ]

    /// Puts `text` on the clipboard and pastes it into the focused window with wtype. The text
    /// stays on the clipboard afterwards, like on the Mac. Returns false if the paste keystroke
    /// couldn't be sent (the text is still on the clipboard if copying worked).
    static func paste(_ text: String, windowClass: String?) async -> Bool {
        guard await Shell.run("wl-copy", input: text).succeeded else { return false }
        let modifiers = isTerminal(windowClass) ? ["ctrl", "shift"] : ["ctrl"]
        let keys = modifiers.flatMap { ["-M", $0] } + ["-k", "v"] + modifiers.reversed().flatMap { ["-m", $0] }
        return await Shell.run("wtype", keys).succeeded
    }

    static func notify(_ message: String) {
        Shell.spawn("notify-send", ["--app-name=Murmur", "--expire-time=3000", "Murmur", message])
    }
}
