import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

// How syncing is going, read from the status file Omarchy Sync writes after every sync (from
// its window or the 15-minute background sync). Dim when everything is in step, spinning while
// syncing, and in the alert colour when something waits for you. Click to open the app.
BarWidget {
  id: root
  moduleName: "io.github.tsubaie.omarchy-sync"

  property string syncState: "synced"
  property string message: "Omarchy Sync"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  FileView {
    path: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state")
      + "/omarchy-sync/status.json"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      try {
        const status = JSON.parse(text())
        root.syncState = status.state || "synced"
        root.message = status.text || "Omarchy Sync"
      } catch (e) {
        root.syncState = "synced"
      }
    }
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    // Material Design sync icons: sync, sync with an alert, sync off.
    text: root.syncState === "attention" ? "\u{F04E7}" : root.syncState === "error" ? "\u{F04E8}" : "\u{F04E6}"
    active: root.syncState === "attention" || root.syncState === "error"
    dimmed: root.syncState === "synced"
    slotSize: Style.bar.statusSlot
    fontSize: Style.font.caption
    tooltipText: "Omarchy Sync: " + root.message
    transformOrigin: Item.Center
    onPressed: Quickshell.execDetached(["omarchy-launch-or-focus", "omarchy-sync", "omarchy-sync"])

    RotationAnimation on rotation {
      from: 0
      to: 360
      duration: 1200
      loops: Animation.Infinite
      running: root.syncState === "syncing"
      onRunningChanged: if (!running) button.rotation = 0
    }
  }
}
