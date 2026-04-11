import AppKit

@MainActor
final class StatusItemController: NSObject {
    private var statusItem: NSStatusItem?
    private let menu = NSMenu()
    private weak var notchController: NotchPanelController?
    private var ipcClient: IPCClient?
    private var latestSnapshot: UiStateSnapshot?

    func start(ipcClient: IPCClient, notchController: NotchPanelController) {
        self.notchController = notchController
        self.ipcClient = ipcClient
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
            let attentionItem = NSMenuItem(title: "⚠ \(attention.title)", action: nil, keyEquivalent: "")
            attentionItem.isEnabled = false
            menu.addItem(attentionItem)
            menu.addItem(NSMenuItem(title: "  \(attention.message)", action: nil, keyEquivalent: ""))

            if attention.status == .waitingApproval {
                let approveItem = NSMenuItem(title: "  ✅ Allow", action: #selector(approveAttention), keyEquivalent: "")
                approveItem.target = self
                menu.addItem(approveItem)
                let denyItem = NSMenuItem(title: "  ❌ Deny", action: #selector(denyAttention), keyEquivalent: "")
                denyItem.target = self
                menu.addItem(denyItem)
            } else if attention.status == .waitingQuestion {
                for option in attention.options {
                    let optItem = NSMenuItem(title: "  → \(option)", action: #selector(answerOption(_:)), keyEquivalent: "")
                    optItem.target = self
                    optItem.representedObject = AnswerContext(requestId: attention.request_id, optionId: option)
                    menu.addItem(optItem)
                }
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

    @objc private func approveAttention() {
        guard let requestId = latestSnapshot?.top_attention?.request_id else { return }
        sendAction(.approve(requestId: requestId, always: false))
    }

    @objc private func denyAttention() {
        guard let requestId = latestSnapshot?.top_attention?.request_id else { return }
        sendAction(.deny(requestId: requestId))
    }

    @objc private func answerOption(_ sender: NSMenuItem) {
        guard let ctx = sender.representedObject as? AnswerContext else { return }
        sendAction(.answerOption(requestId: ctx.requestId, optionId: ctx.optionId))
    }

    private func sendAction(_ action: UiAction) {
        guard let ipcClient else { return }
        Task {
            do {
                let updatedState = try await ipcClient.sendUiAction(action)
                await MainActor.run {
                    self.apply(snapshot: updatedState)
                    self.notchController?.apply(snapshot: updatedState)
                }
            } catch {
                // Action failed — next poll will refresh
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

private class AnswerContext: NSObject {
    let requestId: String
    let optionId: String
    init(requestId: String, optionId: String) {
        self.requestId = requestId
        self.optionId = optionId
    }
}
