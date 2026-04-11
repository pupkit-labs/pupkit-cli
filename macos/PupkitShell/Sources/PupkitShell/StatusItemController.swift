import AppKit

@MainActor
final class StatusItemController: NSObject {
    private var statusItem: NSStatusItem?
    private let menu = NSMenu()
    private weak var notchController: NotchPanelController?
    private var latestSnapshot: UiStateSnapshot?
    private var ipcClient: IPCClient?
    private var errorMessage: String?

    func start(ipcClient: IPCClient, notchController: NotchPanelController) {
        self.ipcClient = ipcClient
        self.notchController = notchController
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.button?.title = "🐶 Pupkit"
        item.button?.toolTip = "Pupkit Shell"
        item.menu = menu
        statusItem = item
        rebuildMenu()
    }

    func apply(snapshot: UiStateSnapshot) {
        latestSnapshot = snapshot
        errorMessage = nil
        let attentionCount = snapshot.top_attention == nil ? 0 : 1
        statusItem?.button?.title = attentionCount > 0 ? "🐶(\(attentionCount))" : "🐶"
        rebuildMenu()
    }

    func apply(error: String) {
        errorMessage = error
        rebuildMenu()
    }

    private func rebuildMenu() {
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
            let messageItem = NSMenuItem(title: attention.message, action: nil, keyEquivalent: "")
            messageItem.isEnabled = false
            menu.addItem(messageItem)

            switch attention.status {
            case .waitingApproval:
                let approve = NSMenuItem(title: "Approve", action: #selector(approveTopAttention), keyEquivalent: "")
                approve.target = self
                approve.representedObject = attention.request_id
                menu.addItem(approve)

                let deny = NSMenuItem(title: "Deny", action: #selector(denyTopAttention), keyEquivalent: "")
                deny.target = self
                deny.representedObject = attention.request_id
                menu.addItem(deny)
            case .waitingQuestion:
                for option in attention.options {
                    let optionItem = NSMenuItem(title: option, action: #selector(answerTopAttentionOption(_:)), keyEquivalent: "")
                    optionItem.target = self
                    optionItem.representedObject = [attention.request_id, option]
                    menu.addItem(optionItem)
                }
            default:
                break
            }
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

    @objc private func approveTopAttention(_ sender: NSMenuItem) {
        guard let requestId = sender.representedObject as? String else { return }
        Task { @MainActor in
            guard let ipcClient else { return }
            do {
                let snapshot = try await ipcClient.approve(requestId: requestId)
                apply(snapshot: snapshot)
                notchController?.apply(snapshot: snapshot)
            } catch {
                apply(error: error.localizedDescription)
            }
        }
    }

    @objc private func denyTopAttention(_ sender: NSMenuItem) {
        guard let requestId = sender.representedObject as? String else { return }
        Task { @MainActor in
            guard let ipcClient else { return }
            do {
                let snapshot = try await ipcClient.deny(requestId: requestId)
                apply(snapshot: snapshot)
                notchController?.apply(snapshot: snapshot)
            } catch {
                apply(error: error.localizedDescription)
            }
        }
    }

    @objc private func answerTopAttentionOption(_ sender: NSMenuItem) {
        guard let payload = sender.representedObject as? [String], payload.count == 2 else { return }
        Task { @MainActor in
            guard let ipcClient else { return }
            do {
                let snapshot = try await ipcClient.answerOption(requestId: payload[0], optionId: payload[1])
                apply(snapshot: snapshot)
                notchController?.apply(snapshot: snapshot)
            } catch {
                apply(error: error.localizedDescription)
            }
        }
    }

    @objc private func toggleNotchPanel() {
        notchController?.togglePanel()
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}
