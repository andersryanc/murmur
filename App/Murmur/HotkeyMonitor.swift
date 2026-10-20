import AppKit

/// Which key you hold to talk. All three are modifier keys, so holding them never types anything.
enum Hotkey: String, CaseIterable, Identifiable {
    case fn, rightOption, rightCommand

    var id: String { rawValue }

    var label: String {
        switch self {
        case .fn: "fn (Globe)"
        case .rightOption: "Right Option ⌥"
        case .rightCommand: "Right Command ⌘"
        }
    }

    var shortLabel: String {
        switch self {
        case .fn: "fn"
        case .rightOption: "right ⌥"
        case .rightCommand: "right ⌘"
        }
    }

    fileprivate var keyCode: UInt16 {
        switch self {
        case .fn: 63
        case .rightOption: 61
        case .rightCommand: 54
        }
    }

    fileprivate var flag: NSEvent.ModifierFlags {
        switch self {
        case .fn: .function
        case .rightOption: .option
        case .rightCommand: .command
        }
    }
}

/// Watches for the push-to-talk key going down and up anywhere in the system.
///
/// Global key monitors only receive events once the app has Accessibility permission, which
/// Murmur needs anyway to paste. Call `install()` again after permission is granted.
final class HotkeyMonitor {
    var hotkey: Hotkey = .fn
    var onPress: () -> Void = {}
    var onRelease: () -> Void = {}
    /// Another key was typed while the hotkey was held, e.g. Option+Arrow to jump words.
    /// That's a shortcut, not dictation, so the recording should be thrown away.
    var onChord: () -> Void = {}

    private var monitors: [Any] = []
    private var isDown = false

    func install() {
        uninstall()
        let mask: NSEvent.EventTypeMask = [.flagsChanged, .keyDown]
        if let global = NSEvent.addGlobalMonitorForEvents(matching: mask, handler: { [weak self] event in
            self?.handle(event)
        }) {
            monitors.append(global)
        }
        if let local = NSEvent.addLocalMonitorForEvents(matching: mask, handler: { [weak self] event in
            self?.handle(event)
            return event
        }) {
            monitors.append(local)
        }
    }

    func uninstall() {
        monitors.forEach(NSEvent.removeMonitor)
        monitors.removeAll()
        isDown = false
    }

    private func handle(_ event: NSEvent) {
        if event.type == .keyDown {
            if isDown { onChord() }
            return
        }
        guard event.keyCode == hotkey.keyCode else { return }
        let pressed = event.modifierFlags.contains(hotkey.flag)
        if pressed, !isDown {
            isDown = true
            onPress()
        } else if !pressed, isDown {
            isDown = false
            onRelease()
        }
    }
}
