# PupkitShell

Thin native macOS shell for the Rust `pupkitd` core.

## Current scope

- `NSStatusItem` menu bar entry
- polling-based IPC client for `StateSnapshot`
- notch panel scaffold with SwiftUI content
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
