import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// The pill near the bottom of the screen that shows Murmur listening and working, like the Mac
// app's HUD. It follows the daemon over its control socket: after sending "watch", the daemon
// writes one line per event (`state recording`, `queue 2`, `level 0.42`, `notice …`).
//
// Each dictation waiting to paste gets its own card, oldest on the left: the first is being
// processed, the rest are queued. A new recording's card sits to their right.
//
// The surface never takes keyboard focus and ignores the mouse. If it took focus, the paste
// would land in the wrong place.
Item {
  id: root

  readonly property int levelCount: 14
  // Matches the daemon's Paths.socket. UID usually isn't exported, so the fallback asks `id -u`.
  property string runtimeDir: Quickshell.env("XDG_RUNTIME_DIR") || ""
  readonly property string socketPath: runtimeDir === "" ? "" : runtimeDir + "/murmur/murmur.sock"

  Process {
    running: root.runtimeDir === ""
    command: ["id", "-u"]
    stdout: StdioCollector {
      onStreamFinished: root.runtimeDir = "/tmp/murmur-" + text.trim()
    }
  }

  property string phase: "ready"
  property string notice: ""
  // One entry per dictation waiting to paste. When the oldest pastes, the first entry goes and
  // the others slide left.
  ListModel { id: queue }
  property var levels: []
  property bool shown: false
  // Picked when the pill appears, so it doesn't jump monitors mid-dictation.
  property var targetScreen: null

  function resetLevels() {
    var zeros = []
    for (var i = 0; i < root.levelCount; i++) zeros.push(0)
    root.levels = zeros
  }

  function show() {
    hideTimer.stop()
    if (!root.shown) {
      var name = Hyprland.focusedMonitor ? String(Hyprland.focusedMonitor.name || "") : ""
      root.targetScreen = Quickshell.screens.find(s => s.name === name) || null
    }
    root.shown = true
  }

  // A notice stays up for a moment after the dictation ends; otherwise the pill goes at once.
  function settle() {
    if (root.notice !== "") hideTimer.restart()
    else root.shown = false
  }

  function setQueue(count) {
    while (queue.count > count) queue.remove(0)
    while (queue.count < count) queue.append({ waiting: true })
  }

  function handle(line) {
    var space = line.indexOf(" ")
    var kind = space < 0 ? line : line.slice(0, space)
    var rest = space < 0 ? "" : line.slice(space + 1)
    if (kind === "queue") {
      root.setQueue(Math.max(0, Number(rest) || 0))
    } else if (kind === "state") {
      var wasRecording = root.phase === "recording"
      root.phase = rest
      if (rest === "recording") {
        if (!wasRecording) {
          root.notice = ""
          root.resetLevels()
        }
        root.show()
      } else if (rest === "processing") {
        // An older daemon doesn't send `queue`.
        if (queue.count === 0) queue.append({ waiting: true })
        root.show()
      } else {
        root.setQueue(0)
        root.settle()
      }
    } else if (kind === "level" && root.phase === "recording") {
      var level = Math.max(0, Math.min(1, Number(rest) || 0))
      root.levels = root.levels.slice(1).concat([level])
    } else if (kind === "notice") {
      root.notice = rest
      root.show()
      if (root.phase !== "recording" && root.phase !== "processing") hideTimer.restart()
    }
  }

  Component.onCompleted: resetLevels()

  Timer {
    id: hideTimer
    interval: 2500
    onTriggered: root.shown = false
  }

  Socket {
    id: socket
    path: root.socketPath
    connected: root.socketPath !== ""

    onConnectionStateChanged: {
      if (connected) {
        write("watch\n")
        flush()
      } else {
        root.phase = "ready"
        root.notice = ""
        root.setQueue(0)
        root.shown = false
      }
    }

    parser: SplitParser {
      onRead: line => root.handle(line)
    }
  }

  // The daemon may start after the shell, or restart; keep trying until it's there.
  Timer {
    interval: 2000
    repeat: true
    running: !socket.connected && root.socketPath !== ""
    onTriggered: socket.connected = true
  }

  // A rounded card in the theme's popup style, sized to what's inside it.
  component Card: BorderSurface {
    id: card

    readonly property int pad: Style.space(16)
    default property alias body: inner.data

    width: card.borderLeft + card.pad + inner.width + card.pad + card.borderRight
    height: card.borderTop + Style.space(40) + card.borderBottom
    color: Util.alpha(Commons.Color.background, 0.97)
    borderSpec: Border.surfaceSpec("popups", "border", Commons.Color.popups.border, Math.max(1, Style.space(2)))
    radius: Style.cornerRadius

    Behavior on width {
      enabled: root.shown
      NumberAnimation { duration: 200; easing.type: Easing.OutCubic }
    }

    Item {
      id: inner
      anchors.centerIn: parent
      width: childrenRect.width
      height: Style.space(22)
    }
  }

  component Label: Text {
    textFormat: Text.PlainText
    font.family: Style.font.family
    font.pixelSize: Style.font.subtitle
    font.weight: Font.Medium
    color: Commons.Color.popups.text
    anchors.verticalCenter: parent.verticalCenter
  }

  PanelWindow {
    id: panel
    screen: root.targetScreen
    visible: root.shown || cards.opacity > 0
    anchors { top: true; bottom: true; left: true; right: true }
    color: "transparent"
    WlrLayershell.namespace: "murmur"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    exclusionMode: ExclusionMode.Ignore
    mask: Region {}

    Row {
      id: cards
      spacing: Style.space(10)
      anchors.horizontalCenter: parent.horizontalCenter
      anchors.bottom: parent.bottom
      anchors.bottomMargin: Style.space(28)
      opacity: root.shown ? 1 : 0

      Behavior on opacity { NumberAnimation { duration: 150 } }
      add: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 150 }
      }
      move: Transition {
        NumberAnimation { properties: "x"; duration: 250; easing.type: Easing.OutCubic }
      }

      // Waiting to paste: pulsing dots on the one being processed, "Queued" on the rest.
      Repeater {
        model: queue

        Card {
          id: waiting
          required property int index
          readonly property bool active: index === 0

          Row {
            spacing: Style.space(10)
            anchors.verticalCenter: parent.verticalCenter

            Row {
              id: dots
              property real t: 0
              spacing: Style.space(4)
              anchors.verticalCenter: parent.verticalCenter
              NumberAnimation on t {
                running: waiting.active && root.shown
                from: 0
                to: 2 * Math.PI / 6
                duration: 1047
                loops: Animation.Infinite
              }
              Repeater {
                model: 3
                Rectangle {
                  required property int index
                  width: Style.space(6)
                  height: width
                  radius: width / 2
                  color: Commons.Color.popups.text
                  opacity: waiting.active ? 0.35 + 0.65 * Math.max(0, Math.sin(dots.t * 6 - index * 0.8)) : 0.35
                }
              }
            }

            Label {
              text: waiting.active ? "Murmuring…" : "Queued"
              color: Util.alpha(Commons.Color.popups.text, waiting.active ? 0.9 : 0.6)
            }
          }
        }
      }

      // Recording: a dot and the live mic level, newest bar on the right.
      Card {
        visible: root.phase === "recording"

        Row {
          spacing: Style.space(10)
          anchors.verticalCenter: parent.verticalCenter

          Rectangle {
            width: Style.space(8)
            height: width
            radius: width / 2
            color: Commons.Color.urgent
            anchors.verticalCenter: parent.verticalCenter
          }

          Row {
            spacing: Style.space(3)
            anchors.verticalCenter: parent.verticalCenter
            Repeater {
              model: root.levelCount
              Rectangle {
                required property int index
                width: Style.space(3)
                height: Style.space(4) + (root.levels[index] || 0) * Style.space(18)
                radius: width / 2
                color: Commons.Color.popups.text
                anchors.verticalCenter: parent.verticalCenter
                Behavior on height { NumberAnimation { duration: 80 } }
              }
            }
          }
        }
      }

      // Notice: something went wrong (the text was still pasted or copied).
      Card {
        visible: root.notice !== ""

        Row {
          spacing: Style.space(10)
          anchors.verticalCenter: parent.verticalCenter

          Label {
            text: ""
            font.pixelSize: Style.font.heading
            font.weight: Font.Normal
            color: Commons.Color.accent
          }

          Label {
            text: root.notice
            elide: Text.ElideRight
            width: Math.min(implicitWidth, Style.space(320))
          }
        }
      }
    }
  }
}
