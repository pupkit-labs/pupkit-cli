# PupkitShell

Thin native macOS shell for the Rust `pupkitd` core.

## Current scope

- `NSStatusItem` menu bar entry
- polling-based IPC client for `StateSnapshot`
- menu bar action routing for approve / deny / answer-option
- notch panel scaffold with action callbacks
- intentionally thin business logic: renders Rust-produced state only

## Build (on macOS)

```bash
cd macos/PupkitShell
swift build
swift run PupkitShell
```

The shell expects the Rust daemon socket at:

```text
~/.local/share/pupkit/pupkitd.sock
```

## Suggested local test flow

In terminal 1:

```bash
cd ~/liupx_git/pupkit-labs/pupkit-cli
cargo run -- daemon
```

In terminal 2, send a simulated blocking request:

```bash
printf '%s' '{"session_id":"demo-shell","hook_event_name":"PermissionRequest","tool_name":"Edit","tool_input":{"path":"src/lib.rs"},"title":"demo shell approval"}' | cargo run -- bridge claude
```

Then run the shell:

```bash
cd macos/PupkitShell
swift run PupkitShell
```
