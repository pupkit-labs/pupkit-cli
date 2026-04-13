import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let statusItemController = StatusItemController()
    private let notchController = NotchPanelController()
    private let ipcClient = IPCClient()
    private var refreshTimer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        // Close any auto-opened Settings window (SwiftUI may spawn one)
        closeSettingsWindows()
        statusItemController.start(ipcClient: ipcClient, notchController: notchController)
        notchController.configure(ipcClient: ipcClient) { [weak self] updatedState in
            self?.statusItemController.apply(snapshot: updatedState)
        }
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                await self?.refreshState()
            }
        }
        Task { @MainActor in
            await refreshState()
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        refreshTimer?.invalidate()
        let home = FileManager.default.homeDirectoryForCurrentUser
        let pupkitDir = home.appendingPathComponent(".local/share/pupkit")

        // Write shell-paused marker so watchdog won't restart during shutdown
        let marker = pupkitDir.appendingPathComponent("shell-paused")
        FileManager.default.createFile(atPath: marker.path, contents: nil)

        // Also stop the daemon process
        let pidFile = pupkitDir.appendingPathComponent("pupkitd.pid")
        if let pidString = try? String(contentsOf: pidFile, encoding: .utf8)
            .trimmingCharacters(in: .whitespacesAndNewlines),
           let pid = Int32(pidString) {
            kill(pid, SIGTERM)
        }
    }

    private func refreshState() async {
        do {
            let snapshot = try await ipcClient.fetchStateSnapshot()
            statusItemController.apply(snapshot: snapshot)
            notchController.apply(snapshot: snapshot)
        } catch {
            statusItemController.apply(error: error.localizedDescription)
            notchController.apply(snapshot: nil)
        }
    }

    /// Close any SwiftUI-managed Settings windows that auto-open on launch or reactivation.
    private func closeSettingsWindows() {
        for window in NSApp.windows where window.title.lowercased().contains("setting") || window.className.contains("Settings") {
            window.close()
        }
    }

    func applicationDidBecomeActive(_ notification: Notification) {
        closeSettingsWindows()
    }
}
