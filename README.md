[English](./README.md) | [简体中文](./README.zh-CN.md)

# pupkit 🐾

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
- `pupkit update`

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

Or explicitly:

```sh
pupkit welcome
```

If you need to refresh GitHub authentication for Copilot quota lookup:

```sh
pupkit auth
```

If you installed via the shell installer and want to update to the latest release:

```sh
pupkit update
```

## Commands 🧭

### `welcome`

Render the main welcome screen.

This includes:

- ASCII title
- public IP with country marker
- proxy status
- AI Quick Look for Claude, Codex, and Copilot

### `auth`

Force a fresh GitHub device flow and store the resulting token for later Copilot quota requests.

### `update`

Update `pupkit` to the latest GitHub Release via the shell installer.

If `pupkit` was installed with Homebrew, use `brew upgrade pupkit` instead.

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
