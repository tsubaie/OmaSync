import QtQuick
import QtQuick.Shapes
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

// How syncing is going, read from the status file Omarchy Sync writes after every sync (from
// its window or the 15-minute background sync), shown as the app's own mark. It sits quietly in
// the bar's colour, spins in green while syncing, stays green for a few seconds when a sync
// finishes well, turns red when syncing fails and yellow when something waits for you, using
// the Omarchy theme's own colours. Click to open Omarchy Sync on its Sync page.
//
// The shell keeps a widget's code by file name, so this file's name carries a number that goes
// up whenever it changes (see BAR_FILES in src-tauri/src/sync.rs). QML only loads files named
// like types: a capital letter, then letters and digits (no hyphens).
BarWidget {
  id: root
  moduleName: "io.github.tsubaie.omarchy-sync"

  property string syncState: "synced"
  property string message: "Everything matches your other computers"
  // True for a few seconds after a sync ends with everything in step.
  property bool justSynced: false

  // The theme's own green, yellow, red and blue, with fallbacks.
  property color good: "#8dbf6a"
  property color waiting: "#e5c07b"
  property color bad: Color.urgent
  readonly property color idle: root.bar ? root.bar.barForeground : Color.foreground

  readonly property color tint: syncState === "syncing" ? good
    : syncState === "attention" ? waiting
    : syncState === "error" ? bad
    : justSynced ? good
    : idle

  onSyncStateChanged: if (syncState !== "synced") justSynced = false

  Timer {
    id: greenTimer
    interval: 4000
    onTriggered: root.justSynced = false
  }

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
        const next = status.state || "synced"
        // A sync that just finished in step shows green for a moment.
        if (root.syncState === "syncing" && next === "synced") {
          root.justSynced = true
          greenTimer.restart()
        }
        root.syncState = next
        root.message = status.text || ""
      } catch (e) {
        root.syncState = "synced"
      }
    }
  }

  FileView {
    path: Color.currentThemePath + "/colors.toml"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      // Omarchy themes name them (green = "#..."); older ones number them (color2 = "#...").
      const pick = (...keys) => {
        for (const key of keys) {
          const match = text().match(new RegExp("^" + key + "\\s*=\\s*\"(#[0-9a-fA-F]{6})\"", "m"))
          if (match) return match[1]
        }
        return null
      }
      root.good = pick("green", "color2") || "#8dbf6a"
      root.waiting = pick("yellow", "color3") || "#e5c07b"
      root.bad = pick("red", "color1") || Color.urgent
    }
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    tooltipText: "Omarchy Sync: " + root.message
    onPressed: Quickshell.execDetached(["omarchy-sync"])

    iconComponent: Component {
      Item {
        id: mark
        // The app's two-arrow mark, drawn on a 106-unit square and scaled to the icon slot.
        Shape {
          width: 106
          height: 106
          scale: Math.min(mark.width, mark.height) / 106
          transformOrigin: Item.TopLeft
          preferredRendererType: Shape.CurveRenderer
          ShapePath {
            strokeWidth: -1
            fillColor: root.tint
            PathSvg { path: "M9 77V26Q9 11.5 23.5 11.5H60V1L83 20.25 60 39.5V29H27V77Z" }
          }
          ShapePath {
            strokeWidth: -1
            fillColor: Qt.darker(root.tint, 1.25)
            PathSvg { path: "M97 29V80Q97 94.5 82.5 94.5H46V105L23 85.75 46 66.5V77H79V29Z" }
          }
        }

        RotationAnimation on rotation {
          from: 0
          to: 360
          duration: 1200
          loops: Animation.Infinite
          running: root.syncState === "syncing"
          onRunningChanged: if (!running) mark.rotation = 0
        }
      }
    }
  }
}
