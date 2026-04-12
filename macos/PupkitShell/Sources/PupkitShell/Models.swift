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
    let allow_freeform: Bool
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
    let attentions: [AttentionCard]
    let sessions: [SessionListItem]
    let recent_completions: [CompletionItem]
    let usage: UsageCompact?
}

struct UsageCompact: Decodable {
    let claude_24h_tokens: UInt64?
    let claude_7d_tokens: UInt64?
    let codex_5h_remaining_pct: UInt8?
    let codex_7d_remaining_pct: UInt8?
    let copilot_premium_remaining_pct_x10: UInt64?
}

// MARK: - UiAction (Encodable, sent to daemon)

enum UiAction: Encodable {
    case approve(requestId: String, always: Bool)
    case deny(requestId: String)
    case answerOption(requestId: String, optionId: String)
    case answerText(requestId: String, text: String)
    case dismissCompletion(sessionId: String)

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .approve(let requestId, let always):
            try container.encode(["Approve": ApprovePayload(request_id: requestId, always: always)])
        case .deny(let requestId):
            try container.encode(["Deny": DenyPayload(request_id: requestId)])
        case .answerOption(let requestId, let optionId):
            try container.encode(["AnswerOption": AnswerOptionPayload(request_id: requestId, option_id: optionId)])
        case .answerText(let requestId, let text):
            try container.encode(["AnswerText": AnswerTextPayload(request_id: requestId, text: text)])
        case .dismissCompletion(let sessionId):
            try container.encode(["DismissCompletion": DismissPayload(session_id: sessionId)])
        }
    }
}

private struct ApprovePayload: Encodable { let request_id: String; let always: Bool }
private struct DenyPayload: Encodable { let request_id: String }
private struct AnswerOptionPayload: Encodable { let request_id: String; let option_id: String }
private struct AnswerTextPayload: Encodable { let request_id: String; let text: String }
private struct DismissPayload: Encodable { let session_id: String }

// MARK: - ClientRequest (Encodable, sent to daemon)

enum ClientRequest: Encodable {
    case stateSnapshot
    case ui(UiAction)

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .stateSnapshot:
            try container.encode("StateSnapshot")
        case .ui(let action):
            try container.encode(["Ui": action])
        }
    }
}

// MARK: - ServerResponse (Decodable, received from daemon)

enum ServerResponse: Decodable {
    case ack
    case stateSnapshot(UiStateSnapshot)
    case uiActionResult(decision: HookDecisionPayload?, state: UiStateSnapshot)
    case error(String)

    private enum CodingKeys: String, CodingKey {
        case StateSnapshot
        case Error
        case UiActionResult
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

        if let object = try? decoder.singleValueContainer().decode([String: UiActionResultPayload].self),
           let result = object["UiActionResult"] {
            self = .uiActionResult(decision: result.decision, state: result.state)
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

struct HookDecisionPayload: Decodable {}
private struct UiActionResultPayload: Decodable {
    let decision: HookDecisionPayload?
    let state: UiStateSnapshot
}
private struct ErrorPayload: Decodable {
    let message: String
}
