[English](./README.md) | [简体中文](./README.zh-CN.md)

# pupkit 🐾

`pupkit` is a welcome-first CLI for surfacing local environment info and AI usage at a glance — with a native macOS Dynamic Island companion.

It is designed to render a compact terminal welcome screen with:

- public IP and country
- proxy status
- Claude usage quick look
- Codex rate-limit quick look
- GitHub Copilot quota quick look

On macOS, pupkit also provides **PupkitShell** — a native SwiftUI overlay at the screen notch (Dynamic Island) that shows live AI session status, approval requests, and tool activity.

The current product surface:

- `pupkit` — welcome screen + auto-starts daemon in background
- `pupkit welcome` — explicit welcome screen
- `pupkit start|stop|restart|status` — unified service lifecycle (daemon + shell)
- `pupkit daemon [start|stop|restart|status]` — daemon-only management
- `pupkit shell [start|stop|restart|status]` — PupkitShell-only management (macOS)
- `pupkit auth` — GitHub device flow for Copilot quota
- `pupkit update` — self-update (includes PupkitShell on macOS)

## Install 📦

### Homebrew

```sh
brew install pupkit-labs/tap/pupkit
```

### Shell Installer

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/pupkit-labs/pupkit-cli/releases/latest/download/pupkit-installer.sh | sh
```

### Build From Source

```sh
cargo build --release
./target/release/pupkit welcome
```

## Shell Setup 💻

The setup below works for common terminals such as Terminal.app, iTerm2, Warp, and VS Code Terminal.

If you installed `pupkit` via Homebrew or the shell installer, it is typically available on `PATH` already.

If you built from source or want a manual setup, place the binary in `~/.local/bin`:

```sh
mkdir -p ~/.local/bin
cp ./target/release/pupkit ~/.local/bin/pupkit
```

### zsh / bash

Add this to `~/.zshrc` or `~/.bashrc`:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

If you want a new interactive shell to render the welcome screen automatically, add this to the end of `~/.zshrc` or `~/.bashrc`:

```sh
if command -v pupkit >/dev/null 2>&1; then
  pupkit welcome
fi
```

If you use zsh and want a copy-paste command that appends the block automatically:

```sh
grep -Fq '# pupkit welcome' ~/.zshrc 2>/dev/null || printf '\n# pupkit welcome\nif command -v pupkit >/dev/null 2>&1; then\n  pupkit welcome\nfi\n' >> ~/.zshrc
```

### fish

Add `~/.local/bin` to `PATH`:

```fish
fish_add_path $HOME/.local/bin
```

If you want a new interactive shell to render the welcome screen automatically, add this to `~/.config/fish/config.fish`:

```fish
if status is-interactive
    and type -q pupkit
    pupkit welcome
end
```

## Quick Start ⚡

Render the welcome screen:

```sh
pupkit
```

This also auto-starts the daemon in the background. On macOS, the daemon launches PupkitShell (Dynamic Island overlay) automatically.

Or explicitly:

```sh
pupkit welcome
```

### Service Management

Start or stop everything (daemon + PupkitShell) at once:

```sh
pupkit start     # start daemon in background + shell
pupkit stop      # stop daemon + shell
pupkit restart   # restart both
pupkit status    # show running state
```

Manage daemon and shell individually:

```sh
pupkit daemon start|stop|restart|status
pupkit shell start|stop|restart|status
```

> **Note**: Quitting PupkitShell from the macOS menu bar stops both shell and daemon. Use `pupkit start` to bring them back.

If you need to refresh GitHub authentication for Copilot quota lookup:

```sh
pupkit auth
```

If you installed via the shell installer and want to update to the latest release:

```sh
pupkit update
```

## Commands 🧭

### `start` / `stop` / `restart` / `status`

Unified lifecycle management for the entire pupkit service (daemon + PupkitShell).

- `pupkit start` — starts daemon in background; on macOS also launches PupkitShell with watchdog
- `pupkit stop` — stops both daemon and PupkitShell
- `pupkit restart` — restarts both
- `pupkit status` — shows running status of daemon and shell

### `daemon [start|stop|restart|status]`

Manage the background daemon independently. The daemon:

- Binds a Unix socket at `~/.local/share/pupkit/pupkitd.sock`
- Writes PID to `~/.local/share/pupkit/pupkitd.pid`
- Watches JSONL files from Claude Code, Codex, and Copilot for session activity
- Receives bridge events from AI tool hooks
- On macOS, auto-discovers and launches **PupkitShell** (Dynamic Island overlay)
- Runs a **watchdog thread** that restarts PupkitShell if it crashes (checks every 10s)
- If PupkitShell is not found locally, downloads it from the latest GitHub release

`pupkit daemon` (no subcommand) is equivalent to `pupkit daemon start` (runs in foreground).

### `shell [start|stop|restart|status]` (macOS only)

Manage PupkitShell independently:

- `pupkit shell start` — launches PupkitShell and re-enables the daemon watchdog
- `pupkit shell stop` — stops PupkitShell and pauses the watchdog (won't auto-restart)
- `pupkit shell restart` — restarts PupkitShell
- `pupkit shell status` — shows whether PupkitShell is running

### `welcome`

Render the main welcome screen.

This includes:

- ASCII title
- public IP with country marker
- proxy status
- AI Quick Look for Claude, Codex, and Copilot

Also auto-starts the daemon in background if not already running.

### `auth`

Force a fresh GitHub device flow and store the resulting token for later Copilot quota requests.

### `update`

Update `pupkit` to the latest GitHub Release via the shell installer.

If the current version is already the latest release, `update` exits without reinstalling.

If `pupkit` was installed with Homebrew, use `brew upgrade pupkit` instead.

On macOS, `update` also downloads PupkitShell from the release archive.

## Authentication 🔐

Copilot quota is fetched directly from GitHub:

- API: `https://api.github.com/copilot_internal/user`
- token cache: `~/.local/share/pupkit/github_token`

Token lookup order:

1. `PUP_GITHUB_TOKEN`
2. `GITHUB_TOKEN`
3. `GH_TOKEN`
4. `~/.local/share/pupkit/github_token`

If you want the `welcome` path to trigger device flow when no token is available, run it in an interactive terminal with:

```sh
PUP_COPILOT_DEVICE_AUTH=1 pupkit welcome
```

## Environment Variables 🌍

- `PUP_GITHUB_TOKEN`: preferred GitHub token for Copilot quota requests
- `GITHUB_TOKEN`: fallback GitHub token
- `GH_TOKEN`: fallback GitHub token
- `PUP_COPILOT_DEVICE_AUTH=1`: allow `welcome` to enter GitHub device flow when needed
- `PUP_PROXY_TUN_ADDR`: optional `host:port` override used for proxy/TUN detection
- `PUPKIT_SHELL_PATH`: override path to PupkitShell binary (macOS only)
- `PUP_COPILOT_API_PORT`: port for copilot-api service (default: 1414)

## PupkitShell — macOS Dynamic Island 🏝️

On macOS, pupkit includes **PupkitShell**, a native SwiftUI overlay that appears at the screen notch area (Dynamic Island style). It shows:

- Active AI coding sessions (Claude, Codex, Copilot) with tool-specific pixel art icons
- Approval requests that need your attention
- Session activity and tool execution status
- Usage metrics per session

PupkitShell is automatically:
- **Bundled** in Homebrew and release archives
- **Downloaded** on first `pupkit daemon` if not present
- **Launched** when the daemon starts (skipped if already running)

The source lives in `macos/PupkitShell/` (Swift Package Manager project, requires macOS 14+).

## Development 🛠️

Run locally:

```sh
cargo run
```

Run tests:

```sh
cargo test
```

Validate release config:

```sh
dist manifest --output-format=json --no-local-paths
```

## Release 🚀

Release artifacts are published through:

- GitHub Releases: `https://github.com/pupkit-labs/pupkit-cli/releases`
- Homebrew tap: `pupkit-labs/homebrew-tap`
