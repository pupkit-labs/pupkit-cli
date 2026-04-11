import Foundation

enum ShellStatus: String, Decodable {
    case running = "Running"
    case waitingApproval = "WaitingApproval"
    case waitingQuestion = "WaitingQuestion"
    case completedRecent = "CompletedRecent"
    case failed = "Failed"
    case ended = "Ended"
    case stale = "Stale"
}

struct AttentionCard: Decodable {
    let session_id: String
    let request_id: String
    let source: String
    let title: String
    let status: ShellStatus
    let message: String
    let options: [String]
}

struct SessionListItem: Decodable {
    let session_id: String
    let source: String
    let title: String
    let status: ShellStatus
    let summary: String?
}

struct CompletionItem: Decodable {
    let session_id: String
    let source: String
    let title: String
    let headline: String
    let body: String
}

struct UiStateSnapshot: Decodable {
    let top_attention: AttentionCard?
    let sessions: [SessionListItem]
    let recent_completions: [CompletionItem]
}

enum ServerResponse: Decodable {
    case ack
    case stateSnapshot(UiStateSnapshot)
    case error(String)

    private enum CodingKeys: String, CodingKey {
        case StateSnapshot
        case Error
        case message
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let raw = try? container.decode(String.self), raw == "Ack" {
            self = .ack
            return
        }

        if let object = try? decoder.singleValueContainer().decode([String: UiStateSnapshot].self),
           let snapshot = object["StateSnapshot"] {
            self = .stateSnapshot(snapshot)
            return
        }

        if let object = try? decoder.singleValueContainer().decode([String: ErrorPayload].self),
           let payload = object["Error"] {
            self = .error(payload.message)
            return
        }

        throw DecodingError.dataCorruptedError(in: container, debugDescription: "Unsupported server response")
    }
}

private struct ErrorPayload: Decodable {
    let message: String
}
