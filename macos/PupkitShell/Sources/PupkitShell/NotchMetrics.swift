import AppKit
import CoreGraphics

// MARK: - NSScreen Notch Detection

extension NSScreen {
    var notchSize: CGSize {
        guard safeAreaInsets.top > 0 else {
            return CGSize(width: 224, height: 38)
        }
        let notchHeight = safeAreaInsets.top
        let leftPadding = auxiliaryTopLeftArea?.width ?? 0
        let rightPadding = auxiliaryTopRightArea?.width ?? 0
        let notchWidth = frame.width - leftPadding - rightPadding + 4
        return CGSize(width: notchWidth, height: notchHeight)
    }

    var hasNotch: Bool {
        safeAreaInsets.top > 0
    }

    var islandClosedHeight: CGFloat {
        if safeAreaInsets.top > 0 {
            return safeAreaInsets.top
        }
        let reserved = max(0, frame.maxY - visibleFrame.maxY)
        return reserved > 0 ? reserved : 24
    }
}

// MARK: - Layout Constants

enum IslandMetrics {
    static let openedPanelWidth: CGFloat = 560
    static let openedShadowHorizontalInset: CGFloat = 18
    static let openedShadowBottomInset: CGFloat = 22
    static let closedHoverScale: CGFloat = 1.028
    static let hoverOpenDelay: TimeInterval = 0.3
    static let hoverCloseDelay: TimeInterval = 0.5
}
