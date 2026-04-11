import AppKit
import SwiftUI

@MainActor
final class NotchPanelController {
    private var panel: NSPanel?
    private var latestSnapshot: UiStateSnapshot?
    private var ipcClient: IPCClient?
    private var onStateUpdate: ((UiStateSnapshot) -> Void)?

    func configure(ipcClient: IPCClient, onStateUpdate: @escaping (UiStateSnapshot) -> Void) {
        self.ipcClient = ipcClient
        self.onStateUpdate = onStateUpdate
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
        panel.contentView = NSHostingView(rootView: NotchPanelView(snapshot: nil, onAction: { _ in }))
        position(panel: panel)
        self.panel = panel
    }

    func apply(snapshot: UiStateSnapshot?) {
        latestSnapshot = snapshot
        if let hostingView = panel?.contentView as? NSHostingView<NotchPanelView> {
            hostingView.rootView = NotchPanelView(snapshot: snapshot, onAction: { [weak self] action in
                self?.handleAction(action)
            })
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

    private func position(panel: NSPanel) {
        guard let screen = NSScreen.main else { return }
        let frame = screen.visibleFrame
        let x = frame.midX - panel.frame.width / 2
        let y = frame.maxY - panel.frame.height - 6
        panel.setFrameOrigin(NSPoint(x: x, y: y))
    }

    private func handleAction(_ action: UiAction) {
        guard let ipcClient else { return }
        Task {
            do {
                let updatedState = try await ipcClient.sendUiAction(action)
                await MainActor.run {
                    self.apply(snapshot: updatedState)
                    self.onStateUpdate?(updatedState)
                }
            } catch {
                // Action failed — next poll will refresh state
            }
        }
    }
}

private struct NotchPanelView: View {
    let snapshot: UiStateSnapshot?
    let onAction: (UiAction) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(snapshot?.top_attention?.title ?? "Pupkit")
                .font(.headline)
            Text(snapshot?.top_attention?.message ?? "No pending attention")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            if let attention = snapshot?.top_attention {
                actionButtons(for: attention)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .background(.ultraThinMaterial)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .padding(8)
    }

    @ViewBuilder
    private func actionButtons(for attention: AttentionCard) -> some View {
        switch attention.status {
        case .waitingApproval:
            HStack(spacing: 8) {
                Button {
                    onAction(.approve(requestId: attention.request_id, always: false))
                } label: {
                    Text("Allow")
                        .foregroundStyle(.white)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 5)
                        .background(Color.green, in: Capsule())
                }
                .buttonStyle(.plain)

                Button {
                    onAction(.approve(requestId: attention.request_id, always: true))
                } label: {
                    Text("Always Allow")
                        .foregroundStyle(.white)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 5)
                        .background(Color.green.opacity(0.6), in: Capsule())
                }
                .buttonStyle(.plain)

                Button {
                    onAction(.deny(requestId: attention.request_id))
                } label: {
                    Text("Deny")
                        .foregroundStyle(.white)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 5)
                        .background(Color.red, in: Capsule())
                }
                .buttonStyle(.plain)
            }

        case .waitingQuestion:
            HStack(spacing: 6) {
                ForEach(attention.options, id: \.self) { option in
                    Button {
                        onAction(.answerOption(requestId: attention.request_id, optionId: option))
                    } label: {
                        Text(option)
                            .foregroundStyle(.white)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 4)
                            .background(Color.blue, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
            }

        default:
            EmptyView()
        }
    }
}
