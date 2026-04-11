import Foundation
import Darwin

actor IPCClient {
    private let socketPath: String

    init(socketPath: String = NSHomeDirectory() + "/.local/share/pupkit/pupkitd.sock") {
        self.socketPath = socketPath
    }

    func fetchStateSnapshot() async throws -> UiStateSnapshot {
        let response = try send(request: .stateSnapshot)
        switch response {
        case .stateSnapshot(let snapshot):
            return snapshot
        case .uiActionResult(let payload):
            return payload.state
        case .ack:
            throw NSError(domain: "PupkitShell", code: 1, userInfo: [NSLocalizedDescriptionKey: "Unexpected ACK response"])
        case .error(let message):
            throw NSError(domain: "PupkitShell", code: 2, userInfo: [NSLocalizedDescriptionKey: message])
        }
    }

    func approve(requestId: String, always: Bool = false) async throws -> UiStateSnapshot {
        let response = try send(request: .ui(.approve(requestId: requestId, always: always)))
        return try unwrapUiState(from: response)
    }

    func deny(requestId: String) async throws -> UiStateSnapshot {
        let response = try send(request: .ui(.deny(requestId: requestId)))
        return try unwrapUiState(from: response)
    }

    func answerOption(requestId: String, optionId: String) async throws -> UiStateSnapshot {
        let response = try send(request: .ui(.answerOption(requestId: requestId, optionId: optionId)))
        return try unwrapUiState(from: response)
    }

    private func unwrapUiState(from response: ServerResponse) throws -> UiStateSnapshot {
        switch response {
        case .uiActionResult(let payload):
            return payload.state
        case .stateSnapshot(let snapshot):
            return snapshot
        case .ack:
            throw NSError(domain: "PupkitShell", code: 6, userInfo: [NSLocalizedDescriptionKey: "Unexpected ACK response"])
        case .error(let message):
            throw NSError(domain: "PupkitShell", code: 7, userInfo: [NSLocalizedDescriptionKey: message])
        }
    }

    private func send(request: ClientRequestEnvelope) throws -> ServerResponse {
        let payload = try JSONEncoder().encode(request)
        let responseData = try sendRaw(jsonData: payload)
        return try JSONDecoder().decode(ServerResponse.self, from: responseData)
    }

    private func sendRaw(jsonData: Data) throws -> Data {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            throw NSError(domain: "PupkitShell", code: 3, userInfo: [NSLocalizedDescriptionKey: "Failed to create unix socket"])
        }
        defer { close(fd) }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let maxPathLength = MemoryLayout.size(ofValue: addr.sun_path)
        guard socketPath.utf8.count < maxPathLength else {
            throw NSError(domain: "PupkitShell", code: 5, userInfo: [NSLocalizedDescriptionKey: "Socket path too long"])
        }
        withUnsafeMutablePointer(to: &addr.sun_path.0) { ptr in
            ptr.initialize(to: 0)
            socketPath.withCString { cString in
                strncpy(ptr, cString, maxPathLength - 1)
            }
        }

        let connectResult = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connectResult == 0 else {
            throw NSError(domain: "PupkitShell", code: 4, userInfo: [NSLocalizedDescriptionKey: "Failed to connect to \(socketPath)"])
        }

        _ = jsonData.withUnsafeBytes { bytes in
            send(fd, bytes.baseAddress, bytes.count, 0)
        }
        shutdown(fd, SHUT_WR)

        var output = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while true {
            let count = recv(fd, &buffer, buffer.count, 0)
            if count <= 0 { break }
            output.append(contentsOf: buffer.prefix(Int(count)))
        }
        return output
    }
}
