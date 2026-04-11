import AppKit
import SwiftUI

@MainActor
final class NotchPanelController {
    private var panel: NSPanel?
    private var latestSnapshot: UiStateSnapshot?
    private var ipcClient: IPCClient?

    func configure(ipcClient: IPCClient) {
        self.ipcClient = ipcClient
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 120),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.level = .statusBar
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        panel.contentView = NSHostingView(rootView: NotchPanelView(snapshot: nil, onApprove: nil, onDeny: nil, onAnswer: nil))
        position(panel: panel)
        self.panel = panel
    }

    func apply(snapshot: UiStateSnapshot?) {
        latestSnapshot = snapshot
        if let hostingView = panel?.contentView as? NSHostingView<NotchPanelView> {
            hostingView.rootView = NotchPanelView(
                snapshot: snapshot,
                onApprove: { [weak self] requestId in await self?.approve(requestId: requestId) },
                onDeny: { [weak self] requestId in await self?.deny(requestId: requestId) },
                onAnswer: { [weak self] requestId, option in await self?.answer(requestId: requestId, option: option) }
            )
        }
        if snapshot?.top_attention != nil {
            panel?.orderFrontRegardless()
        } else {
            panel?.orderOut(nil)
        }
    }

    func togglePanel() {
        guard let panel else { return }
        if panel.isVisible {
            panel.orderOut(nil)
        } else {
            position(panel: panel)
            panel.orderFrontRegardless()
        }
    }

    private func approve(requestId: String) async {
        guard let ipcClient else { return }
        do {
            let snapshot = try await ipcClient.approve(requestId: requestId)
            apply(snapshot: snapshot)
        } catch {
            apply(snapshot: nil)
        }
    }

    private func deny(requestId: String) async {
        guard let ipcClient else { return }
        do {
            let snapshot = try await ipcClient.deny(requestId: requestId)
            apply(snapshot: snapshot)
        } catch {
            apply(snapshot: nil)
        }
    }

    private func answer(requestId: String, option: String) async {
        guard let ipcClient else { return }
        do {
            let snapshot = try await ipcClient.answerOption(requestId: requestId, optionId: option)
            apply(snapshot: snapshot)
        } catch {
            apply(snapshot: nil)
        }
    }

    private func position(panel: NSPanel) {
        guard let screen = NSScreen.main else { return }
        let frame = screen.visibleFrame
        let x = frame.midX - panel.frame.width / 2
        let y = frame.maxY - panel.frame.height - 6
        panel.setFrameOrigin(NSPoint(x: x, y: y))
    }
}

private struct NotchPanelView: View {
    let snapshot: UiStateSnapshot?
    let onApprove: ((String) async -> Void)?
    let onDeny: ((String) async -> Void)?
    let onAnswer: ((String, String) async -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(snapshot?.top_attention?.title ?? "Pupkit")
                .font(.headline)
            Text(snapshot?.top_attention?.message ?? "No pending attention")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            if let attention = snapshot?.top_attention {
                switch attention.status {
                case .waitingApproval:
                    HStack {
                        Button("Approve") {
                            Task { await onApprove?(attention.request_id) }
                        }
                        Button("Deny") {
                            Task { await onDeny?(attention.request_id) }
                        }
                    }
                case .waitingQuestion:
                    HStack {
                        ForEach(attention.options, id: \.self) { option in
                            Button(option) {
                                Task { await onAnswer?(attention.request_id, option) }
                            }
                        }
                    }
                default:
                    if !attention.options.isEmpty {
                        HStack {
                            ForEach(attention.options, id: \.self) { option in
                                Text(option)
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 4)
                                    .background(Color.blue.opacity(0.15))
                                    .clipShape(Capsule())
                            }
                        }
                    }
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .background(.ultraThinMaterial)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .padding(8)
    }
}
