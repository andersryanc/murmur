import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// Murmur's menu in the Omarchy bar, like the Mac app's menu-bar menu: a mic icon that turns
// red while recording and dims when the daemon isn't running, and a popup with the cleanup,
// microphone and sound settings, recent dictations to copy, and the Dictionary.
//
// The popup runs `murmur settings` when it opens (one JSON object with everything it shows)
// and `murmur set <key> <value>` when something is picked; `set` writes config.json and tells
// the daemon to reload. The icon follows the daemon over the same socket as the pill.
Panel {
  id: root
  moduleName: "plyhdrn.murmur"
  ipcTarget: "plyhdrn.murmur"

  readonly property string murmur: Quickshell.env("HOME") + "/.local/bin/murmur"
  // Matches the daemon's socket path, as in Murmur.qml.
  property string runtimeDir: Quickshell.env("XDG_RUNTIME_DIR") || ""
  readonly property string socketPath: runtimeDir === "" ? "" : runtimeDir + "/murmur/murmur.sock"

  // `murmur settings`, or null before it first answers.
  property var info: null
  // From the socket: ready, recording or processing; "" while not connected.
  property string phase: ""
  // `murmur set` runs one at a time; picks made meanwhile wait here.
  property var pending: []

  readonly property bool running: socket.connected
  readonly property string statusText: {
    if (!root.running) return "Not running"
    if (root.phase === "recording") return "Recording"
    if (root.phase === "processing") return "Murmuring…"
    var status = root.info ? String(root.info.status) : ""
    if (status === "loading") return "Loading the speech model"
    if (status.indexOf("error: ") === 0) return status.slice(7)
    return "Ready"
  }

  readonly property var cleanupModes: [
    { value: "claude", label: "Claude", icon: "󰚩" },
    { value: "ollama", label: "Local", icon: "󰒋" },
    { value: "off", label: "Off", icon: "󰅖" }
  ]
  readonly property var soundThemes: [
    { value: "wood", label: "Wood" },
    { value: "soft", label: "Soft" },
    { value: "chime", label: "Chime" },
    { value: "bubble", label: "Bubble" },
    { value: "off", label: "Off" }
  ]

  // The current choice stays listed even if Ollama doesn't have it (yet).
  readonly property var modelOptions: {
    if (!root.info) return []
    var installed = root.info.localModels || []
    var names = installed.slice()
    if (names.indexOf(root.info.ollamaModel) < 0) names.push(root.info.ollamaModel)
    names.sort()
    return names.map(function(name) {
      return { value: name, label: installed.indexOf(name) < 0 ? name + " (not installed)" : name }
    })
  }

  // A saved mic that's unplugged stays selected, so it's used again when it comes back.
  readonly property var micOptions: {
    if (!root.info) return []
    var sources = root.info.microphones || []
    var fallback = sources.find(function(s) { return s.name === root.info.defaultMicrophone })
    var options = [{
      value: "default",
      label: fallback ? "System default (" + fallback.description + ")" : "System default"
    }]
    sources.forEach(function(s) { options.push({ value: s.name, label: s.description }) })
    var chosen = root.info.microphone
    if (chosen && !sources.some(function(s) { return s.name === chosen }))
      options.push({ value: chosen, label: chosen + " (disconnected, using the default)" })
    return options
  }

  function refresh() {
    if (!infoProc.running) infoProc.running = true
  }

  // Shows the pick at once, then saves it.
  function set(key, value) {
    var next = Object.assign({}, root.info)
    next[key] = key === "microphone" && value === "default" ? null : value
    root.info = next
    root.pending = root.pending.concat([[key, value]])
    runPending()
  }

  function runPending() {
    if (setProc.running || root.pending.length === 0) return
    setProc.command = [root.murmur, "set", root.pending[0][0], root.pending[0][1]]
    root.pending = root.pending.slice(1)
    setProc.running = true
  }

  function copy(text) {
    Quickshell.execDetached(["bash", "-c", "printf %s " + Util.shellQuote(text) + " | wl-copy"])
    root.close()
  }

  // The Dictionary card is in the plugin's pill panel (Murmur.qml), the one always loaded.
  function openDictionary() {
    root.close()
    Quickshell.execDetached(["omarchy-shell", "shell", "call", "plyhdrn.murmur", "openDictionary", ""])
  }

  function startDaemon() {
    Quickshell.execDetached(["bash", "-c",
      "pkill -x murmur; while pgrep -x murmur >/dev/null; do sleep 0.1; done; exec "
      + Util.shellQuote(root.murmur) + " daemon"])
  }

  function quitDaemon() {
    Quickshell.execDetached(["pkill", "-x", "murmur"])
  }

  function preview(text) {
    var flat = String(text).replace(/\n/g, " ")
    return flat.length > 40 ? flat.slice(0, 40) + "…" : flat
  }

  onOpenedChanged: if (opened) refresh()

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  Process {
    running: root.runtimeDir === ""
    command: ["id", "-u"]
    stdout: StdioCollector {
      onStreamFinished: root.runtimeDir = "/tmp/murmur-" + text.trim()
    }
  }

  Process {
    id: infoProc
    command: [root.murmur, "settings"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          root.info = JSON.parse(text)
        } catch (e) {
          console.warn("murmur settings: " + e)
        }
      }
    }
  }

  Process {
    id: setProc
    onExited: root.pending.length > 0 ? root.runPending() : root.refresh()
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
        root.phase = ""
      }
      if (root.opened) root.refresh()
    }

    parser: SplitParser {
      onRead: line => {
        if (line.indexOf("state ") === 0) root.phase = line.slice(6)
      }
    }
  }

  // The daemon may start after the shell, or restart; keep trying until it's there.
  Timer {
    interval: 2000
    repeat: true
    running: !socket.connected && root.socketPath !== ""
    onTriggered: socket.connected = true
  }

  // Loading the model takes a few seconds; keep the status current while the popup shows it.
  Timer {
    interval: 1000
    repeat: true
    running: root.opened && root.running && root.info !== null && String(root.info.status) === "loading"
    onTriggered: root.refresh()
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.running ? "󰍬" : "󰍭"
    active: root.phase === "recording"
    dimmed: !root.running
    tooltipText: root.opened ? "" : "Murmur: " + root.statusText
    onPressed: function(b) { root.toggle() }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(360))
    contentHeight: panel.fittedContentHeight(column.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Column {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        spacing: Style.space(14)

        // ---------- Hero: icon · name and status ----------
        Item {
          width: parent.width
          implicitHeight: Math.max(heroIcon.implicitHeight, heroLabels.implicitHeight)

          Text {
            id: heroIcon
            textFormat: Text.PlainText
            text: button.text
            color: root.phase === "recording" ? root.bar.urgent : root.bar.foreground
            opacity: root.running ? 1 : 0.45
            font.family: root.bar.fontFamily
            font.pixelSize: Style.font.display
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
          }

          Column {
            id: heroLabels
            anchors.left: heroIcon.right
            anchors.leftMargin: Style.space(14)
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(2)

            Text {
              text: "Murmur"
              color: root.bar.foreground
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.title
              font.bold: true
            }

            Text {
              textFormat: Text.PlainText
              text: root.statusText.toUpperCase()
              color: Qt.darker(root.bar.foreground, 1.4)
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
              font.bold: true
              font.letterSpacing: 1.2
              elide: Text.ElideRight
              width: parent.width
            }
          }
        }

        Text {
          visible: root.info !== null && root.info.needsApiKey === true
          width: parent.width
          wrapMode: Text.WordWrap
          textFormat: Text.PlainText
          text: "No Anthropic API key found. Store one with: secret-tool store --label='Murmur' service com.plyhdrn.murmur account anthropic-api-key"
          color: root.bar.urgent
          font.family: root.bar.fontFamily
          font.pixelSize: Style.font.bodySmall
        }

        // ---------- Settings (once `murmur settings` has answered) ----------
        Column {
          visible: root.info !== null
          width: parent.width
          spacing: Style.space(14)

          PanelSeparator { foreground: root.bar.foreground }

          Section {
            title: "CLEANUP"

            ChoiceRow {
              choices: root.cleanupModes
              current: root.info ? root.info.cleanup : ""
              onPicked: function(value) { root.set("cleanup", value) }
            }

            Dropdown {
              visible: root.info !== null && root.info.cleanup === "ollama"
              width: parent.width
              showLabel: false
              fontFamily: root.bar.fontFamily
              options: root.modelOptions
              value: root.info ? root.info.ollamaModel : ""
              onChanged: function(value) { root.set("ollamaModel", value) }
            }
          }

          Section {
            title: "MICROPHONE"

            Dropdown {
              width: parent.width
              showLabel: false
              fontFamily: root.bar.fontFamily
              options: root.micOptions
              value: root.info && root.info.microphone ? root.info.microphone : "default"
              onChanged: function(value) { root.set("microphone", value) }
            }
          }

          Section {
            title: "SOUND EFFECTS"

            ChoiceRow {
              choices: root.soundThemes
              current: root.info ? root.info.sounds : ""
              onPicked: function(value) { root.set("sounds", value) }
            }
          }

          PanelSeparator { foreground: root.bar.foreground }

          Section {
            title: "RECENT · CLICK TO COPY"

            Text {
              visible: !root.info || (root.info.recent || []).length === 0
              text: "No dictations yet"
              color: root.bar.foreground
              opacity: 0.6
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.bodySmall
            }

            Repeater {
              model: root.info ? root.info.recent || [] : []
              Button {
                required property var modelData
                width: parent.width
                leftAlign: true
                text: root.preview(modelData)
                fontSize: Style.font.bodySmall
                foreground: root.bar.foreground
                fontFamily: root.bar.fontFamily
                onClicked: root.copy(modelData)
              }
            }
          }

          PanelSeparator { foreground: root.bar.foreground }

          Row {
            id: actions
            width: parent.width
            spacing: Style.space(6)

            readonly property int count: root.running ? 3 : 2
            readonly property real cellWidth: (width - spacing * (count - 1)) / count

            ActionButton {
              iconText: "󰗚"
              text: "Dictionary"
              onClicked: root.openDictionary()
            }
            ActionButton {
              iconText: root.running ? "󰑓" : "󰐊"
              text: root.running ? "Restart" : "Start"
              onClicked: root.startDaemon()
            }
            ActionButton {
              visible: root.running
              iconText: "󰐥"
              text: "Quit"
              onClicked: root.quitDaemon()
            }
          }
        }
      }
    }
  }

  // A titled group of controls.
  component Section: Column {
    property string title: ""
    default property alias body: content.data

    width: parent.width
    spacing: Style.space(10)

    PanelSectionHeader {
      text: title
      foreground: root.bar.foreground
      fontFamily: root.bar.fontFamily
    }

    Column {
      id: content
      width: parent.width
      spacing: Style.space(6)
    }
  }

  // One button per choice, the current one highlighted, as in the power panel's profiles.
  component ChoiceRow: Row {
    id: choiceRow
    property var choices: []
    property string current: ""
    signal picked(string value)

    width: parent.width
    spacing: Style.space(6)
    readonly property real cellWidth: choices.length > 0 ? (width - spacing * (choices.length - 1)) / choices.length : 0

    Repeater {
      model: choiceRow.choices
      Button {
        required property var modelData
        width: choiceRow.cellWidth
        iconText: modelData.icon || ""
        iconSize: Style.font.title
        text: modelData.label
        fontSize: Style.font.bodySmall
        foreground: root.bar.foreground
        fontFamily: root.bar.fontFamily
        horizontalPadding: Style.spacing.controlPaddingX
        verticalPadding: Style.spacing.controlPaddingY + Style.space(2)
        bordered: true
        active: choiceRow.current === modelData.value
        onClicked: choiceRow.picked(modelData.value)
      }
    }
  }

  component ActionButton: Button {
    width: actions.cellWidth
    iconSize: Style.font.title
    fontSize: Style.font.bodySmall
    foreground: root.bar.foreground
    fontFamily: root.bar.fontFamily
    horizontalPadding: Style.spacing.controlPaddingX
    verticalPadding: Style.spacing.controlPaddingY + Style.space(2)
    bordered: true
  }
}
