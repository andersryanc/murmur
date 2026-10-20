import AppKit
import ApplicationServices

/// Puts text at the cursor of whatever app is in front, by pasting it.
///
/// Pasting works in nearly every text field (native, Electron, browsers, terminals), unlike
/// typing characters or writing through the Accessibility API. Your clipboard is saved first and
/// put back right after, so dictating doesn't clobber what you'd copied.
enum Paster {
    enum Outcome {
        case pasted
        /// No Accessibility permission, so the text was left on the clipboard for a manual ⌘V.
        case copiedOnly
    }

    static func paste(_ text: String) -> Outcome {
        let pasteboard = NSPasteboard.general
        let saved = snapshot(pasteboard)

        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
        // Tells clipboard managers (Raycast, Maccy, Paste…) not to record this temporary entry.
        pasteboard.setData(Data(), forType: NSPasteboard.PasteboardType("org.nspasteboard.TransientType"))

        guard AXIsProcessTrusted() else { return .copiedOnly }

        sendCommandV()

        // Give the target app time to read the pasteboard, then restore it, unless something
        // else has written to the clipboard in the meantime.
        let ourChange = pasteboard.changeCount
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) {
            guard pasteboard.changeCount == ourChange else { return }
            pasteboard.clearContents()
            if !saved.isEmpty { pasteboard.writeObjects(saved) }
        }
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

    /// Deep copy of every item and type on the pasteboard (text, images, files, rich text…).
    private static func snapshot(_ pasteboard: NSPasteboard) -> [NSPasteboardItem] {
        (pasteboard.pasteboardItems ?? []).map { item in
            let copy = NSPasteboardItem()
            for type in item.types {
                if let data = item.data(forType: type) { copy.setData(data, forType: type) }
            }
            return copy
        }
    }
}
