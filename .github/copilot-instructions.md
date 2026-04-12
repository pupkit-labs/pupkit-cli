# Copilot Instructions

## Build & Run

```sh
cargo build
cargo run                      # defaults to slim `welcome` + auto-starts daemon
cargo run -- welcome
cargo run -- daemon            # start background service + PupkitShell
cargo run -- details           # full system-summary + ai-tools + ai-skills + ai-usage + services
cargo run -- system-summary
cargo run -- ai-tools
cargo run -- ai-tools --skills
cargo run -- ai-usage
cargo run -- install
cargo run -- services
```

## Tests

```sh
cargo test                                        # all tests
cargo test display_label_appends_flag             # single test by name (substring match)
cargo test -- --nocapture                         # show println output
```

Tests live **inline in each source file** (`#[cfg(test)]` blocks). There are no `.rs` files under `tests/` — that directory holds only fixtures and snapshots.

## Architecture

```
main.rs
  └─ lib.rs::run()
       └─ commands/mod.rs::run()   ← arg parsing, dispatch
            ├─ commands/<cmd>.rs   ← thin glue: call collector(s), call render, print
            ├─ collectors/<x>.rs   ← data acquisition (I/O, file reads, subprocess)
            ├─ render/mod.rs       ← pure string rendering, no I/O
            ├─ model/mod.rs        ← plain data structs/enums, no logic beyond Display helpers
            ├─ shell/mod.rs        ← TTY detection, shell label, subprocess helpers
            └─ daemon/             ← background service: socket server, watcher, shell launcher
                 ├─ server.rs      ← bind() + accept_loop(), client request handling
                 ├─ watcher/       ← JSONL file polling for session discovery
                 ├─ shell_launcher.rs  ← macOS PupkitShell auto-launch + download
                 ├─ config.rs      ← DaemonConfig (socket, state, shell paths)
                 └─ app.rs         ← PupkitDaemon state machine
```

- **Only one external dependency**: `serde_json`. No CLI framework — arg parsing is manual in `commands/mod.rs`.
- `WelcomeSnapshot` is the root aggregate type that composes `SystemSummary`, `AiToolsSummary`, `AiUsageSummary`, and `CopilotUsageSummary`.
- `commands/` modules are intentionally thin: they call a collector, pass the result to a render function, and `print!` the output.

## Key Conventions

### Render functions come in pairs
Every public render function has a `_with_width` sibling used by tests:
```rust
pub fn render_system_summary(summary: &SystemSummary) -> String         // calls resolve_total_width()
fn render_system_summary_with_width(summary: &SystemSummary, w: usize) // deterministic, used in tests
```
Always add both variants when adding a new render function.

### Snapshot tests
Render tests compare against committed snapshot files in `tests/snapshots/*.txt`. They use `include_str!` with `env!("CARGO_MANIFEST_DIR")` at compile time:
```rust
include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots/welcome-wide.txt"))
```
When render output changes intentionally, **manually update the snapshot files** — there is no auto-update mechanism. The test helper `normalize_snapshot` strips trailing whitespace before comparing.

### Fixture loading
Collector tests load fixtures the same way — `include_str!` with `CARGO_MANIFEST_DIR` — via a `fixture_text(name)` helper function inside the `#[cfg(test)]` block. Fixture data lives in `tests/fixtures/`.

### TTY guard
`shell::can_render_welcome()` gates the default `welcome` invocation. An explicit `cargo run -- welcome` bypasses it. This is intentional to support non-TTY environments.

## Collectors: Data Sources

### Claude usage
- Scans `$HOME/.claude/projects/**/*.jsonl` recursively.
- Each JSONL line is a JSON object; token fields (`input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`) are summed into `TokenBreakdown` buckets (24 h / 7 d / lifetime) using file mtime as the timestamp proxy.
- Malformed lines and unreadable files are counted and surfaced as `AiUsageSummary.warnings`.

### Codex usage
- Session data: `$HOME/.codex/sessions/**/*.jsonl` — each session file is scanned for rate-limit events and token counts.
- Plan type: preferred source is a `plan_type` field embedded in a session event; fallback is `$HOME/.codex/auth.json`, which is searched via the multi-path list `AUTH_PLAN_PATHS` (covers several known JWT/claims shapes).
- `AUTH_JWT_TOKEN_PATHS` is used to decode the plan from a raw JWT `id_token` when the flat JSON paths don't match.
- `UsageAvailability` is `Live` when token records exist, `Partial` when only the directory or plan type is present, `Unavailable` otherwise.

### Collector testability pattern
Public collectors accept `home: Option<&Path>` and `now: SystemTime` injected parameters (the `_with_home` / `_with_now` sibling functions). Tests pass a temp-dir root and a fixed `SystemTime` to make results deterministic without touching the real filesystem.

### Copilot usage
- **Primary source**: HTTP GET to a local `copilot-api` service at `http://localhost:{port}/usage` (port defaults to `1414`, configurable via `PUP_COPILOT_API_PORT` env var). The API returns a JSON object with `copilot_plan`, `quota_reset_date`, and `quota_snapshots` containing `premium_interactions`, `chat`, and `completions` entries.
- **Fallback source**: Scans `$HOME/.copilot/session-state/*/events.jsonl` to count session requests and detect the active model.
- Both sources are merged: the API provides quota/plan data; local scan provides request counts and sessions.
- Model types: `CopilotQuotaInfo` (login, plan, reset_date, premium/chat/completions quotas) and `CopilotQuotaEntry` (entitlement, remaining, percent_remaining_x10, unlimited).
- The HTTP fetch uses `curl` with a 5-second timeout; if the API is unreachable, the collector falls back gracefully to local-only data.

## Install Command

`install` copies the current binary to `$HOME/.local/bin/pup` (mode `0o755`) and upserts a managed block into six shell profile files:

| File | Mode |
|---|---|
| `~/.zshrc`, `~/.bashrc`, `~/.config/fish/config.fish` | `Interactive` — adds PATH + auto-invokes `pup` in interactive shells |
| `~/.zprofile`, `~/.bash_profile`, `~/.profile` | `PathOnly` — adds PATH only |

The managed block is delimited by:
```
# >>> pup-cli-start-rust install >>>
# <<< pup-cli-start-rust install <<<
```

`upsert_managed_block` replaces the block in-place if already present, so `install` is **idempotent**. Files that don't exist are created (parent directories included). The fish variant uses fish syntax; all others use POSIX `case`/`$-` guards.

## Governance

This project uses the **周天子百官协作制** (Imperial Court multi-agent) workflow defined in `AGENTS.md`. Key trigger phrases and role assignments are documented there. Documentation artifacts go to `docs/governance/`, `docs/adr/`, `docs/reports/`, and `docs/history/`.

## Daemon

The `pupkit daemon` command starts a background service:

1. **Bind** a Unix socket at `~/.local/share/pupkit/pupkitd.sock`
2. **Start watcher** — polls JSONL files from Claude Code, Codex, and Copilot for session activity
3. **Launch PupkitShell** — macOS only; spawns PupkitShell as detached process
4. **Accept loop** — serves client requests (hooks, UI actions, state snapshots)

Server refactoring: `serve_forever()` is split into `bind()` + `accept_loop()` for orchestration. `daemon_arc()` exposes the shared `Arc<Mutex<PupkitDaemon>>`.

### DaemonConfig

- `socket_path`: Unix socket location
- `state_path`: persisted daemon state JSON
- `shell_binary_path`: resolved PupkitShell binary (macOS only)
  - Resolution order: `PUPKIT_SHELL_PATH` env → sibling of `current_exe()` → None

### Welcome auto-start

Running `pupkit` (no args) renders the welcome screen and then checks if the daemon socket is active. If not, spawns `pupkit daemon` as a detached background process.

## PupkitShell (macOS)

Native SwiftUI overlay at the macOS screen notch (Dynamic Island style). Source: `macos/PupkitShell/`.

### shell_launcher.rs

- `ensure_available()` — returns PupkitShell path; downloads from latest release if not found
- `try_launch()` — spawns PupkitShell as detached process; skips if already running (pgrep check)
- `download_shell()` — downloads release archive and extracts PupkitShell binary + resource bundle
- All functions are `#[cfg(target_os = "macos")]` guarded; no-op on other platforms

### CI Integration

- `release.yml` builds PupkitShell with Swift on macOS runners
- Injects binary + `PupkitShell_PupkitShell.bundle` into cargo-dist tar.xz archives
- Patches Homebrew formula to install PupkitShell alongside pupkit
