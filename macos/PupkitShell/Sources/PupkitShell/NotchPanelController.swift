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
    /// When true, the island was opened programmatically (new attention arrived)
    /// and should stay open for at least `programmaticMinDisplayTime` seconds.
    private var programmaticOpen = false
    private var programmaticOpenTime: Date?
    /// When a new attention arrives, this holds the source to auto-switch to.
    private var suggestedTab: String?
    private var globalMoveMonitor: Any?
    private var globalClickMonitor: Any?
    private var localMoveMonitor: Any?
    private var hoverTimer: DispatchWorkItem?
    private var closeTimer: DispatchWorkItem?
    private var notchRect: NSRect = .zero
    /// When false, the island won't auto-open on new attention events.
    private var autoExpandEnabled = true

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
            suggestedTab: nil,
            autoExpandEnabled: true,
            onAction: { _ in },
            onToggleAutoExpand: {}
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
        let previousAttentionIds = Set(latestSnapshot?.attentions.map(\.request_id) ?? [])
        latestSnapshot = snapshot
        updateView()

        let currentAttentions = snapshot?.attentions ?? []
        let currentAttentionIds = Set(currentAttentions.map(\.request_id))
        let newIds = currentAttentionIds.subtracting(previousAttentionIds)
        let hasNew = !newIds.isEmpty
        if hasNew && islandStatus == .closed && autoExpandEnabled {
            programmaticOpen = true
            programmaticOpenTime = Date()
            openIsland()
        }

        // When new attention arrives, switch to ALL tab to show everything
        if hasNew {
            suggestedTab = "ALL"
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

        // Enable key window status if any attention has freeform text input
        let needsKey = latestSnapshot?.attentions.contains(where: {
            $0.allow_freeform && $0.status == .waitingQuestion
        }) ?? false
        panel?.allowsKeyStatus = needsKey
        if needsKey {
            panel?.makeKeyAndOrderFront(nil)
            // Ensure the panel stays focusable after view updates
            DispatchQueue.main.async { [weak panel] in
                panel?.makeKeyAndOrderFront(nil)
            }
        }

        updateView()
    }

    private func closeIsland() {
        cancelTimers()
        islandStatus = .closed
        programmaticOpen = false
        programmaticOpenTime = nil
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

        // Consume the suggestedTab once per update
        let tabHint = suggestedTab
        suggestedTab = nil

        if let hostView = panel.contentView as? NSHostingView<IslandContentView> {
            hostView.rootView = IslandContentView(
                snapshot: latestSnapshot,
                isOpened: isOpened,
                closedNotchWidth: screen.notchSize.width,
                closedNotchHeight: screen.islandClosedHeight,
                suggestedTab: tabHint,
                autoExpandEnabled: autoExpandEnabled,
                onAction: { [weak self] action in self?.handleAction(action) },
                onToggleAutoExpand: { [weak self] in
                    self?.autoExpandEnabled.toggle()
                    self?.updateView()
                }
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
        let contentHeight: CGFloat = 720
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
        // Prefer built-in display (has the physical notch on MacBooks)
        if let builtIn = NSScreen.screens.first(where: { $0.isBuiltIn && $0.hasNotch }) {
            return builtIn
        }
        return NSScreen.screens.first(where: { $0.hasNotch }) ?? NSScreen.main ?? NSScreen.screens[0]
    }

    // MARK: - Mouse event monitoring

    private func startEventMonitoring() {
        globalMoveMonitor = NSEvent.addGlobalMonitorForEvents(matching: .mouseMoved) { [weak self] event in
            Task { @MainActor in
                self?.handleMouseMoved(NSEvent.mouseLocation)
            }
        }
        // Local monitor covers mouse moves INSIDE the panel (global monitor only
        // fires for events delivered to OTHER apps). Without this, mouse leaving
        // the visible island but still inside the panel frame goes undetected.
        localMoveMonitor = NSEvent.addLocalMonitorForEvents(matching: .mouseMoved) { [weak self] event in
            Task { @MainActor in
                self?.handleMouseMoved(NSEvent.mouseLocation)
            }
            return event
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
            // If opened programmatically, enforce minimum display time (5s)
            // so user can switch desktops back to interact
            if programmaticOpen, let openTime = programmaticOpenTime {
                let elapsed = Date().timeIntervalSince(openTime)
                if elapsed < 3.0 {
                    return  // don't schedule close yet
                }
            }
            scheduleClose()
        } else if islandStatus == .opened && isPointInExpandedArea(loc) {
            // User moved mouse into panel — no longer a programmatic-only open
            programmaticOpen = false
            cancelCloseTimer()
        }
    }

    private func handleMouseDown(_ loc: NSPoint) {
        if islandStatus == .closed && isPointInClosedArea(loc) {
            cancelTimers()
            programmaticOpen = false
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
        let frame = panel.frame
        // Inset by shadow/padding so detection matches the visible island, not the full panel
        let insetH = IslandMetrics.openedShadowHorizontalInset + 14
        let insetB = IslandMetrics.openedShadowBottomInset
        let tolerance: CGFloat = 8
        let visualRect = NSRect(
            x: frame.minX + insetH - tolerance,
            y: frame.minY + insetB - tolerance,
            width: frame.width - (insetH - tolerance) * 2,
            height: frame.height - insetB + tolerance
        )
        return visualRect.contains(pt)
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

// MARK: - Island Content View

struct IslandContentView: View {
    let snapshot: UiStateSnapshot?
    let isOpened: Bool
    let closedNotchWidth: CGFloat
    let closedNotchHeight: CGFloat
    /// Hint from controller to auto-switch tab when new attention arrives.
    let suggestedTab: String?
    let autoExpandEnabled: Bool
    let onAction: (UiAction) -> Void
    let onToggleAutoExpand: () -> Void

    @State private var isHovering = false
    @State private var freeformTexts: [String: String] = [:]
    @State private var selectedTab: String? = nil  // nil = "All"
    @State private var isPulsing = false
    @State private var ripple1 = false
    @State private var ripple2 = false
    @State private var ripple3 = false
    @FocusState private var focusedField: String?

    private var hasAttention: Bool { !(snapshot?.attentions.isEmpty ?? true) }
    private var hasAnySessions: Bool { (snapshot?.sessions.count ?? 0) > 0 }

    /// Compute opened content height based on what's currently visible
    private var estimatedContentHeight: CGFloat {
        let attentions = filteredAttentions
        if !attentions.isEmpty {
            let tabBarHeight: CGFloat = 36
            var cardsHeight: CGFloat = 0
            for att in attentions {
                cardsHeight += estimatedCardHeight(for: att)
            }
            let spacing = CGFloat(max(0, attentions.count - 1)) * 12
            let scrollContent = min(cardsHeight + spacing, 480)
            let clearAllHeight: CGFloat = 22  // Clear all button row
            // 22 = top padding(8) + VStack spacing(8) + bottom padding(6)
            return tabBarHeight + clearAllHeight + scrollContent + 22
        } else {
            // Session info or empty state
            let activeSources = Set(snapshot?.sessions.map(\.source) ?? [])
            let hasTabBar = activeSources.count > 1
            let tabBarHeight: CGFloat = hasTabBar ? 44 : 0  // 36 + 8 VStack spacing
            let sessionInfoHeight: CGFloat = 56  // text lines + bottom padding(16)
            // top padding(8) + tabBar + sessionInfo + bottom padding(6)
            return 14 + tabBarHeight + sessionInfoHeight
        }
    }

    /// Dynamically compute card height based on actual content
    private func estimatedCardHeight(for att: AttentionCard) -> CGFloat {
        let cardPadding: CGFloat = 24  // .padding(12) top + bottom
        let vSpacing: CGFloat = 8      // VStack spacing
        let titleHeight: CGFloat = 22
        // Message: estimate lines from char count, capped at lineLimit(3)
        let charsPerLine: CGFloat = 48
        let msgLines = min(3, max(1, ceil(CGFloat(att.message.count) / charsPerLine)))
        let messageHeight = msgLines * 20

        var inner = titleHeight + vSpacing + messageHeight + vSpacing

        switch att.status {
        case .waitingApproval:
            inner += 42   // HStack of 3 buttons
        case .waitingQuestion:
            // FlowLayout: estimate rows from option label widths
            let availW: CGFloat = 440
            var rowW: CGFloat = 0
            var rows: CGFloat = 1
            for opt in att.options {
                let btnW = CGFloat(opt.count) * 9 + 34  // text + padding
                if rowW + btnW + 8 > availW && rowW > 0 {
                    rows += 1
                    rowW = btnW
                } else {
                    rowW += (rowW > 0 ? 8 : 0) + btnW
                }
            }
            inner += rows * 42 + max(0, rows - 1) * 8

            if att.allow_freeform {
                inner += vSpacing + 42  // input row
            }
        default:
            break
        }
        return inner + cardPadding
    }

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
        let dynamicOpenedHeight = closedNotchHeight + estimatedContentHeight + 4
        let openedHeight = min(dynamicOpenedHeight, layoutHeight - 4)

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
                            .frame(width: openedWidth - 36)
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
        .overlay(alignment: .top) {
            if isOpened {
                usageStrip(notchWidth: closedNotchWidth, totalWidth: layoutWidth)
                    .frame(height: closedNotchHeight)
            } else {
                closedFlankingIndicators
            }
        }
        .scaleEffect(isOpened ? 1 : (isHovering ? IslandMetrics.closedHoverScale : 1), anchor: .top)
        .padding(.horizontal, insetH)
        .padding(.bottom, insetB)
        .animation(notchAnimation, value: isOpened)
        .animation(.spring(response: 0.38, dampingFraction: 0.8), value: isHovering)
        .onHover { hovering in
            isHovering = hovering
        }
        .onChange(of: suggestedTab) { oldValue, newValue in
            if let tab = newValue {
                // "ALL" maps to nil (show everything)
                selectedTab = (tab == "ALL") ? nil : tab
            }
        }
    }

    // MARK: - Header Row

    @ViewBuilder
    private var headerRow: some View {
        // Closed state: indicators are rendered as flanking overlays (see closedFlankingIndicators)
        // to avoid being obscured by the macOS system notch mask on built-in displays.
        Color.clear
    }

    /// Indicators that flank the physical notch in closed state — positioned outside the
    /// clipped notch shape so they remain visible on built-in displays where macOS renders
    /// a system-level black mask over the camera notch area.
    @ViewBuilder
    private var closedFlankingIndicators: some View {
        if !isOpened && (hasAnySessions || hasAttention) {
            HStack(spacing: 0) {
                // Left indicator: bell with pulse when attention, green dot otherwise
                ZStack {
                    if hasAttention {
                        // Ripple wave 1
                        Circle()
                            .stroke(Color.orange.opacity(0.6), lineWidth: 1)
                            .frame(width: 12, height: 12)
                            .scaleEffect(ripple1 ? 3.0 : 1.0)
                            .opacity(ripple1 ? 0 : 0.6)
                        // Ripple wave 2 (delayed)
                        Circle()
                            .stroke(Color.orange.opacity(0.5), lineWidth: 1)
                            .frame(width: 12, height: 12)
                            .scaleEffect(ripple2 ? 3.0 : 1.0)
                            .opacity(ripple2 ? 0 : 0.5)
                        // Ripple wave 3 (more delayed)
                        Circle()
                            .stroke(Color.orange.opacity(0.4), lineWidth: 1)
                            .frame(width: 12, height: 12)
                            .scaleEffect(ripple3 ? 3.0 : 1.0)
                            .opacity(ripple3 ? 0 : 0.4)
                        // Bell icon with gentle swing
                        Image(systemName: "bell.fill")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(Color.orange)
                            .rotationEffect(.degrees(isPulsing ? 5 : -5), anchor: .top)
                    } else {
                        Circle()
                            .fill(Color.green)
                            .frame(width: 6, height: 6)
                    }
                }
                .frame(width: 22, height: 18)
                .task(id: hasAttention) {
                    isPulsing = false
                    ripple1 = false; ripple2 = false; ripple3 = false
                    guard hasAttention else { return }
                    try? await Task.sleep(nanoseconds: 100_000_000)
                    // Bell swing
                    withAnimation(.easeInOut(duration: 0.7).repeatForever(autoreverses: true)) {
                        isPulsing = true
                    }
                    // Staggered ripples — each loops independently
                    withAnimation(.easeOut(duration: 1.5).repeatForever(autoreverses: false)) {
                        ripple1 = true
                    }
                    try? await Task.sleep(nanoseconds: 500_000_000)
                    withAnimation(.easeOut(duration: 1.5).repeatForever(autoreverses: false)) {
                        ripple2 = true
                    }
                    try? await Task.sleep(nanoseconds: 500_000_000)
                    withAnimation(.easeOut(duration: 1.5).repeatForever(autoreverses: false)) {
                        ripple3 = true
                    }
                }

                Spacer()

                // Right indicator: session count
                if hasAnySessions {
                    Text("\(snapshot?.sessions.count ?? 0)")
                        .font(.system(size: 10, weight: .semibold, design: .rounded))
                        .foregroundStyle(.white.opacity(0.8))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 3)
                        .background(
                            Capsule().fill(Color.black.opacity(0.65))
                        )
                }
            }
            .frame(width: closedNotchWidth + 56)
            .frame(height: closedNotchHeight)
            .transition(.opacity)
        }
    }

    // MARK: - Usage Strip (flanking notch)

    private static let usageMicro: Font = .system(size: 14, weight: .medium, design: .monospaced)

    @ViewBuilder
    private func usageStrip(notchWidth: CGFloat, totalWidth: CGFloat) -> some View {
        let usage = snapshot?.usage
        let flankWidth = max(0, (totalWidth - notchWidth) / 2 - 8)

        HStack(spacing: 0) {
            // Left flank: Claude Code usage (24h / 7d)
            HStack(spacing: 6) {
                if let tokens24h = usage?.claude_24h_tokens {
                    HStack(spacing: 2) {
                        Text("Claude")
                            .foregroundStyle(Self.toolTabs[0].color.opacity(0.7))
                        Text("\(Self.formatTokens(tokens24h))/24h")
                            .foregroundStyle(.white.opacity(0.50))
                    }
                }
                if let tokens7d = usage?.claude_7d_tokens {
                    HStack(spacing: 2) {
                        Text("\(Self.formatTokens(tokens7d))/7d")
                            .foregroundStyle(.white.opacity(0.50))
                    }
                }
            }
            .font(Self.usageMicro)
            .frame(width: flankWidth, alignment: .trailing)
            .padding(.trailing, 6)

            Spacer()
                .frame(width: notchWidth)

            // Right flank: Codex + Copilot + toggle
            HStack(spacing: 6) {
                if let pct5h = usage?.codex_5h_remaining_pct {
                    HStack(spacing: 2) {
                        Text("Codex")
                            .foregroundStyle(Self.toolTabs[1].color.opacity(0.7))
                        Text("\(pct5h)%")
                            .foregroundStyle(.white.opacity(0.50))
                    }
                }
                if let pctX10 = usage?.copilot_premium_remaining_pct_x10 {
                    HStack(spacing: 2) {
                        Text("Copilot")
                            .foregroundStyle(Self.toolTabs[2].color.opacity(0.7))
                        Text(Self.formatPctX10(pctX10))
                            .foregroundStyle(.white.opacity(0.50))
                    }
                }
                Button(action: onToggleAutoExpand) {
                    Image(systemName: autoExpandEnabled ? "bell.fill" : "bell.slash.fill")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(.white.opacity(autoExpandEnabled ? 0.6 : 0.3))
                }
                .buttonStyle(.plain)
                .padding(.leading, 6)
                .help(autoExpandEnabled ? "Disable auto-expand" : "Enable auto-expand")
            }
            .font(Self.usageMicro)
            .frame(width: flankWidth, alignment: .leading)
            .padding(.leading, 6)
        }
    }

    private static func formatTokens(_ tokens: UInt64) -> String {
        if tokens >= 1_000_000 {
            let m = Double(tokens) / 1_000_000.0
            return String(format: "%.1fM", m)
        } else if tokens >= 1_000 {
            let k = Double(tokens) / 1_000.0
            return String(format: "%.0fK", k)
        }
        return "\(tokens)"
    }

    private static func formatPctX10(_ value: UInt64) -> String {
        let pct = Double(value) / 10.0
        if pct == pct.rounded() {
            return String(format: "%.0f%%", pct)
        }
        return String(format: "%.1f%%", pct)
    }

    // MARK: - Tool Tabs

    // Brand-accurate accent colors
    private static let toolTabs: [(key: String, tag: String, color: Color)] = [
        ("ClaudeCode", "ClaudeCode", Color(red: 0.855, green: 0.467, blue: 0.337)),  // #DA7756
        ("Codex",      "Codex",      Color(red: 0.063, green: 0.639, blue: 0.498)),  // #10A37F
        ("Copilot",    "Copilot",    Color(red: 0.65, green: 0.45, blue: 0.95)),  // lighter purple
    ]

    // MARK: - Pixel Art Tool Logos (faithful 2× sub-character decomposition)
    //
    // Each block/box character → 2×2 sub-pixels based on Unicode quadrant definitions.
    // Rendered with px_w=1pt, px_h=targetH/rows to normalize all icons to 12pt tall.

    // Claude Code (from ASCII art):
    //  ▐▛███▜▌     padded to 9 chars
    // ▝▜█████▛▘
    //   ▘▘ ▝▝
    // 2× decomposition: 18 cols × 6 rows (including empty bottom row)
    private static let claudePixels: [[UInt8]] = [
        [0,0,0,1,1,1,1,1,1,1,1,1,1,1,1,0,0,0],  // row0 top: ' ','▐','▛','█','█','█','▜','▌',' '
        [0,0,0,1,1,0,1,1,1,1,1,1,0,1,1,0,0,0],  // row0 bot
        [0,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,0],  // row1 top: '▝','▜','█','█','█','█','█','▛','▘'
        [0,0,0,1,1,1,1,1,1,1,1,1,1,1,1,0,0,0],  // row1 bot
        [0,0,0,0,1,0,1,0,0,0,0,1,0,1,0,0,0,0],  // row2 top: ' ',' ','▘','▘',' ','▝','▝',' ',' '
        [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],  // row2 bot (empty)
    ]

    // Codex (from ASCII art): >_ Codex → hand-designed >_ at 2px stroke
    // 10 cols × 8 rows
    private static let codexPixels: [[UInt8]] = [
        [1,1,0,0,0,0,0,0,0,0],
        [0,1,1,0,0,0,0,0,0,0],
        [0,0,1,1,0,0,0,0,0,0],
        [0,0,0,1,1,0,0,0,0,0],
        [0,0,1,1,0,0,0,0,0,0],
        [0,1,1,0,0,0,0,0,0,0],
        [1,1,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,1,1,1,1,1],
    ]

    // Copilot (from ASCII art):
    // ╭─╮╭─╮  box-drawing top edges → sub-pixel line positions
    // ╰─╯╰─╯  box-drawing bottom edges
    // █ ▘▝ █  block chars
    //  ▔▔▔▔   upper bar
    // 2× decomposition: 12 cols × 8 rows (preserving empty rows for spacing)
    private static let copilotPixels: [[UInt8]] = [
        [0,0,0,0,0,0,0,0,0,0,0,0],  // row0 top (box-drawing thin top)
        [0,1,1,1,1,0,0,1,1,1,1,0],  // row0 bot (╭─╮╭─╮ lower half)
        [0,1,1,1,1,0,0,1,1,1,1,0],  // row1 top (╰─╯╰─╯ upper half)
        [0,0,0,0,0,0,0,0,0,0,0,0],  // row1 bot (box-drawing thin bottom)
        [1,1,0,0,1,0,0,1,0,0,1,1],  // row2 top (█ ▘▝ █)
        [1,1,0,0,0,0,0,0,0,0,1,1],  // row2 bot
        [0,0,1,1,1,1,1,1,1,1,0,0],  // row3 top ( ▔▔▔▔ )
        [0,0,0,0,0,0,0,0,0,0,0,0],  // row3 bot (empty)
    ]

    // ALL tab: 2×2 grid / dashboard icon (12 cols × 12 rows, 1:1 pixel ratio)
    private static let allPixels: [[UInt8]] = [
        [1,1,1,1,1,0,0,1,1,1,1,1],
        [1,1,1,1,1,0,0,1,1,1,1,1],
        [1,1,1,1,1,0,0,1,1,1,1,1],
        [1,1,1,1,1,0,0,1,1,1,1,1],
        [1,1,1,1,1,0,0,1,1,1,1,1],
        [0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0,0],
        [1,1,1,1,1,0,0,1,1,1,1,1],
        [1,1,1,1,1,0,0,1,1,1,1,1],
        [1,1,1,1,1,0,0,1,1,1,1,1],
        [1,1,1,1,1,0,0,1,1,1,1,1],
        [1,1,1,1,1,0,0,1,1,1,1,1],
    ]

    private static let toolPixelMap: [String: [[UInt8]]] = [
        "ALL": allPixels,
        "ClaudeCode": claudePixels,
        "Codex": codexPixels,
        "Copilot": copilotPixels,
    ]

    @ViewBuilder
    private static func toolIcon(for key: String, color: Color, isSelected: Bool) -> some View {
        let baseOpacity = isSelected ? 0.85 : 0.35
        if let pixels = toolPixelMap[key] {
            let rows = pixels.count
            let cols = pixels.map(\.count).max() ?? 1
            let targetH: CGFloat = 12
            let pxW: CGFloat = 1.0
            let pxH: CGFloat = targetH / CGFloat(rows)

            Canvas { context, _ in
                for r in 0..<rows {
                    for c in 0..<pixels[r].count where pixels[r][c] > 0 {
                        let rect = CGRect(
                            x: CGFloat(c) * pxW,
                            y: CGFloat(r) * pxH,
                            width: pxW,
                            height: pxH
                        )
                        context.fill(Path(rect), with: .color(color.opacity(baseOpacity)))
                    }
                }
            }
            .frame(width: CGFloat(cols) * pxW, height: targetH)
        } else {
            RoundedRectangle(cornerRadius: 2)
                .fill(color.opacity(baseOpacity))
                .frame(width: 12, height: 12)
        }
    }

    private static let mono: Font = .system(size: 14, weight: .regular, design: .monospaced)
    private static let monoSmall: Font = .system(size: 13, weight: .regular, design: .monospaced)
    private static let monoBold: Font = .system(size: 14, weight: .medium, design: .monospaced)

    private func attentionCount(for source: String?) -> Int {
        guard let attentions = snapshot?.attentions else { return 0 }
        if let source { return attentions.filter { $0.source == source }.count }
        return attentions.count
    }

    private func sessionCount(for source: String?) -> Int {
        guard let sessions = snapshot?.sessions else { return 0 }
        if let source { return sessions.filter { $0.source == source }.count }
        return sessions.count
    }

    private var filteredAttentions: [AttentionCard] {
        guard let attentions = snapshot?.attentions else { return [] }
        if let tab = selectedTab {
            return attentions.filter { $0.source == tab }
        } else {
            return attentions
        }
    }

    private var filteredSessions: [SessionListItem] {
        guard let sessions = snapshot?.sessions else { return [] }
        guard let tab = selectedTab else { return sessions }
        return sessions.filter { $0.source == tab }
    }

    @ViewBuilder
    private var toolTabBar: some View {
        HStack(spacing: 0) {
            tabButton(tag: "ALL", isSelected: selectedTab == nil, badgeCount: 0) {
                selectedTab = nil
            }

            ForEach(Self.toolTabs, id: \.key) { tab in
                let count = attentionCount(for: tab.key)
                let hasSessions = sessionCount(for: tab.key) > 0
                if hasSessions || count > 0 {
                    tabButton(tag: tab.tag, isSelected: selectedTab == tab.key, badgeCount: count, accentColor: tab.color) {
                        selectedTab = tab.key
                    }
                }
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private func tabButton(tag: String, isSelected: Bool, badgeCount: Int, accentColor: Color = .white, action: @escaping () -> Void) -> some View {
        HStack(spacing: 4) {
            Self.toolIcon(for: tag, color: accentColor, isSelected: isSelected)
            Text(tag)
                .font(Self.mono)
                .foregroundStyle(isSelected ? .white.opacity(0.9) : .white.opacity(0.30))
            if badgeCount > 0 {
                Text("·\(badgeCount)")
                    .font(Self.monoSmall)
                    .foregroundStyle(Color(red: 0.9, green: 0.3, blue: 0.3))
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
        .overlay(
            Rectangle()
                .frame(height: isSelected ? 2 : 1)
                .foregroundStyle(isSelected ? accentColor.opacity(0.7) : .white.opacity(0.10)),
            alignment: .bottom
        )
        .contentShape(Rectangle())
        .onHover { hovering in
            if hovering { action() }
        }
    }

    // MARK: - Opened Content

    @ViewBuilder
    private var openedContent: some View {
        VStack(alignment: .leading, spacing: 8) {
            let activeSources = Set(snapshot?.sessions.map(\.source) ?? [])
            if activeSources.count > 1 || hasAttention {
                toolTabBar
            }

            let attentions = filteredAttentions
            if !attentions.isEmpty {
                // Clear all button for current tab
                HStack {
                    Spacer()
                    Button(action: {
                        onAction(.clearAttentions(source: selectedTab))
                    }) {
                        HStack(spacing: 3) {
                            Image(systemName: "xmark.circle")
                                .font(.system(size: 10))
                            Text("Clear all")
                                .font(Self.monoSmall)
                        }
                        .foregroundStyle(.white.opacity(0.35))
                    }
                    .buttonStyle(.plain)
                    .onHover { hovering in
                        if hovering {
                            NSCursor.pointingHand.push()
                        } else {
                            NSCursor.pop()
                        }
                    }
                }
                ScrollView(.vertical, showsIndicators: true) {
                    VStack(alignment: .leading, spacing: 12) {
                        ForEach(Array(attentions.enumerated()), id: \.element.request_id) { _, attention in
                            attentionCardView(for: attention)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 480)
            } else {
                let sessions = filteredSessions
                if !sessions.isEmpty {
                    VStack(spacing: 2) {
                        Text("\(sessions.count) session(s) active")
                            .font(Self.mono)
                            .foregroundStyle(.white.opacity(0.7))
                        Text("no pending actions")
                            .font(Self.monoSmall)
                            .foregroundStyle(.white.opacity(0.35))
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.bottom, 16)
                } else {
                    Text("no active sessions")
                        .font(Self.monoSmall)
                        .foregroundStyle(.white.opacity(0.25))
                        .frame(maxWidth: .infinity)
                        .padding(.bottom, 16)
                }
            }
        }
        .padding(.horizontal, 18)
        .padding(.top, 8)
        .padding(.bottom, 6)
    }

    // Dash-dot border: thin, bright
    private static let dashDotStyle = StrokeStyle(lineWidth: 1.5, dash: [6, 3, 2, 3])

    @ViewBuilder
    private func attentionCardView(for attention: AttentionCard) -> some View {
        let accent = sourceAccent(attention.source)
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 4) {
                Text(sourceTag(attention.source))
                    .font(Self.monoSmall)
                    .foregroundStyle(accent.opacity(0.9))
                Text(attention.title)
                    .font(Self.monoBold)
                    .foregroundStyle(.white.opacity(0.50))
                Spacer()
                // Dismiss button
                Button(action: {
                    onAction(.dismissAttention(requestId: attention.request_id))
                }) {
                    Image(systemName: "xmark")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(.white.opacity(0.4))
                        .frame(width: 18, height: 18)
                        .background(Circle().fill(.white.opacity(0.08)))
                }
                .buttonStyle(.plain)
                .help("Dismiss")
            }
            Text(attention.message)
                .font(Self.mono)
                .foregroundStyle(.white.opacity(0.85))
                .lineLimit(3)
                .frame(maxWidth: .infinity, alignment: .leading)

            actionButtons(for: attention, accent: accent)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(accent.opacity(0.14), in: RoundedRectangle(cornerRadius: 4))
        .overlay(
            RoundedRectangle(cornerRadius: 4)
                .strokeBorder(accent.opacity(0.55), style: Self.dashDotStyle)
        )
    }

    private func sourceTag(_ source: String) -> String {
        switch source {
        case "ClaudeCode": return "[CC]"
        case "Codex":      return "[CX]"
        case "Copilot":    return "[CP]"
        default:           return "[??]"
        }
    }

    private func sourceAccent(_ source: String) -> Color {
        Self.toolTabs.first(where: { $0.key == source })?.color ?? .white.opacity(0.5)
    }

    // Brighter version of accent for send button (mix toward white by ~30%)
    private static func brightenedAccent(_ source: String) -> Color {
        switch source {
        case "ClaudeCode": return Color(red: 0.92, green: 0.58, blue: 0.45)  // lighter terra cotta
        case "Codex":      return Color(red: 0.15, green: 0.78, blue: 0.62)  // lighter teal
        case "Copilot":    return Color(red: 0.72, green: 0.55, blue: 0.98)  // lighter purple
        default:           return Color.white.opacity(0.6)
        }
    }

    // MARK: - Action Buttons

    @ViewBuilder
    private func actionButtons(for attention: AttentionCard, accent: Color) -> some View {
        switch attention.status {
        case .waitingApproval:
            HStack(spacing: 10) {
                terminalButton("allow", color: Color(red: 0.35, green: 0.82, blue: 0.48), primary: true) {
                    onAction(.approve(requestId: attention.request_id, always: false))
                }
                terminalButton("always", color: Color(red: 0.35, green: 0.82, blue: 0.48), primary: false) {
                    onAction(.approve(requestId: attention.request_id, always: true))
                }
                terminalButton("deny", color: Color(red: 0.90, green: 0.38, blue: 0.38), primary: false) {
                    onAction(.deny(requestId: attention.request_id))
                }
            }

        case .waitingQuestion:
            VStack(spacing: 10) {
                FlowLayout(spacing: 8) {
                    ForEach(Array(attention.options.enumerated()), id: \.element) { idx, option in
                        terminalButton(option, color: Color(red: 0.50, green: 0.72, blue: 0.95), primary: idx == 0) {
                            onAction(.answerOption(requestId: attention.request_id, optionId: option))
                        }
                    }
                }

                if attention.allow_freeform {
                    let inputBlue = Color(red: 0.40, green: 0.65, blue: 0.95)
                    let isFocused = focusedField == attention.request_id
                    HStack(spacing: 10) {
                        TextField("…", text: freeformBinding(for: attention.request_id))
                            .textFieldStyle(.plain)
                            .font(Self.mono)
                            .foregroundStyle(.white.opacity(0.85))
                            .tint(.white)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 10)
                            .background(
                                isFocused ? inputBlue.opacity(0.18) : Color.white.opacity(0.14),
                                in: RoundedRectangle(cornerRadius: 3)
                            )
                            .focused($focusedField, equals: attention.request_id)
                            .onHover { hovering in
                                if hovering {
                                    focusedField = attention.request_id
                                } else if focusedField == attention.request_id {
                                    focusedField = nil
                                }
                            }
                            .onSubmit {
                                let text = freeformTexts[attention.request_id] ?? ""
                                guard !text.isEmpty else { return }
                                onAction(.answerText(requestId: attention.request_id, text: text))
                                freeformTexts[attention.request_id] = ""
                            }

                        Button {
                            let text = freeformTexts[attention.request_id] ?? ""
                            guard !text.isEmpty else { return }
                            onAction(.answerText(requestId: attention.request_id, text: text))
                            freeformTexts[attention.request_id] = ""
                        } label: {
                            Text(">")
                                .font(Self.monoBold)
                                .foregroundStyle(.white)
                                .padding(.horizontal, 12)
                                .padding(.vertical, 10)
                                .background(Self.brightenedAccent(attention.source), in: RoundedRectangle(cornerRadius: 3))
                        }
                        .buttonStyle(.plain)
                        .disabled((freeformTexts[attention.request_id] ?? "").isEmpty)
                        .opacity((freeformTexts[attention.request_id] ?? "").isEmpty ? 0.40 : 1.0)
                    }
                }
            }

        default:
            EmptyView()
        }
    }

    @ViewBuilder
    private func terminalButton(_ label: String, color: Color, primary: Bool = false, action: @escaping () -> Void) -> some View {
        TerminalButtonView(label: label, color: color, primary: primary, action: action)
    }

    private func freeformBinding(for requestId: String) -> Binding<String> {
        Binding(
            get: { freeformTexts[requestId] ?? "" },
            set: { freeformTexts[requestId] = $0 }
        )
    }
}

// Separate struct for hover state on buttons
private struct TerminalButtonView: View {
    let label: String
    let color: Color
    let primary: Bool
    let action: () -> Void

    private static let mono = Font.system(size: 13, weight: .regular, design: .monospaced)

    @State private var isHovered = false

    var body: some View {
        Text(label)
            .font(Self.mono)
            .foregroundStyle(isHovered || primary ? color : color.opacity(0.80))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 10)
            .padding(.horizontal, 8)
            .background(
                color.opacity(isHovered ? 0.35 : (primary ? 0.28 : 0.18)),
                in: RoundedRectangle(cornerRadius: 3)
            )
            .contentShape(Rectangle())
            .onHover { hovering in
                isHovered = hovering
            }
            .onTapGesture {
                action()
            }
    }
}
