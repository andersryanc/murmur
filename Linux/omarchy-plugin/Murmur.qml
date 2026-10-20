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
// writes one line per event (`state recording`, `level 0.42`, `notice …`).
//
// The surface never takes keyboard focus and ignores the mouse. If it took focus, the paste
// would land in the wrong place.
Item {
  id: root

  readonly property int levelCount: 14
  readonly property string socketPath: (Quickshell.env("XDG_RUNTIME_DIR") || ("/tmp/murmur-" + Quickshell.env("UID"))) + "/murmur/murmur.sock"

  property string phase: "ready"
  property string notice: ""
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

  function handle(line) {
    var space = line.indexOf(" ")
    var kind = space < 0 ? line : line.slice(0, space)
    var rest = space < 0 ? "" : line.slice(space + 1)
    if (kind === "state") {
      root.phase = rest
      if (rest === "recording") {
        root.notice = ""
        root.resetLevels()
        root.show()
      } else if (rest === "processing") {
        root.show()
      } else {
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
    connected: true

    onConnectionStateChanged: {
      if (connected) {
        write("watch\n")
        flush()
      } else {
        root.phase = "ready"
        root.notice = ""
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
    running: !socket.connected
    onTriggered: socket.connected = true
  }

  PanelWindow {
    id: panel
    screen: root.targetScreen
    visible: root.shown || card.opacity > 0
    anchors { top: true; bottom: true; left: true; right: true }
    color: "transparent"
    WlrLayershell.namespace: "murmur"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    exclusionMode: ExclusionMode.Ignore
    mask: Region {}

    BorderSurface {
      id: card

      readonly property int pad: Style.space(16)

      width: card.borderLeft + card.pad + content.implicitWidth + card.pad + card.borderRight
      height: card.borderTop + Style.space(40) + card.borderBottom
      anchors.horizontalCenter: parent.horizontalCenter
      anchors.bottom: parent.bottom
      anchors.bottomMargin: Style.space(28)
      color: Util.alpha(Commons.Color.background, 0.97)
      borderSpec: Border.surfaceSpec("popups", "border", Commons.Color.popups.border, Math.max(1, Style.space(2)))
      radius: Style.cornerRadius
      opacity: root.shown ? 1 : 0

      Behavior on opacity { NumberAnimation { duration: 150 } }
      Behavior on width {
        enabled: root.shown
        NumberAnimation { duration: 200; easing.type: Easing.OutCubic }
      }

      Item {
        id: content
        anchors.centerIn: parent
        implicitWidth: noticeRow.visible ? noticeRow.implicitWidth
          : (recordingRow.visible ? recordingRow.implicitWidth : thinkingRow.implicitWidth)
        implicitHeight: Style.space(22)

        // Recording: a dot and the live mic level, newest bar on the right.
        Row {
          id: recordingRow
          visible: root.phase === "recording" && root.notice === ""
          anchors.centerIn: parent
          spacing: Style.space(10)

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

        // Processing: pulsing dots while Parakeet and the cleanup model work.
        Row {
          id: thinkingRow
          visible: root.phase !== "recording" && root.notice === ""
          anchors.centerIn: parent
          spacing: Style.space(10)

          Row {
            id: dots
            property real t: 0
            spacing: Style.space(4)
            anchors.verticalCenter: parent.verticalCenter
            NumberAnimation on t {
              running: thinkingRow.visible && root.shown
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
                opacity: 0.35 + 0.65 * Math.max(0, Math.sin(dots.t * 6 - index * 0.8))
              }
            }
          }

          Text {
            textFormat: Text.PlainText
            text: "Murmuring…"
            font.family: Style.font.family
            font.pixelSize: Style.font.subtitle
            font.weight: Font.Medium
            color: Util.alpha(Commons.Color.popups.text, 0.9)
            anchors.verticalCenter: parent.verticalCenter
          }
        }

        // Notice: something went wrong (the text was still pasted or copied).
        Row {
          id: noticeRow
          visible: root.notice !== ""
          anchors.centerIn: parent
          spacing: Style.space(10)

          Text {
            textFormat: Text.PlainText
            text: ""
            font.family: Style.font.family
            font.pixelSize: Style.font.heading
            color: Commons.Color.accent
            anchors.verticalCenter: parent.verticalCenter
          }

          Text {
            textFormat: Text.PlainText
            text: root.notice
            font.family: Style.font.family
            font.pixelSize: Style.font.subtitle
            font.weight: Font.Medium
            color: Commons.Color.popups.text
            elide: Text.ElideRight
            width: Math.min(implicitWidth, Style.space(320))
            anchors.verticalCenter: parent.verticalCenter
          }
        }
      }
    }
  }
}
