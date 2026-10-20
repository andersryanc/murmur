import AppKit
import SwiftUI

/// The small pill near the bottom of the screen that shows Murmur listening and working.
///
/// It's a non-activating panel that ignores the mouse, so it never takes focus away from the
/// app you're dictating into. If it did, the paste would land in the wrong place.
final class HUDController {
    private let panel: NSPanel
    private var hideWork: DispatchWorkItem?

    init(model: AppModel) {
        panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 360, height: 72),
            styleMask: [.nonactivatingPanel, .borderless],
            backing: .buffered,
            defer: true
        )
        panel.isFloatingPanel = true
        panel.level = .statusBar
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = false
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.contentView = NSHostingView(rootView: HUDView(model: model))
    }

    func show() {
        hideWork?.cancel()
        hideWork = nil
        position()
        panel.alphaValue = 1
        panel.orderFrontRegardless()
    }

    func hide(after delay: TimeInterval = 0) {
        hideWork?.cancel()
        let work = DispatchWorkItem { [panel] in
            NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.15
                panel.animator().alphaValue = 0
            } completionHandler: {
                panel.orderOut(nil)
            }
        }
        hideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }

    /// Bottom-center of the screen with the mouse on it, just above the Dock.
    private func position() {
        let mouse = NSEvent.mouseLocation
        let screen = NSScreen.screens.first { $0.frame.contains(mouse) } ?? NSScreen.main
        guard let visible = screen?.visibleFrame else { return }
        let size = panel.frame.size
        panel.setFrameOrigin(NSPoint(x: visible.midX - size.width / 2, y: visible.minY + 28))
    }
}

struct HUDView: View {
    let model: AppModel

    var body: some View {
        HStack(spacing: 10) {
            content
        }
        .padding(.horizontal, 16)
        .frame(height: 40)
        .background {
            Capsule()
                .fill(.black.opacity(0.82))
                .overlay(Capsule().strokeBorder(.white.opacity(0.12), lineWidth: 0.5))
                .shadow(color: .black.opacity(0.35), radius: 12, y: 4)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .animation(.spring(duration: 0.25), value: model.phase)
    }

    @ViewBuilder
    private var content: some View {
        if let notice = model.notice, model.phase != .recording {
            Image(systemName: "exclamationmark.circle.fill")
                .foregroundStyle(.orange)
            Text(notice)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(.white)
                .lineLimit(1)
        } else if model.phase == .recording {
            Circle()
                .fill(Murmur.accent)
                .frame(width: 8, height: 8)
            LevelBars(levels: model.levels)
        } else {
            ThinkingDots()
            Text("Murmuring…")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(.white.opacity(0.9))
        }
    }
}

/// Live microphone level, newest bar on the right.
private struct LevelBars: View {
    let levels: [Float]

    var body: some View {
        HStack(alignment: .center, spacing: 3) {
            ForEach(levels.indices, id: \.self) { index in
                Capsule()
                    .fill(Murmur.gradient)
                    .frame(width: 3, height: 4 + CGFloat(levels[index]) * 18)
            }
        }
        .frame(height: 22)
        .animation(.linear(duration: 0.08), value: levels)
    }
}

private struct ThinkingDots: View {
    var body: some View {
        TimelineView(.animation) { timeline in
            let t = timeline.date.timeIntervalSinceReferenceDate
            HStack(spacing: 4) {
                ForEach(0..<3) { index in
                    Circle()
                        .fill(Murmur.gradient)
                        .frame(width: 6, height: 6)
                        .opacity(0.35 + 0.65 * max(0, sin(t * 6 - Double(index) * 0.8)))
                }
            }
        }
    }
}

/// Placeholder brand palette until the branding pass.
enum Murmur {
    static let accent = Color(red: 0.62, green: 0.48, blue: 1.0)
    static let gradient = LinearGradient(
        colors: [Color(red: 0.45, green: 0.85, blue: 0.95), Color(red: 0.66, green: 0.47, blue: 1.0)],
        startPoint: .bottom, endPoint: .top
    )
}
