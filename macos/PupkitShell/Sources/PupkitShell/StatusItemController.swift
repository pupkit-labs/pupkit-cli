import AppKit

@MainActor
final class StatusItemController: NSObject {
    private var statusItem: NSStatusItem?
    private let menu = NSMenu()
    private weak var notchController: NotchPanelController?
    private var latestSnapshot: UiStateSnapshot?

    func start(ipcClient: IPCClient, notchController: NotchPanelController) {
        self.notchController = notchController
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.button?.title = "🐶 Pupkit"
        item.button?.toolTip = "Pupkit Shell"
        item.menu = menu
        statusItem = item
        rebuildMenu(errorMessage: nil)
    }

    func apply(snapshot: UiStateSnapshot) {
        latestSnapshot = snapshot
        let attentionCount = snapshot.top_attention == nil ? 0 : 1
        statusItem?.button?.title = attentionCount > 0 ? "🐶(\(attentionCount))" : "🐶"
        rebuildMenu(errorMessage: nil)
    }

    func apply(error: String) {
        rebuildMenu(errorMessage: error)
    }

    private func rebuildMenu(errorMessage: String?) {
        menu.removeAllItems()

        if let errorMessage {
            let item = NSMenuItem(title: "Error: \(errorMessage)", action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
            menu.addItem(.separator())
        }

        if let attention = latestSnapshot?.top_attention {
            let attentionItem = NSMenuItem(title: "Needs attention: \(attention.title)", action: nil, keyEquivalent: "")
            attentionItem.isEnabled = false
            menu.addItem(attentionItem)
            menu.addItem(NSMenuItem(title: attention.message, action: nil, keyEquivalent: ""))
            menu.addItem(.separator())
        }

        for session in latestSnapshot?.sessions.prefix(8) ?? [] {
            let item = NSMenuItem(title: "\(session.title) · \(session.status.rawValue)", action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
        }

        if latestSnapshot?.sessions.isEmpty ?? true {
            let empty = NSMenuItem(title: "No active sessions", action: nil, keyEquivalent: "")
            empty.isEnabled = false
            menu.addItem(empty)
        }

        menu.addItem(.separator())
        let openNotch = NSMenuItem(title: "Toggle Notch Panel", action: #selector(toggleNotchPanel), keyEquivalent: "")
        openNotch.target = self
        menu.addItem(openNotch)

        let quit = NSMenuItem(title: "Quit", action: #selector(quit), keyEquivalent: "")
        quit.target = self
        menu.addItem(quit)
    }

    @objc private func toggleNotchPanel() {
        notchController?.togglePanel()
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}
