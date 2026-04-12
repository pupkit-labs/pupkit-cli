import AppKit
import SwiftUI

// MARK: - KeyablePanel

/// NSPanel subclass that can become the key window when needed (e.g., for TextField input).
private class KeyablePanel: NSPanel {
    var allowsKeyStatus = false

    override var canBecomeKey: Bool { allowsKeyStatus }
}

// MARK: - Island State

enum IslandStatus {
    case closed
    case opened
}

// MARK: - Controller

@MainActor
final class NotchPanelController {
    private var panel: KeyablePanel?
    private var latestSnapshot: UiStateSnapshot?
    private var ipcClient: IPCClient?
    private var onStateUpdate: ((UiStateSnapshot) -> Void)?
    private var islandStatus: IslandStatus = .closed
    private var globalMoveMonitor: Any?
    private var globalClickMonitor: Any?
    private var hoverTimer: DispatchWorkItem?
    private var closeTimer: DispatchWorkItem?
    private var notchRect: NSRect = .zero

    var isVisible: Bool { panel?.isVisible == true }

    func configure(ipcClient: IPCClient, onStateUpdate: @escaping (UiStateSnapshot) -> Void) {
        self.ipcClient = ipcClient
        self.onStateUpdate = onStateUpdate
        let screen = targetScreen()
        let maxSize = panelMaxSize(on: screen)
        let panel = KeyablePanel(
            contentRect: NSRect(x: 0, y: 0, width: maxSize.width, height: maxSize.height),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isFloatingPanel = true
        panel.level = .statusBar
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.isMovable = false
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        panel.titleVisibility = .hidden
        panel.titlebarAppearsTransparent = true

        let hostView = NSHostingView(rootView: IslandContentView(
            snapshot: nil,
            isOpened: false,
            closedNotchWidth: screen.notchSize.width,
            closedNotchHeight: screen.islandClosedHeight,
            onAction: { _ in }
        ))
        hostView.wantsLayer = true
        hostView.layer?.backgroundColor = NSColor.clear.cgColor
        panel.contentView = hostView

        self.panel = panel
        positionPanel(panel, on: screen)
        computeNotchRect(screen: screen)
        panel.orderFrontRegardless()
        panel.ignoresMouseEvents = true
        startEventMonitoring()
    }

    func apply(snapshot: UiStateSnapshot?) {
        let previousAttention = latestSnapshot?.top_attention?.request_id
        latestSnapshot = snapshot
        updateView()

        let hasAttention = snapshot?.top_attention != nil
        let isNewAttention = hasAttention && snapshot?.top_attention?.request_id != previousAttention
        if isNewAttention && islandStatus == .closed {
            openIsland()
        }
    }

    func togglePanel() {
        if islandStatus == .opened {
            closeIsland()
        } else {
            openIsland()
        }
    }

    // MARK: - State transitions

    private func openIsland() {
        cancelTimers()
        islandStatus = .opened
        panel?.ignoresMouseEvents = false
        panel?.acceptsMouseMovedEvents = true

        // Enable key window status if there's a freeform text input
        let needsKey = latestSnapshot?.top_attention?.allow_freeform == true
            && latestSnapshot?.top_attention?.status == .waitingQuestion
        panel?.allowsKeyStatus = needsKey
        if needsKey {
            panel?.makeKeyAndOrderFront(nil)
        }

        updateView()
    }

    private func closeIsland() {
        cancelTimers()
        islandStatus = .closed
        panel?.ignoresMouseEvents = true
        panel?.acceptsMouseMovedEvents = false
        panel?.allowsKeyStatus = false
        updateView()
    }

    // MARK: - View update

    private func updateView() {
        guard let panel else { return }
        let screen = targetScreen()
        let isOpened = islandStatus == .opened

        if let hostView = panel.contentView as? NSHostingView<IslandContentView> {
            hostView.rootView = IslandContentView(
                snapshot: latestSnapshot,
                isOpened: isOpened,
                closedNotchWidth: screen.notchSize.width,
                closedNotchHeight: screen.islandClosedHeight,
                onAction: { [weak self] action in self?.handleAction(action) }
            )
        }

        if !panel.isVisible {
            panel.orderFrontRegardless()
        }
    }

    // MARK: - Positioning

    private func panelMaxSize(on screen: NSScreen) -> CGSize {
        let insetH = IslandMetrics.openedShadowHorizontalInset
        let insetB = IslandMetrics.openedShadowBottomInset
        let openedWidth = IslandMetrics.openedPanelWidth
        let contentHeight: CGFloat = 360
        let width = openedWidth + (insetH * 2) + 28
        let height = screen.islandClosedHeight + contentHeight + insetB
        return CGSize(width: width, height: height)
    }

    private func positionPanel(_ panel: NSPanel, on screen: NSScreen) {
        let size = panel.frame.size
        let x = screen.frame.midX - size.width / 2
        let y = screen.frame.maxY - size.height
        panel.setFrame(NSRect(x: x, y: y, width: size.width, height: size.height), display: true)
    }

    private func computeNotchRect(screen: NSScreen) {
        let ns = screen.notchSize
        let sf = screen.frame
        notchRect = NSRect(
            x: sf.midX - ns.width / 2,
            y: sf.maxY - ns.height,
            width: ns.width,
            height: ns.height
        )
    }

    private func targetScreen() -> NSScreen {
        NSScreen.screens.first(where: { $0.hasNotch }) ?? NSScreen.main ?? NSScreen.screens[0]
    }

    // MARK: - Mouse event monitoring

    private func startEventMonitoring() {
        globalMoveMonitor = NSEvent.addGlobalMonitorForEvents(matching: .mouseMoved) { [weak self] event in
            Task { @MainActor in
                self?.handleMouseMoved(NSEvent.mouseLocation)
            }
        }
        globalClickMonitor = NSEvent.addGlobalMonitorForEvents(matching: .leftMouseDown) { [weak self] event in
            Task { @MainActor in
                self?.handleMouseDown(NSEvent.mouseLocation)
            }
        }
    }

    private func handleMouseMoved(_ loc: NSPoint) {
        let inNotch = isPointInClosedArea(loc)
        if islandStatus == .closed && inNotch {
            scheduleOpen()
        } else if islandStatus == .closed && !inNotch {
            cancelOpenTimer()
        }
        if islandStatus == .opened && !isPointInExpandedArea(loc) {
            scheduleClose()
        } else if islandStatus == .opened && isPointInExpandedArea(loc) {
            cancelCloseTimer()
        }
    }

    private func handleMouseDown(_ loc: NSPoint) {
        if islandStatus == .closed && isPointInClosedArea(loc) {
            cancelTimers()
            openIsland()
        } else if islandStatus == .opened && !isPointInExpandedArea(loc) {
            closeIsland()
        }
    }

    private func isPointInClosedArea(_ pt: NSPoint) -> Bool {
        notchRect.insetBy(dx: -20, dy: -8).contains(pt)
    }

    private func isPointInExpandedArea(_ pt: NSPoint) -> Bool {
        guard let panel else { return false }
        return panel.frame.contains(pt)
    }

    // MARK: - Timers

    private func scheduleOpen() {
        guard hoverTimer == nil else { return }
        let item = DispatchWorkItem { [weak self] in
            self?.hoverTimer = nil
            self?.openIsland()
        }
        hoverTimer = item
        DispatchQueue.main.asyncAfter(deadline: .now() + IslandMetrics.hoverOpenDelay, execute: item)
    }

    private func scheduleClose() {
        guard closeTimer == nil else { return }
        let item = DispatchWorkItem { [weak self] in
            self?.closeTimer = nil
            self?.closeIsland()
        }
        closeTimer = item
        DispatchQueue.main.asyncAfter(deadline: .now() + IslandMetrics.hoverCloseDelay, execute: item)
    }

    private func cancelOpenTimer() {
        hoverTimer?.cancel()
        hoverTimer = nil
    }

    private func cancelCloseTimer() {
        closeTimer?.cancel()
        closeTimer = nil
    }

    private func cancelTimers() {
        cancelOpenTimer()
        cancelCloseTimer()
    }

    // MARK: - IPC action

    private func handleAction(_ action: UiAction) {
        guard let ipcClient else { return }
        Task {
            do {
                let updatedState = try await ipcClient.sendUiAction(action)
                await MainActor.run {
                    self.latestSnapshot = updatedState
                    self.updateView()
                    self.onStateUpdate?(updatedState)
                }
            } catch {
                // Action failed — next poll will refresh state
            }
        }
    }

    deinit {
        if let m = globalMoveMonitor { NSEvent.removeMonitor(m) }
        if let m = globalClickMonitor { NSEvent.removeMonitor(m) }
    }
}

// MARK: - Animations

private let openAnimation = Animation.spring(response: 0.42, dampingFraction: 0.8, blendDuration: 0)
private let closeAnimation = Animation.smooth(duration: 0.3)

// MARK: - Flow Layout (wrapping)

struct FlowLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let containerWidth = proposal.width ?? .infinity
        var x: CGFloat = 0
        var y: CGFloat = 0
        var lineHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x + size.width > containerWidth && x > 0 {
                x = 0
                y += lineHeight + spacing
                lineHeight = 0
            }
            lineHeight = max(lineHeight, size.height)
            x += size.width + spacing
        }
        return CGSize(width: containerWidth, height: y + lineHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var lineHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x + size.width > bounds.maxX && x > bounds.minX {
                x = bounds.minX
                y += lineHeight + spacing
                lineHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), proposal: .unspecified)
            lineHeight = max(lineHeight, size.height)
            x += size.width + spacing
        }
    }
}

// MARK: - Content Height Preference Key

private struct ContentHeightKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = max(value, nextValue())
    }
}

// MARK: - Island Content View

struct IslandContentView: View {
    let snapshot: UiStateSnapshot?
    let isOpened: Bool
    let closedNotchWidth: CGFloat
    let closedNotchHeight: CGFloat
    let onAction: (UiAction) -> Void

    @State private var isHovering = false
    @State private var measuredContentHeight: CGFloat = 80
    @State private var freeformText: String = ""

    private var hasAttention: Bool { snapshot?.top_attention != nil }
    private var hasAnySessions: Bool { (snapshot?.sessions.count ?? 0) > 0 }

    private var notchAnimation: Animation {
        isOpened ? openAnimation : closeAnimation
    }

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .top) {
                Color.clear

                islandSurface(availableSize: geo.size)
                    .frame(maxWidth: .infinity, alignment: .top)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .ignoresSafeArea()
        .preferredColorScheme(.dark)
    }

    @ViewBuilder
    private func islandSurface(availableSize: CGSize) -> some View {
        let insetH = IslandMetrics.openedShadowHorizontalInset
        let insetB = IslandMetrics.openedShadowBottomInset
        let layoutWidth = max(0, availableSize.width - (insetH * 2))
        let layoutHeight = max(0, availableSize.height - insetB)

        let openedWidth = min(IslandMetrics.openedPanelWidth, layoutWidth - 28)
        // Dynamic height: header + measured content + padding, capped to available space
        let dynamicOpenedHeight = closedNotchHeight + measuredContentHeight + 20
        let openedHeight = min(dynamicOpenedHeight, layoutHeight - 14)

        let closedWidth = closedNotchWidth
        let closedHeight = closedNotchHeight

        let currentWidth = isOpened ? openedWidth : closedWidth
        let currentHeight = isOpened ? openedHeight : closedHeight

        let shape = NotchShape(
            topCornerRadius: isOpened ? NotchShape.openedTopRadius : NotchShape.closedTopRadius,
            bottomCornerRadius: isOpened ? NotchShape.openedBottomRadius : NotchShape.closedBottomRadius
        )

        VStack(spacing: 0) {
            ZStack(alignment: .top) {
                // Background shape
                shape
                    .fill(Color.black)
                    .frame(width: currentWidth, height: currentHeight)

                VStack(spacing: 0) {
                    // Header row (always in notch height)
                    headerRow
                        .frame(height: closedNotchHeight)

                    // Expandable content
                    if isOpened {
                        openedContent
                            .frame(width: openedWidth - 32)
                            .background(
                                GeometryReader { geo in
                                    Color.clear.preference(key: ContentHeightKey.self, value: geo.size.height)
                                }
                            )
                            .onPreferenceChange(ContentHeightKey.self) { height in
                                measuredContentHeight = max(height, 40)
                            }
                            .transition(.opacity.combined(with: .move(edge: .top)))
                    }
                }
                .frame(width: currentWidth, height: currentHeight, alignment: .top)
                .clipShape(shape)

                // Top-edge strip for notch blend
                Rectangle()
                    .fill(Color.black)
                    .frame(height: 1)
                    .padding(.horizontal, isOpened ? NotchShape.openedTopRadius : NotchShape.closedTopRadius)

                // Border stroke
                shape
                    .stroke(Color.white.opacity(isOpened ? 0.07 : 0.04), lineWidth: 1)
                    .frame(width: currentWidth, height: currentHeight)
            }
            .frame(width: currentWidth, height: currentHeight, alignment: .top)
        }
        .scaleEffect(isOpened ? 1 : (isHovering ? IslandMetrics.closedHoverScale : 1), anchor: .top)
        .padding(.horizontal, insetH)
        .padding(.bottom, insetB)
        .animation(notchAnimation, value: isOpened)
        .animation(.spring(response: 0.38, dampingFraction: 0.8), value: isHovering)
        .onHover { hovering in
            isHovering = hovering
        }
    }

    // MARK: - Header Row

    @ViewBuilder
    private var headerRow: some View {
        if isOpened {
            // Empty header — content starts directly below notch
            Color.clear
        } else {
            HStack(spacing: 0) {
                if hasAnySessions || hasAttention {
                    HStack(spacing: 4) {
                        if hasAttention {
                            Circle()
                                .fill(Color.orange)
                                .frame(width: 6, height: 6)
                        } else {
                            Circle()
                                .fill(Color.green)
                                .frame(width: 6, height: 6)
                        }
                    }
                    .frame(width: 20)
                }

                Spacer()

                if hasAnySessions {
                    Text("\(snapshot?.sessions.count ?? 0)")
                        .font(.system(size: 11, weight: .medium, design: .rounded))
                        .foregroundStyle(.white.opacity(0.7))
                        .frame(width: 28)
                }
            }
            .padding(.horizontal, 8)
        }
    }

    // MARK: - Opened Content

    @ViewBuilder
    private var openedContent: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let attention = snapshot?.top_attention {
                VStack(alignment: .leading, spacing: 6) {
                    Text(attention.title)
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(.white)
                    Text(attention.message)
                        .font(.system(size: 12))
                        .foregroundStyle(.white.opacity(0.7))
                        .lineLimit(3)
                }

                actionButtons(for: attention)
            } else if hasAnySessions {
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(snapshot?.sessions.count ?? 0) active session(s)")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(.white)
                    Text("No pending actions")
                        .font(.system(size: 12))
                        .foregroundStyle(.white.opacity(0.5))
                }
            } else {
                Text("No active sessions")
                    .font(.system(size: 12))
                    .foregroundStyle(.white.opacity(0.4))
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
    }

    // MARK: - Action Buttons

    @ViewBuilder
    private func actionButtons(for attention: AttentionCard) -> some View {
        switch attention.status {
        case .waitingApproval:
            HStack(spacing: 8) {
                Button {
                    onAction(.approve(requestId: attention.request_id, always: false))
                } label: {
                    Text("Allow")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 6)
                        .background(Color.green, in: Capsule())
                }
                .buttonStyle(.plain)

                Button {
                    onAction(.approve(requestId: attention.request_id, always: true))
                } label: {
                    Text("Always Allow")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 6)
                        .background(Color.green.opacity(0.6), in: Capsule())
                }
                .buttonStyle(.plain)

                Button {
                    onAction(.deny(requestId: attention.request_id))
                } label: {
                    Text("Deny")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 6)
                        .background(Color.red, in: Capsule())
                }
                .buttonStyle(.plain)
            }

        case .waitingQuestion:
            VStack(spacing: 8) {
                FlowLayout(spacing: 6) {
                    ForEach(attention.options, id: \.self) { option in
                        Button {
                            onAction(.answerOption(requestId: attention.request_id, optionId: option))
                        } label: {
                            Text(option)
                                .font(.system(size: 12, weight: .medium))
                                .foregroundStyle(.white)
                                .lineLimit(2)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 5)
                                .background(Color.blue, in: Capsule())
                        }
                        .buttonStyle(.plain)
                    }
                }

                if attention.allow_freeform {
                    HStack(spacing: 6) {
                        TextField("Type a response…", text: $freeformText)
                            .textFieldStyle(.plain)
                            .font(.system(size: 12))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 6)
                            .background(Color.white.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
                            .onSubmit {
                                guard !freeformText.isEmpty else { return }
                                onAction(.answerText(requestId: attention.request_id, text: freeformText))
                                freeformText = ""
                            }

                        Button {
                            guard !freeformText.isEmpty else { return }
                            onAction(.answerText(requestId: attention.request_id, text: freeformText))
                            freeformText = ""
                        } label: {
                            Image(systemName: "paperplane.fill")
                                .font(.system(size: 12))
                                .foregroundStyle(.white)
                                .padding(6)
                                .background(
                                    freeformText.isEmpty ? Color.gray.opacity(0.4) : Color.green,
                                    in: Circle()
                                )
                        }
                        .buttonStyle(.plain)
                        .disabled(freeformText.isEmpty)
                    }
                }
            }

        default:
            EmptyView()
        }
    }
}
