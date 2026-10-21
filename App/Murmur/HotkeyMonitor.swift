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

    fileprivate var flag: CGEventFlags {
        switch self {
        case .fn: .maskSecondaryFn
        case .rightOption: .maskAlternate
        case .rightCommand: .maskCommand
        }
    }
}

/// Watches for the push-to-talk key going down and up anywhere in the system.
///
/// Listens with a Quartz event tap at the HID level, the first stop for keyboard events. Other
/// dictation apps (Wispr Flow, for one) install *active* taps at the later session level that
/// swallow fn, and an ordinary NSEvent global monitor sits behind those, so it would never see
/// the key. This tap is listen-only: it never blocks or changes events for anyone else.
///
/// Creating the tap needs Accessibility permission, which Murmur needs anyway to paste. Call
/// `install()` again once permission is granted.
final class HotkeyMonitor {
    /// How long the key has to stay down before it counts as push-to-talk. A quicker tap is left
    /// to macOS, so tapping fn still opens the emoji picker without starting a recording.
    static let holdDelay: TimeInterval = 0.2

    var hotkey: Hotkey = .fn
    var onPress: () -> Void = {}
    var onRelease: () -> Void = {}
    /// Another key was typed while the hotkey was held, e.g. Option+Arrow to jump words.
    /// That's a shortcut, not dictation, so the recording should be thrown away.
    var onChord: () -> Void = {}

    private var tap: CFMachPort?
    private var runLoopSource: CFRunLoopSource?
    private var isDown = false
    /// The hold delay is running: the key is down but `onPress` hasn't fired yet.
    private var pendingPress: DispatchWorkItem?

    /// Returns false when the tap couldn't be created, which means no Accessibility permission yet.
    @discardableResult
    func install() -> Bool {
        uninstall()
        let mask = (1 << CGEventType.flagsChanged.rawValue) | (1 << CGEventType.keyDown.rawValue)
        guard let tap = CGEvent.tapCreate(
            tap: .cghidEventTap,
            place: .headInsertEventTap,
            options: .listenOnly,
            eventsOfInterest: CGEventMask(mask),
            callback: hotkeyTapCallback,
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else { return false }

        let source = CFMachPortCreateRunLoopSource(nil, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        self.tap = tap
        runLoopSource = source
        return true
    }

    func uninstall() {
        if let tap { CGEvent.tapEnable(tap: tap, enable: false) }
        if let runLoopSource { CFRunLoopRemoveSource(CFRunLoopGetMain(), runLoopSource, .commonModes) }
        tap = nil
        runLoopSource = nil
        isDown = false
        cancelPendingPress()
    }

    private func cancelPendingPress() {
        pendingPress?.cancel()
        pendingPress = nil
    }

    fileprivate func handle(type: CGEventType, keyCode: UInt16, flags: CGEventFlags) {
        switch type {
        case .tapDisabledByTimeout, .tapDisabledByUserInput:
            // macOS switches a tap off if it stalls; turn it straight back on.
            if let tap { CGEvent.tapEnable(tap: tap, enable: true) }
        case .keyDown:
            guard isDown else { return }
            if pendingPress != nil {
                // A shortcut typed before the hold delay ran out: never start recording at all.
                cancelPendingPress()
            } else {
                onChord()
            }
        case .flagsChanged:
            guard keyCode == hotkey.keyCode else { return }
            let pressed = flags.contains(hotkey.flag)
            if pressed, !isDown {
                isDown = true
                let press = DispatchWorkItem { [weak self] in
                    guard let self else { return }
                    pendingPress = nil
                    onPress()
                }
                pendingPress = press
                DispatchQueue.main.asyncAfter(deadline: .now() + Self.holdDelay, execute: press)
            } else if !pressed, isDown {
                isDown = false
                if pendingPress != nil {
                    // A quick tap: recording never started, so there's nothing to finish.
                    cancelPendingPress()
                } else {
                    onRelease()
                }
            }
        default:
            break
        }
    }
}

/// C callback for the event tap. The tap's run loop source is on the main run loop, so this
/// always runs on the main thread.
private nonisolated func hotkeyTapCallback(
    proxy: CGEventTapProxy, type: CGEventType, event: CGEvent, userInfo: UnsafeMutableRawPointer?
) -> Unmanaged<CGEvent>? {
    if let userInfo {
        let monitor = Unmanaged<HotkeyMonitor>.fromOpaque(userInfo).takeUnretainedValue()
        let keyCode = UInt16(event.getIntegerValueField(.keyboardEventKeycode))
        let flags = event.flags
        MainActor.assumeIsolated { monitor.handle(type: type, keyCode: keyCode, flags: flags) }
    }
    return Unmanaged.passUnretained(event)
}
