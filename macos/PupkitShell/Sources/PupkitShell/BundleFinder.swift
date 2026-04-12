import Foundation

extension Bundle {
    /// Safely locate the SPM resource bundle relative to the running executable.
    /// Unlike the auto-generated `Bundle.module`, this returns nil instead of
    /// calling fatalError when the bundle is not found — critical for CLI tools
    /// installed outside the original build directory (e.g. via Homebrew).
    static var pupkitResources: Bundle? {
        let bundleName = "PupkitShell_PupkitShell.bundle"

        // 1. Resolve the executable's real path (follows symlinks)
        let executableURL = URL(fileURLWithPath: ProcessInfo.processInfo.arguments[0])
            .resolvingSymlinksInPath()
        let siblingURL = executableURL.deletingLastPathComponent()
            .appendingPathComponent(bundleName)
        if let bundle = Bundle(path: siblingURL.path) {
            return bundle
        }

        // 2. Fallback: Bundle.main approach (works for .app bundles)
        let mainPath = Bundle.main.bundleURL.appendingPathComponent(bundleName).path
        if let bundle = Bundle(path: mainPath) {
            return bundle
        }

        return nil
    }
}
