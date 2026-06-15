import AppKit
import AVFoundation
import SwiftUI

/// AppDelegate — manages NSApplication lifecycle, menu bar,
/// permissions, and provides the dependency container for services.
///
/// Runs before SwiftUI body is evaluated, so this is where we
/// request microphone + accessibility permissions and wire up
/// the global hotkey listener (Phase 5).
final class AppDelegate: NSObject, NSApplicationDelegate {

    // ── Window references ──────────────────────────────────────────────

    private var mainWindow: NSWindow?
    private var settingsWindow: NSWindow?

    // ── Menu bar ───────────────────────────────────────────────────────

    private var statusItem: NSStatusItem?

    func applicationDidFinishLaunching(_ notification: Notification) {
        setupMenuBar()
        requestPermissions()

        // Record reference to main window for menu bar "Show" action
        DispatchQueue.main.async { [weak self] in
            self?.mainWindow = NSApp.windows.first
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        // Future: stop embedded HTTP server, flush model cache
        NSLog("[voice] app terminating")
    }

    /// Keep running when window is closed (menu bar app behavior).
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return false
    }

    // ── Menu Bar ───────────────────────────────────────────────────────

    private func setupMenuBar() {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)

        if let button = statusItem?.button {
            button.title = "🎙️"
            button.toolTip = "Anything Voice"
        }

        let menu = NSMenu()

        menu.addItem(NSMenuItem(
            title: "Show Window",
            action: #selector(showMainWindow),
            keyEquivalent: "",
            target: self
        ))

        menu.addItem(NSMenuItem(
            title: "Settings…",
            action: #selector(openSettings),
            keyEquivalent: ",",
            target: self
        ))

        menu.addItem(.separator())

        menu.addItem(NSMenuItem(
            title: "Quit",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q",
            target: NSApp
        ))

        statusItem?.menu = menu
    }

    @objc private func showMainWindow() {
        NSApp.activate(ignoringOtherApps: true)
        mainWindow?.makeKeyAndOrderFront(nil)
    }

    @objc private func openSettings() {
        NSApp.activate(ignoringOtherApps: true)
        if settingsWindow == nil {
            settingsWindow = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 500, height: 400),
                styleMask: [.titled, .closable, .miniaturizable],
                backing: .buffered,
                defer: false
            )
            settingsWindow?.title = "Settings"
            settingsWindow?.center()
        }
        settingsWindow?.makeKeyAndOrderFront(nil)
    }

    // ── Permissions ────────────────────────────────────────────────────

    private func requestPermissions() {
        // Microphone — critical; app does nothing without it
        requestMicrophonePermission()

        // Accessibility — needed for global hotkeys + keystroke paste
        // This pops a system dialog directing user to System Settings
        requestAccessibilityPermission()
    }

    private func requestMicrophonePermission() {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            NSLog("[voice] microphone: authorized")
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { granted in
                NSLog("[voice] microphone: %@", granted ? "granted" : "denied")
            }
        case .denied, .restricted:
            NSLog("[voice] microphone: denied — open System Settings > Privacy > Microphone")
        @unknown default:
            break
        }
    }

    private func requestAccessibilityPermission() {
        let opts = [kAXTrustedCheckOptionPrompt.takeRetainedValue() as String: true]
        let trusted = AXIsProcessTrustedWithOptions(opts as CFDictionary)
        NSLog("[voice] accessibility: %@", trusted ? "trusted" : "not trusted")
    }
}
