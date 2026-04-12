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
        let previousAttentionIds = Set(latestSnapshot?.attentions.map(\.request_id) ?? [])
        latestSnapshot = snapshot
        updateView()

        let currentAttentionIds = Set(snapshot?.attentions.map(\.request_id) ?? [])
        let hasNew = !currentAttentionIds.subtracting(previousAttentionIds).isEmpty
        if hasNew && islandStatus == .closed {
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

// MARK: - Island Content View

struct IslandContentView: View {
    let snapshot: UiStateSnapshot?
    let isOpened: Bool
    let closedNotchWidth: CGFloat
    let closedNotchHeight: CGFloat
    let onAction: (UiAction) -> Void

    @State private var isHovering = false
    @State private var freeformTexts: [String: String] = [:]
    @State private var selectedTab: String? = nil  // nil = "All"
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
            // 22 = top padding(8) + VStack spacing(8) + bottom padding(6)
            return tabBarHeight + scrollContent + 22
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

    // MARK: - Tool Tabs

    // Brand-accurate accent colors
    private static let toolTabs: [(key: String, tag: String, color: Color)] = [
        ("ClaudeCode", "CC", Color(red: 0.855, green: 0.467, blue: 0.337)),  // #DA7756
        ("Codex",      "CX", Color(red: 0.063, green: 0.639, blue: 0.498)),  // #10A37F
        ("Copilot",    "CP", Color(red: 0.65, green: 0.45, blue: 0.95)),  // lighter purple
    ]

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
        guard let tab = selectedTab else { return attentions }
        return attentions.filter { $0.source == tab }
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
            RoundedRectangle(cornerRadius: 2)
                .fill(accentColor.opacity(isSelected ? 0.85 : 0.35))
                .frame(width: 12, height: 12)
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
                terminalButton("allow", color: Color(red: 0.30, green: 0.75, blue: 0.40), primary: true) {
                    onAction(.approve(requestId: attention.request_id, always: false))
                }
                terminalButton("always", color: Color(red: 0.30, green: 0.75, blue: 0.40), primary: false) {
                    onAction(.approve(requestId: attention.request_id, always: true))
                }
                terminalButton("deny", color: Color(red: 0.85, green: 0.30, blue: 0.30), primary: false) {
                    onAction(.deny(requestId: attention.request_id))
                }
            }

        case .waitingQuestion:
            VStack(spacing: 10) {
                FlowLayout(spacing: 8) {
                    ForEach(Array(attention.options.enumerated()), id: \.element) { idx, option in
                        terminalButton(option, color: Color(red: 0.40, green: 0.65, blue: 0.90), primary: idx == 0) {
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
                                isFocused ? inputBlue.opacity(0.12) : Color.white.opacity(0.10),
                                in: RoundedRectangle(cornerRadius: 3)
                            )
                            .overlay(
                                RoundedRectangle(cornerRadius: 3)
                                    .strokeBorder(
                                        isFocused ? inputBlue.opacity(0.70) : inputBlue.opacity(0.35),
                                        style: Self.dashDotStyle
                                    )
                                    .allowsHitTesting(false)
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
    private static let dashDotStyle = StrokeStyle(lineWidth: 1.5, dash: [6, 3, 2, 3])

    @State private var isHovered = false

    var body: some View {
        Text(label)
            .font(Self.mono)
            .foregroundStyle(isHovered || primary ? color : color.opacity(0.70))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 10)
            .padding(.horizontal, 8)
            .background(
                color.opacity(isHovered ? 0.30 : (primary ? 0.22 : 0.12)),
                in: RoundedRectangle(cornerRadius: 3)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 3)
                    .strokeBorder(
                        color.opacity(isHovered ? 0.85 : (primary ? 0.70 : 0.40)),
                        style: Self.dashDotStyle
                    )
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
