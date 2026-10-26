// Иконка wireproxy в строке меню macOS (аналог tray.ps1 на Windows).
// Сборка: swiftc -O tray.swift -o WireproxyTray
// Запуск: ./WireproxyTray (LaunchAgent — см. com.wireproxy.tray.plist.template)
import AppKit
import Foundation

final class AppDelegate: NSObject, NSApplicationDelegate {
    var statusItem: NSStatusItem!
    var timer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory) // без иконки в Dock

        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = statusItem.button {
            if let img = NSImage(systemSymbolName: "network", accessibilityDescription: "wireproxy") {
                img.isTemplate = true
                button.image = img
            } else {
                button.title = "V"
            }
            button.toolTip = "wireproxy: проверяю…"
        }

        let menu = NSMenu()
        let open = NSMenuItem(title: "Открыть панель управления", action: #selector(openPanel), keyEquivalent: "")
        let refresh = NSMenuItem(title: "Обновить списки доменов", action: #selector(refreshLists), keyEquivalent: "")
        let restart = NSMenuItem(title: "Перезапустить VPN-прокси", action: #selector(restartProxy), keyEquivalent: "")
        let quit = NSMenuItem(title: "Остановить стек и выйти", action: #selector(stopAll), keyEquivalent: "q")
        for item in [open, refresh, restart] {
            item.target = self
            menu.addItem(item)
        }
        menu.addItem(NSMenuItem.separator())
        quit.target = self
        menu.addItem(quit)
        statusItem.menu = menu

        checkStatus()
        timer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
            self?.checkStatus()
        }
    }

    private func post(_ path: String) {
        guard let url = URL(string: "http://127.0.0.1:8080" + path) else { return }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.timeoutInterval = 5
        URLSession.shared.dataTask(with: req) { _, _, _ in }.resume()
    }

    @objc func openPanel() {
        if let url = URL(string: "http://127.0.0.1:8080/") {
            NSWorkspace.shared.open(url)
        }
    }

    @objc func refreshLists() { post("/api/refresh") }
    @objc func restartProxy() { post("/api/restart-proxy") }

    @objc func stopAll() {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = [
            "-c",
            "launchctl unload \"$HOME/Library/LaunchAgents/com.wireproxy.manager.plist\" 2>/dev/null; " +
            "pkill -f 'manager\\.js' 2>/dev/null; " +
            "pkill -f 'wireproxy -c' 2>/dev/null; " +
            "pkill -f 'xray -c' 2>/dev/null; " +
            "launchctl unload \"$HOME/Library/LaunchAgents/com.wireproxy.tray.plist\" 2>/dev/null",
        ]
        try? p.run()
        NSApp.terminate(nil)
    }

    private func checkStatus() {
        guard let url = URL(string: "http://127.0.0.1:8080/api/status") else { return }
        var req = URLRequest(url: url)
        req.timeoutInterval = 4
        URLSession.shared.dataTask(with: req) { [weak self] data, _, _ in
            let ok = (data != nil)
            DispatchQueue.main.async {
                self?.statusItem.button?.toolTip = ok
                    ? "wireproxy: работает · панель 127.0.0.1:8080"
                    : "wireproxy: панель не отвечает"
            }
        }.resume()
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
