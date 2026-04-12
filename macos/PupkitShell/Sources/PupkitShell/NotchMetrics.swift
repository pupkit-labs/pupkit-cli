import AppKit
import CoreGraphics

// MARK: - NSScreen Notch Detection

extension NSScreen {
    /// True only for the built-in display (MacBook screen), not external monitors.
    var isBuiltIn: Bool {
        let key = NSDeviceDescriptionKey("NSScreenNumber")
        guard let screenNumber = deviceDescription[key] as? CGDirectDisplayID else { return false }
        return CGDisplayIsBuiltin(screenNumber) != 0
    }

    /// True only when the screen has a physical camera notch (auxiliary areas
    /// flanking the notch exist). A menu-bar safe area alone does NOT qualify.
    var hasNotch: Bool {
        guard safeAreaInsets.top > 0 else { return false }
        return auxiliaryTopLeftArea != nil || auxiliaryTopRightArea != nil
    }

    var notchSize: CGSize {
        guard hasNotch else {
            return CGSize(width: 224, height: 38)
        }
        let notchHeight = safeAreaInsets.top
        let leftPadding = auxiliaryTopLeftArea?.width ?? 0
        let rightPadding = auxiliaryTopRightArea?.width ?? 0
        let notchWidth = frame.width - leftPadding - rightPadding + 4
        return CGSize(width: notchWidth, height: notchHeight)
    }

    var islandClosedHeight: CGFloat {
        if hasNotch {
            return safeAreaInsets.top
        }
        let reserved = max(0, frame.maxY - visibleFrame.maxY)
        return reserved > 0 ? reserved : 24
    }
}

// MARK: - Layout Constants

enum IslandMetrics {
    static let openedPanelWidth: CGFloat = 680
    static let openedShadowHorizontalInset: CGFloat = 18
    static let openedShadowBottomInset: CGFloat = 22
    static let closedHoverScale: CGFloat = 1.028
    static let hoverOpenDelay: TimeInterval = 0.3
    static let hoverCloseDelay: TimeInterval = 0.3
}
