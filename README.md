# pupkit

`pupkit` is a welcome-first CLI for surfacing local environment info and AI usage at a glance.

It is designed to render a compact terminal welcome screen with:

- public IP and country
- proxy status
- Claude usage quick look
- Codex rate-limit quick look
- GitHub Copilot quota quick look

The current product surface is intentionally small:

- `pupkit`
- `pupkit welcome`
- `pupkit auth`

## Install

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

## Quick Start

Render the welcome screen:

```sh
pupkit
```

Or explicitly:

```sh
pupkit welcome
```

If you need to refresh GitHub authentication for Copilot quota lookup:

```sh
pupkit auth
```

## Commands

### `welcome`

Render the main welcome screen.

This includes:

- ASCII title
- public IP with country marker
- proxy status
- AI Quick Look for Claude, Codex, and Copilot

### `auth`

Force a fresh GitHub device flow and store the resulting token for later Copilot quota requests.

## Authentication

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

## Environment Variables

- `PUP_GITHUB_TOKEN`: preferred GitHub token for Copilot quota requests
- `GITHUB_TOKEN`: fallback GitHub token
- `GH_TOKEN`: fallback GitHub token
- `PUP_COPILOT_DEVICE_AUTH=1`: allow `welcome` to enter GitHub device flow when needed
- `PUP_PROXY_TUN_ADDR`: optional `host:port` override used for proxy/TUN detection

## Development

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

## Release

Release artifacts are published through:

- GitHub Releases: `https://github.com/pupkit-labs/pupkit-cli/releases`
- Homebrew tap: `pupkit-labs/homebrew-tap`

To publish Homebrew formula updates from GitHub Actions, the repository `pupkit-labs/pupkit-cli` must define:

- `HOMEBREW_TAP_TOKEN`

This token should have write access to `pupkit-labs/homebrew-tap`.

## Status

`pupkit` is currently scoped to a minimal welcome-only CLI. The data collectors remain only where they are needed to render the welcome screen.
