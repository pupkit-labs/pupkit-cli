import AppKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private let statusItemController = StatusItemController()
    private let notchController = NotchPanelController()
    private let ipcClient = IPCClient()
    private var refreshTimer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        statusItemController.start(ipcClient: ipcClient, notchController: notchController)
        notchController.configure(ipcClient: ipcClient) { [weak self] updatedState in
            self?.statusItemController.apply(snapshot: updatedState)
        }
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            Task { @MainActor in
                await self?.refreshState()
            }
        }
        Task { @MainActor in
            await refreshState()
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        refreshTimer?.invalidate()
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
}
