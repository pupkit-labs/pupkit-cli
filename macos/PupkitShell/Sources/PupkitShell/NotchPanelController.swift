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
        panel.contentView = NSHostingView(rootView: NotchPanelView(snapshot: nil))
        position(panel: panel)
        self.panel = panel
    }

    func apply(snapshot: UiStateSnapshot) {
        latestSnapshot = snapshot
        if let hostingView = panel?.contentView as? NSHostingView<NotchPanelView> {
            hostingView.rootView = NotchPanelView(snapshot: snapshot)
        }
        if snapshot.top_attention != nil {
            panel?.orderFrontRegardless()
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
}

private struct NotchPanelView: View {
    let snapshot: UiStateSnapshot?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(snapshot?.top_attention?.title ?? "Pupkit")
                .font(.headline)
            Text(snapshot?.top_attention?.message ?? "No pending attention")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            if let options = snapshot?.top_attention?.options, !options.isEmpty {
                HStack {
                    ForEach(options, id: \.self) { option in
                        Text(option)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 4)
                            .background(Color.blue.opacity(0.15))
                            .clipShape(Capsule())
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
