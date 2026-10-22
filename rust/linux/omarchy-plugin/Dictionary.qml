import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// The Dictionary, a port of the Mac app's Dictionary window: your Words (spellings for the
// cleanup step) and Replacements (exact "when I say X, write Y" rules). It's a modal card over
// a scrim, like Omarchy's emoji picker, opened from the bar menu (or by
// `omarchy-shell shell call plyhdrn.murmur openDictionary ""`).
//
// dictionary.json stays the source of truth: the daemon re-reads it every dictation, and it can
// still be edited by hand. Every change here saves the whole file at once, and an edit made on
// disk while the card is open shows up in it. If the file doesn't parse, nothing is saved over
// it until it's fixed.
Item {
  id: root

  readonly property string path: (Quickshell.env("XDG_CONFIG_HOME") || Quickshell.env("HOME") + "/.config")
    + "/murmur/dictionary.json"

  property bool opened: false
  // Picked when the card opens, so it stays put if the focus moves to another monitor.
  property var targetScreen: null

  // Entries carry a runtime id so selection and editing survive re-sorting; the file doesn't.
  property int nextId: 1
  property var words: []          // [{id, text}]
  property var replacements: []   // [{id, from, to}]
  property string tab: "words"
  property string query: ""
  property var selected: null     // id in the current tab
  property var editing: null      // {id, field}
  property string editValue: ""
  // The file couldn't be read or written.
  property string error: ""
  // The file on disk doesn't parse; saving would overwrite the user's half-done hand edit.
  property bool unreadable: false
  property var undoStack: []
  property var message: null      // {text, undo}, shown in the footer for a few seconds

  readonly property bool pair: tab === "replacements"
  readonly property var entries: pair ? replacements : words
  readonly property var shown: visibleEntries(entries, query)
  readonly property string fileName: "dictionary.json"

  property color background: Commons.Color.menu.background
  property color foreground: Commons.Color.menu.text
  property color border: Commons.Color.menu.border
  property var borderSpec: Border.surfaceSpec("menu", "border", border, Math.max(1, Style.space(2)))
  property color scrim: Commons.Color.menu.scrim
  property color selectedBackground: Commons.Color.menu.selectedBackground
  property color accent: Commons.Color.accent
  property color urgent: Commons.Color.urgent
  property string fontFamily: Style.font.menuFamily
  readonly property int rowHeight: Style.spacing.popupRowHeight
  // A replacement column's width, shared by the add row and the list so their text lines up.
  readonly property real column: (card.width - card.contentLeftInset - card.contentRightInset
    - addButtonWidth - arrowWidth - Style.spacing.controlGap * 3) / 2
  readonly property real addButtonWidth: Style.space(72)
  readonly property real arrowWidth: Style.space(16)

  function open() {
    file.reload()
    var name = Hyprland.focusedMonitor ? String(Hyprland.focusedMonitor.name || "") : ""
    root.targetScreen = Quickshell.screens.find(function(s) { return s.name === name }) || null
    root.opened = true
    root.query = ""
    root.selected = null
    root.editing = null
    Qt.callLater(focusAdd)
  }

  function close() {
    if (root.editing) finishEdit(true)
    root.opened = false
    root.message = null
  }

  // MARK: - Text rules

  // Trimmed, with runs of spaces collapsed: what gets saved.
  function tidy(s) { return String(s).replace(/\s+/g, " ").trim() }
  // How the daemon matches a replacement: any case, spaces, hyphens and commas alike.
  function ruleKey(s) { return String(s).toLowerCase().split(/[\s,\-]+/).filter(Boolean).join(" ") }
  function findWord(text, except) {
    return root.words.find(function(w) { return w.text === text && w.id !== except })
  }
  function findRule(from, except) {
    var key = ruleKey(from)
    if (!key) return undefined
    return root.replacements.find(function(r) { return ruleKey(r.from) === key && r.id !== except })
  }
  function compare(a, b) {
    return a.toLowerCase().localeCompare(b.toLowerCase()) || a.localeCompare(b)
  }

  // MARK: - Data in and out

  function plain() {
    return {
      replacements: root.replacements.map(function(r) { return { from: r.from, to: r.to } }),
      words: root.words.map(function(w) { return w.text })
    }
  }

  function setData(data) {
    var id = root.nextId
    root.words = (data.words || []).map(function(text) { return { id: id++, text: text } })
    root.replacements = (data.replacements || []).map(function(r) { return { id: id++, from: r.from, to: r.to } })
    root.nextId = id
  }

  // The file's contents: when it first loads, and whenever it changes on disk.
  function load(raw) {
    var data
    try {
      data = raw.trim() === "" ? {} : JSON.parse(raw)
      var wordsOk = data.words === undefined || (Array.isArray(data.words)
        && data.words.every(function(w) { return typeof w === "string" }))
      var rulesOk = data.replacements === undefined || (Array.isArray(data.replacements)
        && data.replacements.every(function(r) { return r && typeof r.from === "string" && typeof r.to === "string" }))
      if (data === null || typeof data !== "object" || !wordsOk || !rulesOk)
        throw new Error("expected words and replacements")
    } catch (e) {
      root.unreadable = true
      root.error = root.fileName + " has a mistake in it (" + e.message + "). Fix it in a text editor first."
      return
    }
    root.unreadable = false
    root.error = ""
    var tidied = { replacements: data.replacements || [], words: data.words || [] }
    if (JSON.stringify(tidied) === JSON.stringify(plain())) return
    var keep = selectedValue()
    setData(tidied)
    // Changed outside the card: an undo would overwrite those changes.
    root.undoStack = []
    root.editing = null
    root.selected = keep ? idForValue(keep) : null
  }

  function save() {
    if (root.unreadable) {
      root.error = "Not saved: " + root.fileName + " has a mistake in it. Fix it in a text editor first."
      return
    }
    file.setText(JSON.stringify(plain(), null, 2))
  }

  // Every change goes through here: remembered for undo, then saved.
  function change(mutate) {
    var stack = root.undoStack.concat([JSON.stringify(plain())])
    root.undoStack = stack.length > 100 ? stack.slice(1) : stack
    mutate()
    // Reassigned so the bindings that read them notice.
    root.words = root.words.slice()
    root.replacements = root.replacements.slice()
    save()
  }

  function undo() {
    if (root.undoStack.length === 0) return
    var previous = root.undoStack[root.undoStack.length - 1]
    root.undoStack = root.undoStack.slice(0, -1)
    var keep = selectedValue()
    setData(JSON.parse(previous))
    root.selected = keep ? idForValue(keep) : null
    root.editing = null
    save()
    showMessage("", false)
  }

  // MARK: - Selection helpers

  function entry(id) { return root.entries.find(function(e) { return e.id === id }) }
  function label(e) { return root.pair ? e.from : e.text }
  function valueOf(e) { return JSON.stringify(root.pair ? [e.from, e.to] : e.text) }
  function selectedValue() {
    var e = root.selected !== null ? entry(root.selected) : null
    return e ? valueOf(e) : null
  }
  function idForValue(value) {
    var e = root.entries.find(function(e) { return valueOf(e) === value })
    return e ? e.id : null
  }

  // The entries shown: filtered by the search, sorted alphabetically.
  function visibleEntries(list, query) {
    var q = query.toLowerCase()
    var pair = root.pair
    return list.filter(function(e) {
      if (!q) return true
      return pair ? e.from.toLowerCase().indexOf(q) >= 0 || e.to.toLowerCase().indexOf(q) >= 0
        : e.text.toLowerCase().indexOf(q) >= 0
    }).sort(function(a, b) {
      return pair ? compare(a.from, b.from) || compare(a.to, b.to) : compare(a.text, b.text)
    })
  }

  function indexOfId(id) {
    for (var i = 0; i < root.shown.length; i++) if (root.shown[i].id === id) return i
    return -1
  }

  function showMessage(text, withUndo) {
    root.message = text ? { text: text, undo: withUndo === true } : null
    if (root.message) messageTimer.restart()
  }

  // Scrolls to an entry and briefly highlights it, without taking focus.
  function reveal(id) {
    var index = indexOfId(id)
    if (index < 0) return
    list.positionViewAtIndex(index, ListView.Contain)
    root.flashId = -1
    root.flashId = id
    flashTimer.restart()
  }
  property int flashId: -1

  // The entry the add row would duplicate, outlined while typing.
  readonly property var match: {
    if (!root.pair) {
      var text = tidy(wordField.text)
      return text ? findWord(text) || null : null
    }
    var from = tidy(fromField.text)
    return from ? findRule(from) || null : null
  }
  onMatchChanged: if (root.match) list.positionViewAtIndex(indexOfId(root.match.id), ListView.Contain)

  // MARK: - Adding

  function addWord() {
    var text = tidy(wordField.text)
    wordField.text = ""
    if (!text) return
    var existing = findWord(text)
    if (existing) {
      reveal(existing.id)
      return
    }
    clearSearchIfHiding([text])
    var added = { id: root.nextId++, text: text }
    change(function() { root.words.push(added) })
    showMessage("Added “" + text + "”.", true)
    Qt.callLater(function() { reveal(added.id) })
  }

  function addRule() {
    var from = tidy(fromField.text), to = tidy(toField.text)
    if (!from) return fromField.forceActiveFocus()
    if (!to) return toField.forceActiveFocus()
    var existing = findRule(from)
    fromField.text = ""
    toField.text = ""
    fromField.forceActiveFocus()
    var id
    if (existing && existing.to === to) {
      id = existing.id
    } else if (existing) {
      id = existing.id
      change(function() { existing.to = to })
      showMessage("Updated “" + existing.from + "”.", true)
    } else {
      id = root.nextId++
      clearSearchIfHiding([from, to])
      change(function() { root.replacements.push({ id: id, from: from, to: to }) })
      showMessage("Added “" + from + "”.", true)
    }
    Qt.callLater(function() { reveal(id) })
  }

  function clearSearchIfHiding(texts) {
    var q = root.query.toLowerCase()
    if (q && !texts.some(function(t) { return t.toLowerCase().indexOf(q) >= 0 })) {
      searchField.text = ""
    }
  }

  // MARK: - Editing in place

  function startEdit(id, field) {
    var e = entry(id)
    if (!e) return
    var f = field || (root.pair ? "from" : "text")
    root.selected = id
    root.editValue = e[f]
    root.editing = { id: id, field: f }
  }

  // Ends the edit; `next` moves on to the other field of a replacement (Tab).
  function finishEdit(keep, next) {
    var editing = root.editing
    if (!editing) return
    root.editing = null
    var e = entry(editing.id)
    if (e && keep) {
      var value = tidy(root.editValue)
      var problem = ""
      if (editing.field === "text" && findWord(value, e.id)) problem = "“" + value + "” is already in your words."
      else if (editing.field === "from" && findRule(value, e.id)) problem = "There’s already a replacement for “" + value + "”."
      if (problem) showMessage(problem, false)
      // An emptied field goes back to what it was; delete is the way to remove an entry.
      if (value && !problem && value !== e[editing.field]) {
        change(function() { e[editing.field] = value })
      }
    }
    if (next && e) {
      startEdit(e.id, next)
      return
    }
    list.forceActiveFocus()
  }

  // MARK: - Deleting

  function remove(id) {
    var index = indexOfId(id)
    var e = entry(id)
    if (!e) return
    var after = root.shown[index + 1] || root.shown[index - 1]
    var pair = root.pair
    change(function() {
      if (pair) root.replacements.splice(root.replacements.indexOf(e), 1)
      else root.words.splice(root.words.indexOf(e), 1)
    })
    if (root.selected === id) root.selected = after ? after.id : null
    showMessage("Deleted “" + label(e) + "”.", true)
  }

  // MARK: - Tabs and focus

  function setTab(next) {
    if (root.tab === next) return
    if (root.editing) finishEdit(true)
    root.tab = next
    root.selected = null
    searchField.text = ""
    focusAdd()
  }

  function focusAdd() {
    if (root.pair) fromField.forceActiveFocus()
    else wordField.forceActiveFocus()
  }

  function moveSelection(delta) {
    if (root.shown.length === 0) return
    var index = indexOfId(root.selected)
    index = index < 0 ? (delta > 0 ? 0 : root.shown.length - 1)
      : Math.max(0, Math.min(root.shown.length - 1, index + delta))
    root.selected = root.shown[index].id
    list.positionViewAtIndex(index, ListView.Contain)
  }

  // Esc backs out one step at a time: the edit, then the search or a half-typed entry, then
  // the card.
  function backOut() {
    if (root.editing) {
      finishEdit(false)
    } else if (searchField.activeFocus && searchField.text) {
      searchField.text = ""
    } else if (wordField.activeFocus && wordField.text) {
      wordField.text = ""
    } else if ((fromField.activeFocus || toField.activeFocus) && (fromField.text || toField.text)) {
      fromField.text = ""
      toField.text = ""
    } else if (list.activeFocus && root.selected !== null) {
      root.selected = null
      focusAdd()
    } else {
      close()
    }
  }

  function editFile() {
    var path = Util.shellQuote(root.path)
    close()
    Quickshell.execDetached(["bash", "-c",
      "[ -e " + path + " ] || printf '{\\n  \"replacements\": [],\\n  \"words\": []\\n}' > " + path
      + "; exec omarchy-launch-editor " + path])
  }

  FileView {
    id: file
    path: root.path
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: root.load(text())
    onLoadFailed: function(error) {
      if (error === FileViewError.FileNotFound) {
        root.load("")
      } else {
        root.unreadable = true
        root.error = "Can't read " + root.fileName + "."
      }
    }
    onSaved: if (!root.unreadable) root.error = ""
    onSaveFailed: function(error) { root.error = "Couldn't save " + root.fileName + "." }
  }

  Timer {
    id: messageTimer
    interval: 6000
    onTriggered: root.message = null
  }

  Timer {
    id: flashTimer
    interval: 700
    onTriggered: root.flashId = -1
  }

  PanelWindow {
    id: panel
    visible: root.opened
    screen: root.targetScreen
    anchors { top: true; bottom: true; left: true; right: true }
    color: "transparent"
    WlrLayershell.namespace: "murmur-dictionary"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
    exclusionMode: ExclusionMode.Ignore

    Shortcut { sequence: "Escape"; onActivated: root.backOut() }
    Shortcut { sequence: "Ctrl+W"; onActivated: root.close() }
    Shortcut { sequence: "Ctrl+F"; onActivated: { searchField.forceActiveFocus(); searchField.selectAll() } }
    Shortcut { sequence: "Ctrl+1"; onActivated: root.setTab("words") }
    Shortcut { sequence: "Ctrl+2"; onActivated: root.setTab("replacements") }
    Shortcut { sequence: "Ctrl+N"; onActivated: root.focusAdd() }
    // Text fields keep Ctrl+Z for their own undo; this one is for the list.
    Shortcut { sequence: "Ctrl+Z"; onActivated: root.undo() }

    Rectangle {
      anchors.fill: parent
      color: root.scrim
    }

    MouseArea {
      anchors.fill: parent
      onClicked: root.close()
    }

    BorderSurface {
      id: card
      width: Math.min(Style.space(560), panel.width - Style.gapsOut * 2)
      height: Math.min(Style.space(520), panel.height - Style.gapsOut * 2)
      radius: Style.cornerRadius
      anchors.centerIn: parent
      color: root.background
      borderSpec: root.borderSpec
      padding: Style.spacing.panelPadding

      MouseArea { anchors.fill: parent; onClicked: {} }

      Column {
        id: content
        anchors.fill: parent
        anchors.topMargin: card.contentTopInset
        anchors.rightMargin: card.contentRightInset
        anchors.bottomMargin: card.contentBottomInset
        anchors.leftMargin: card.contentLeftInset
        spacing: Style.spacing.xxl

        // ---------- Tabs · search ----------
        Item {
          width: parent.width
          height: Math.max(tabs.implicitHeight, searchField.implicitHeight)

          Row {
            id: tabs
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.spacing.md

            Repeater {
              model: [
                { value: "words", label: "Words", count: root.words.length },
                { value: "replacements", label: "Replacements", count: root.replacements.length }
              ]
              Button {
                required property var modelData
                text: modelData.label + (modelData.count ? "  " + modelData.count : "")
                tooltipText: modelData.value === "words" ? "Ctrl+1" : "Ctrl+2"
                fontSize: Style.font.body
                foreground: root.foreground
                fontFamily: root.fontFamily
                bordered: true
                active: root.tab === modelData.value
                onClicked: root.setTab(modelData.value)
              }
            }
          }

          TextField {
            id: searchField
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(Style.space(180), parent.width - tabs.width - Style.spacing.controlGap)
            placeholderText: "Search"
            foreground: root.foreground
            accent: root.accent
            font.family: root.fontFamily
            enabled: root.entries.length > 0 || text !== ""
            onTextChanged: root.query = text.trim()
            Keys.onDownPressed: { list.forceActiveFocus(); root.moveSelection(1) }
          }
        }

        Text {
          id: explanation
          width: parent.width
          wrapMode: Text.WordWrap
          textFormat: Text.StyledText
          text: root.pair
            ? "Exact swaps made in every dictation, even with cleanup off. Any case, whole words only: <b>cube control</b> → <b>kubectl</b>."
            : "Names and jargon Murmur should spell your way. Cleanup uses them to fix near-misses, like <b>Plyhdrn</b> or <b>Kubernetes</b>."
          color: root.foreground
          opacity: 0.7
          font.family: root.fontFamily
          font.pixelSize: Style.font.bodySmall
        }

        // ---------- Add row ----------
        Item {
          id: addRow
          width: parent.width
          height: wordField.implicitHeight

          Row {
            visible: !root.pair
            anchors.fill: parent
            spacing: Style.spacing.controlGap

            TextField {
              id: wordField
              width: parent.width - addWord.width - parent.spacing
              placeholderText: "Add a word or name"
              foreground: root.foreground
              accent: root.accent
              font.family: root.fontFamily
              rightPadding: Style.spacing.controlPaddingX + (wordBadge.visible ? wordBadge.width + Style.spacing.md : 0)
              onAccepted: root.addWord()
              Keys.onDownPressed: { list.forceActiveFocus(); root.moveSelection(1) }

              Badge { id: wordBadge; text: root.match && !root.pair ? "Already added" : "" }
            }

            Button {
              id: addWord
              width: root.addButtonWidth
              height: parent.height
              text: "Add"
              fontSize: Style.font.body
              foreground: root.foreground
              fontFamily: root.fontFamily
              bordered: true
              readonly property bool ready: root.tidy(wordField.text) !== "" && !root.match
              enabled: ready
              opacity: ready ? 1 : 0.45
              onClicked: root.addWord()
            }
          }

          Row {
            id: ruleRow
            visible: root.pair
            anchors.fill: parent
            spacing: Style.spacing.controlGap

            readonly property bool same: root.match !== null && root.match.to === root.tidy(toField.text)

            TextField {
              id: fromField
              width: root.column
              placeholderText: "When I say…"
              foreground: root.foreground
              accent: root.accent
              font.family: root.fontFamily
              rightPadding: Style.spacing.controlPaddingX + (fromBadge.visible ? fromBadge.width + Style.spacing.md : 0)
              onAccepted: root.addRule()
              Keys.onDownPressed: { list.forceActiveFocus(); root.moveSelection(1) }

              Badge {
                id: fromBadge
                text: !root.match || !root.pair ? ""
                  : (ruleRow.same || root.tidy(toField.text) === "" ? "Already added" : "Will update")
              }
            }

            Arrow { height: parent.height }

            TextField {
              id: toField
              width: root.column
              placeholderText: "Write…"
              foreground: root.foreground
              accent: root.accent
              font.family: root.fontFamily
              onAccepted: root.addRule()
              Keys.onDownPressed: { list.forceActiveFocus(); root.moveSelection(1) }
            }

            Button {
              width: root.addButtonWidth
              height: parent.height
              text: root.match && root.tidy(toField.text) !== "" && !ruleRow.same ? "Update" : "Add"
              fontSize: Style.font.body
              foreground: root.foreground
              fontFamily: root.fontFamily
              bordered: true
              readonly property bool ready: root.tidy(fromField.text) !== "" && root.tidy(toField.text) !== "" && !ruleRow.same
              enabled: ready
              opacity: ready ? 1 : 0.45
              onClicked: root.addRule()
            }
          }
        }

        // ---------- The list ----------
        BorderSurface {
          width: parent.width
          height: parent.height - y - footer.height - parent.spacing
          radius: Style.cornerRadius
          color: Style.controlFill(false, false, root.foreground, root.accent)
          borderSpec: Border.controlSpec(list.activeFocus ? "focus" : "normal", root.foreground, root.accent)

          ListView {
            id: list
            anchors.fill: parent
            anchors.margins: Style.spacing.sm
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: root.shown
            activeFocusOnTab: true

            Keys.onUpPressed: {
              if (root.selected === null || root.indexOfId(root.selected) === 0) root.focusAdd()
              else root.moveSelection(-1)
            }
            Keys.onDownPressed: root.moveSelection(1)
            Keys.onReturnPressed: if (root.selected !== null) root.startEdit(root.selected)
            Keys.onEnterPressed: if (root.selected !== null) root.startEdit(root.selected)
            Keys.onDeletePressed: if (root.selected !== null) root.remove(root.selected)
            Keys.onPressed: function(event) {
              if (event.key === Qt.Key_Backspace && root.selected !== null) {
                root.remove(root.selected)
                event.accepted = true
              }
            }

            delegate: Rectangle {
              id: row
              required property var modelData
              required property int index

              readonly property bool isSelected: root.selected === modelData.id
              readonly property bool isEditing: root.editing !== null && root.editing.id === modelData.id
              readonly property bool outlined: root.match !== null && root.match.id === modelData.id

              width: ListView.view.width
              height: root.rowHeight
              radius: Style.cornerRadius
              color: row.isSelected && list.activeFocus && !row.isEditing ? Util.alpha(root.accent, 0.3)
                : row.isSelected || root.flashId === modelData.id ? root.selectedBackground
                : mouse.containsMouse ? Util.alpha(root.foreground, 0.04)
                : "transparent"
              border.width: row.outlined ? 1 : 0
              border.color: root.accent

              Behavior on color { ColorAnimation { duration: 120 } }

              MouseArea {
                id: mouse
                anchors.fill: parent
                hoverEnabled: true
                onClicked: {
                  if (root.editing && !row.isEditing) root.finishEdit(true)
                  root.selected = modelData.id
                  list.forceActiveFocus()
                }
                onDoubleClicked: function(event) {
                  root.startEdit(modelData.id, root.pair && event.x > root.column + root.arrowWidth ? "to" : undefined)
                }
              }

              Row {
                anchors.left: parent.left
                anchors.right: deleteButton.left
                anchors.leftMargin: Style.spacing.sm
                anchors.verticalCenter: parent.verticalCenter
                spacing: Style.spacing.controlGap

                Cell {
                  entry: row.modelData
                  field: root.pair ? "from" : "text"
                  editing: row.isEditing && root.editing.field === field
                  width: root.pair ? root.column - Style.spacing.sm : parent.width
                }
                Arrow { visible: root.pair; height: row.height }
                Cell {
                  visible: root.pair
                  entry: row.modelData
                  field: "to"
                  editing: row.isEditing && root.editing.field === field
                  width: root.column
                }
              }

              Button {
                id: deleteButton
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                visible: (mouse.containsMouse || hovering || row.isSelected) && !row.isEditing
                property bool hovering: false
                onHovered: function(isHovered) { hovering = isHovered }
                iconText: "󰅖"
                tooltipText: "Delete"
                foreground: root.foreground
                fontFamily: root.fontFamily
                horizontalPadding: Style.spacing.sm
                verticalPadding: Style.spacing.xxs
                onClicked: root.remove(modelData.id)
              }

            }
          }

          // ---------- Empty ----------
          Column {
            anchors.centerIn: parent
            width: parent.width - Style.space(40)
            spacing: Style.spacing.md
            visible: root.shown.length === 0

            Text {
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              textFormat: Text.PlainText
              text: root.query ? "No matches" : root.pair ? "No replacements yet" : "No words yet"
              color: root.foreground
              opacity: 0.8
              font.family: root.fontFamily
              font.pixelSize: Style.font.title
              font.bold: true
            }
            Text {
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              wrapMode: Text.WordWrap
              textFormat: Text.PlainText
              text: root.query
                ? "Nothing " + (root.pair ? "in your replacements" : "in your words") + " contains “" + root.query + "”."
                : root.pair ? "Type what you say and what Murmur should write instead, then press Return."
                : "Add names, product names and jargon you use, then press Return."
              color: root.foreground
              opacity: 0.6
              font.family: root.fontFamily
              font.pixelSize: Style.font.bodySmall
            }
          }
        }

        // ---------- Footer: counts, or a message with Undo; the file ----------
        Item {
          id: footer
          width: parent.width
          height: editButton.implicitHeight

          Text {
            id: status
            anchors.left: parent.left
            anchors.right: undoButton.visible ? undoButton.left : editButton.left
            anchors.rightMargin: Style.spacing.controlGap
            anchors.verticalCenter: parent.verticalCenter
            elide: Text.ElideRight
            textFormat: Text.PlainText
            text: {
              if (root.error) return root.error
              if (root.message) return root.message.text
              var w = root.words.length, r = root.replacements.length
              return w + " word" + (w === 1 ? "" : "s") + ", " + r + " replacement" + (r === 1 ? "" : "s")
            }
            color: root.error ? root.urgent : root.foreground
            opacity: root.error ? 1 : 0.7
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
          }

          Button {
            id: undoButton
            anchors.right: editButton.left
            anchors.rightMargin: Style.spacing.md
            anchors.verticalCenter: parent.verticalCenter
            visible: root.message !== null && root.message.undo && !root.error
            text: "Undo"
            tooltipText: "Ctrl+Z"
            fontSize: Style.font.bodySmall
            foreground: root.accent
            fontFamily: root.fontFamily
            onClicked: root.undo()
          }

          Button {
            id: editButton
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            iconText: "󰏫"
            text: "Edit file"
            tooltipText: root.path
            fontSize: Style.font.bodySmall
            foreground: root.foreground
            fontFamily: root.fontFamily
            bordered: true
            onClicked: root.editFile()
          }
        }
      }
    }
  }

  // One field of an entry: its text, or a text field while it's being edited.
  component Cell: Item {
    id: cell
    property var entry: ({})
    property string field: "text"
    property bool editing: false
    height: root.rowHeight

    Text {
      anchors.fill: parent
      anchors.leftMargin: Style.spacing.sm
      visible: !cell.editing
      verticalAlignment: Text.AlignVCenter
      textFormat: Text.PlainText
      text: cell.entry[cell.field] || ""
      elide: Text.ElideRight
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
    }

    Loader {
      anchors.fill: parent
      active: cell.editing
      sourceComponent: TextField {
        text: root.editValue
        foreground: root.foreground
        accent: root.accent
        font.family: root.fontFamily
        verticalPadding: Style.spacing.xxs
        onTextEdited: root.editValue = text
        onAccepted: root.finishEdit(true)
        onActiveFocusChanged: if (!activeFocus && cell.editing) root.finishEdit(true)
        Keys.onTabPressed: root.finishEdit(true, root.pair && cell.field === "from" ? "to" : undefined)
        Component.onCompleted: { forceActiveFocus(); selectAll() }
      }
    }
  }

  // "Already added" / "Will update", inside the right end of an add field.
  component Badge: Text {
    visible: text !== ""
    anchors.right: parent.right
    anchors.rightMargin: Style.spacing.controlPaddingX
    anchors.verticalCenter: parent.verticalCenter
    textFormat: Text.PlainText
    color: root.foreground
    opacity: 0.55
    font.family: root.fontFamily
    font.pixelSize: Style.font.caption
  }

  component Arrow: Text {
    width: root.arrowWidth
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    text: "→"
    color: root.foreground
    opacity: 0.55
    font.family: root.fontFamily
    font.pixelSize: Style.font.body
  }
}
