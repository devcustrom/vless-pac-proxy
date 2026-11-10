// Иконка wireproxy в строке меню macOS (аналог tray.ps1 на Windows).
// Меню: открыть панель (panel.localhost, если установлены сертификаты),
// установка локальных сертификатов, обновление списков, рестарт, выход.
// Состояние готовности локальных доменов обновляется в фоне (таймер + при
// открытии меню), меню строится по кэшу и не подвисает.
import AppKit
import Foundation

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    var statusItem: NSStatusItem!
    var timer: Timer?
    let baseDir = FileManager.default.currentDirectoryPath
    var ready = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = statusItem.button {
            if let img = NSImage(systemSymbolName: "network", accessibilityDescription: "wireproxy") {
                img.isTemplate = true
                button.image = img
            } else {
                button.title = "V"
            }
            button.toolTip = "wireproxy"
        }
        let menu = NSMenu()
        menu.delegate = self
        statusItem.menu = menu
        checkStatus()
        refreshState()
        timer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in
            self?.checkStatus()
            self?.refreshState()
        }
    }

    // MARK: - меню
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        refreshState()
        let title = ready
            ? "Открыть панель (panel.localhost)"
            : "Открыть панель (127.0.0.1:8080)"
        menu.addItem(item(title, #selector(openPanel)))
        if !ready {
            menu.addItem(item("Добавить локальные сертификаты…", #selector(installCerts)))
        }
        menu.addItem(item("Обновить списки доменов", #selector(refreshLists)))
        menu.addItem(item("Перезапустить VPN-прокси", #selector(restartProxy)))
        menu.addItem(.separator())
        menu.addItem(item("Остановить стек и выйти", #selector(stopAll)))
    }

    private func item(_ title: String, _ action: Selector) -> NSMenuItem {
        let mi = NSMenuItem(title: title, action: action, keyEquivalent: "")
        mi.target = self
        return mi
    }

    // MARK: - действия
    @objc func openPanel() {
        let url = ready ? "https://panel.localhost/" : "http://127.0.0.1:8080/"
        if let u = URL(string: url) {
            NSWorkspace.shared.open(u)
        }
    }

    @objc func installCerts() {
        let ca = baseDir + "/certs/ca.crt"
        guard FileManager.default.fileExists(atPath: ca) else {
            notify("Сначала запустите стек — CA ещё не создан")
            return
        }
        let quoted = ca.replacingOccurrences(of: "'", with: "'\\''")
        let script = "do shell script \"/usr/bin/security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain '"
            + quoted + "'\" with administrator privileges"
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        p.arguments = ["-e", script]
        p.terminationHandler = { [weak self] proc in
            DispatchQueue.main.async {
                if proc.terminationStatus == 0 {
                    self?.notify("Сертификат CA добавлен")
                    self?.refreshState()
                } else {
                    self?.notify("Не удалось добавить сертификат")
                }
            }
        }
        try? p.run()
    }

    @objc func refreshLists() {
        post("/api/refresh", "Обновление списка запущено")
    }

    @objc func restartProxy() {
        post("/api/restart-proxy", "Перезапуск VPN-прокси")
    }

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

    // MARK: - проверки
    private func caTrusted() -> Bool {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/security")
        p.arguments = ["find-certificate", "-c", "wireproxy-control local CA", "/Library/Keychains/System.keychain"]
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        do { try p.run() } catch { return false }
        p.waitUntilExit()
        return p.terminationStatus == 0
    }

    private func panelDomainMapped() -> Bool {
        guard let url = URL(string: "http://127.0.0.1:8080/api/local-domains") else { return false }
        var req = URLRequest(url: url)
        req.timeoutInterval = 2
        let sem = DispatchSemaphore(value: 0)
        var mapped = false
        URLSession.shared.dataTask(with: req) { data, _, _ in
            if let data = data,
               let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let domains = obj["domains"] as? [[String: Any]] {
                for d in domains {
                    if let name = d["domain"] as? String, name == "panel.localhost",
                       let port = d["port"] as? Int, port == 8080 {
                        mapped = true
                    }
                }
            }
            sem.signal()
        }.resume()
        _ = sem.wait(timeout: .now() + 3)
        return mapped
    }

    private func refreshState() {
        DispatchQueue.global().async { [weak self] in
            guard let self = self else { return }
            let ok = self.caTrusted() && self.panelDomainMapped()
            DispatchQueue.main.async { self.ready = ok }
        }
    }

    private func post(_ path: String, _ message: String) {
        guard let url = URL(string: "http://127.0.0.1:8080" + path) else { return }
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.timeoutInterval = 5
        req.setValue("1", forHTTPHeaderField: "X-WPC")
        URLSession.shared.dataTask(with: req) { [weak self] _, _, _ in
            DispatchQueue.main.async { self?.notify(message) }
        }.resume()
    }

    private func notify(_ text: String) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        p.arguments = ["-e", "display notification " + appleQuote(text) + " with title \"wireproxy\""]
        try? p.run()
    }

    private func appleQuote(_ s: String) -> String {
        let esc = s
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        return "\"" + esc + "\""
    }

    private func checkStatus() {
        guard let url = URL(string: "http://127.0.0.1:8080/api/status") else { return }
        var req = URLRequest(url: url)
        req.timeoutInterval = 4
        URLSession.shared.dataTask(with: req) { [weak self] data, _, _ in
            let ok = (data != nil)
            DispatchQueue.main.async {
                self?.statusItem.button?.toolTip = ok
                    ? "wireproxy: работает · панель panel.localhost"
                    : "wireproxy: панель не отвечает"
            }
        }.resume()
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
