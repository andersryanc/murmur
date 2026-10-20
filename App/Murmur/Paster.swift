import AppKit
import ApplicationServices

/// Puts text at the cursor of whatever app is in front, by pasting it.
///
/// Pasting works in nearly every text field (native, Electron, browsers, terminals), unlike
/// typing characters or writing through the Accessibility API. The text stays on the clipboard
/// afterwards, so if no text field had focus you can still paste it yourself.
enum Paster {
    enum Outcome {
        case pasted
        /// No Accessibility permission, so the text was left on the clipboard for a manual ⌘V.
        case copiedOnly
    }

    static func paste(_ text: String) -> Outcome {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)

        guard AXIsProcessTrusted() else { return .copiedOnly }
        sendCommandV()
        return .pasted
    }

    private static func sendCommandV() {
        let source = CGEventSource(stateID: .combinedSessionState)
        let vKey: CGKeyCode = 9  // "V" on ANSI layouts
        let down = CGEvent(keyboardEventSource: source, virtualKey: vKey, keyDown: true)
        let up = CGEvent(keyboardEventSource: source, virtualKey: vKey, keyDown: false)
        down?.flags = .maskCommand
        up?.flags = .maskCommand
        down?.post(tap: .cghidEventTap)
        up?.post(tap: .cghidEventTap)
    }
}
