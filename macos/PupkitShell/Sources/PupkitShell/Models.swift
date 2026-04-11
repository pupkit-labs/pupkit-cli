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
    let last_updated_at: UInt64
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

struct UiActionResultPayload: Decodable {
    let decision: HookDecisionPayload?
    let state: UiStateSnapshot
}

enum HookDecisionPayload: Decodable {
    case approval(requestId: String)
    case questionAnswer(requestId: String)
    case cancelled(requestId: String)
    case timeout(requestId: String)
    case ack

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let raw = try? container.decode(String.self), raw == "Ack" {
            self = .ack
            return
        }
        if let object = try? container.decode([String: ApprovalPayload].self),
           let payload = object["Approval"] {
            self = .approval(requestId: payload.request_id)
            return
        }
        if let object = try? container.decode([String: AnswerPayload].self),
           let payload = object["QuestionAnswer"] {
            self = .questionAnswer(requestId: payload.request_id)
            return
        }
        if let object = try? container.decode([String: RequestOnlyPayload].self),
           let payload = object["Cancelled"] {
            self = .cancelled(requestId: payload.request_id)
            return
        }
        if let object = try? container.decode([String: RequestOnlyPayload].self),
           let payload = object["Timeout"] {
            self = .timeout(requestId: payload.request_id)
            return
        }
        throw DecodingError.dataCorruptedError(in: container, debugDescription: "Unsupported hook decision payload")
    }
}

enum ServerResponse: Decodable {
    case ack
    case stateSnapshot(UiStateSnapshot)
    case uiActionResult(UiActionResultPayload)
    case error(String)

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let raw = try? container.decode(String.self), raw == "Ack" {
            self = .ack
            return
        }

        if let object = try? container.decode([String: UiStateSnapshot].self),
           let snapshot = object["StateSnapshot"] {
            self = .stateSnapshot(snapshot)
            return
        }

        if let object = try? container.decode([String: UiActionResultPayload].self),
           let payload = object["UiActionResult"] {
            self = .uiActionResult(payload)
            return
        }

        if let object = try? container.decode([String: ErrorPayload].self),
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

private struct RequestOnlyPayload: Decodable {
    let request_id: String
}

private struct ApprovalPayload: Decodable {
    let request_id: String
}

private struct AnswerPayload: Decodable {
    let request_id: String
}

enum ClientRequestEnvelope: Encodable {
    case stateSnapshot
    case ui(UiActionEnvelope)

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

enum UiActionEnvelope: Encodable {
    case approve(requestId: String, always: Bool)
    case deny(requestId: String)
    case answerOption(requestId: String, optionId: String)

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .approve(let requestId, let always):
            try container.encode(["Approve": ["request_id": requestId, "always": always] as [String: AnyEncodable]])
        case .deny(let requestId):
            try container.encode(["Deny": ["request_id": requestId] as [String: AnyEncodable]])
        case .answerOption(let requestId, let optionId):
            try container.encode(["AnswerOption": ["request_id": requestId, "option_id": optionId] as [String: AnyEncodable]])
        }
    }
}

struct AnyEncodable: Encodable {
    private let encodeImpl: (Encoder) throws -> Void

    init<T: Encodable>(_ value: T) {
        self.encodeImpl = value.encode
    }

    func encode(to encoder: Encoder) throws {
        try encodeImpl(encoder)
    }
}
