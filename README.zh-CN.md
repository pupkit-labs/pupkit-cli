[English](./README.md) | [简体中文](./README.zh-CN.md)

# pupkit 🐾

`pupkit` 是一个以 welcome 为核心的 CLI 工具，用来在终端里快速展示本机环境信息和 AI 使用概览。

它会输出一个精简的欢迎页，包含：

- 公网 IP 和国家信息
- 代理状态
- Claude 用量速览
- Codex 限流速览
- GitHub Copilot 配额速览

当前对外功能面刻意保持很小：

- `pupkit`
- `pupkit welcome`
- `pupkit auth`
- `pupkit update`

## 安装 📦

### Homebrew

```sh
brew install pupkit-labs/tap/pupkit
```

### Shell Installer

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/pupkit-labs/pupkit-cli/releases/latest/download/pupkit-installer.sh | sh
```

### 从源码构建

```sh
cargo build --release
./target/release/pupkit welcome
```

## 终端集成 💻

下面这套配置适用于 Terminal.app、iTerm2、Warp、VS Code Terminal 这类常见终端。

如果你是通过 Homebrew 或 shell installer 安装的，`pupkit` 通常已经在 `PATH` 里，可以直接使用。

如果你是从源码构建，或者想手动安装，可以把二进制放到 `~/.local/bin`：

```sh
mkdir -p ~/.local/bin
cp ./target/release/pupkit ~/.local/bin/pupkit
```

### zsh / bash

把下面这行加到 `~/.zshrc` 或 `~/.bashrc`：

```sh
export PATH="$HOME/.local/bin:$PATH"
```

如果你希望每次打开新的交互终端时自动显示 welcome，把下面这段加到 `~/.zshrc` 或 `~/.bashrc` 文件末尾：

```sh
if command -v pupkit >/dev/null 2>&1; then
  pupkit welcome
fi
```

如果你用的是 zsh，也可以直接复制下面这条命令执行，它会自动把这段内容追加到 `~/.zshrc` 末尾，并尽量避免重复追加：

```sh
grep -Fq '# pupkit welcome' ~/.zshrc 2>/dev/null || printf '\n# pupkit welcome\nif command -v pupkit >/dev/null 2>&1; then\n  pupkit welcome\nfi\n' >> ~/.zshrc
```

### fish

把 `~/.local/bin` 加到 `PATH`：

```fish
fish_add_path $HOME/.local/bin
```

如果你希望每次打开新的交互终端时自动显示 welcome，把下面内容加到 `~/.config/fish/config.fish`：

```fish
if status is-interactive
    and type -q pupkit
    pupkit welcome
end
```

## 快速开始 ⚡

直接渲染欢迎页：

```sh
pupkit
```

或者显式执行：

```sh
pupkit welcome
```

如果你需要重新刷新 GitHub 认证，用来获取 Copilot 配额：

```sh
pupkit auth
```

如果你是通过 shell installer 安装的，并且想更新到最新发布版本：

```sh
pupkit update
```

## 命令 🧭

### `welcome`

渲染主欢迎页。

其中包括：

- ASCII 标题
- 带国家标识的公网 IP
- 代理状态
- Claude、Codex、Copilot 的 AI Quick Look

### `auth`

强制重新走一次 GitHub device flow，并把 token 保存下来，供后续 Copilot 配额请求复用。

### `update`

通过 shell installer 把 `pupkit` 更新到最新 GitHub Release。

如果当前版本已经是最新 release，`update` 会直接退出，不再重复安装。

如果 `pupkit` 是通过 Homebrew 安装的，请改用 `brew upgrade pupkit`。

## 认证 🔐

Copilot 配额由程序直接请求 GitHub：

- API：`https://api.github.com/copilot_internal/user`
- token 缓存：`~/.local/share/pupkit/github_token`

token 读取顺序：

1. `PUP_GITHUB_TOKEN`
2. `GITHUB_TOKEN`
3. `GH_TOKEN`
4. `~/.local/share/pupkit/github_token`

如果你希望 `welcome` 在没有 token 时自动进入 device flow，需要在交互终端中这样运行：

```sh
PUP_COPILOT_DEVICE_AUTH=1 pupkit welcome
```

## 环境变量 🌍

- `PUP_GITHUB_TOKEN`：优先使用的 GitHub token
- `GITHUB_TOKEN`：回退 GitHub token
- `GH_TOKEN`：回退 GitHub token
- `PUP_COPILOT_DEVICE_AUTH=1`：允许 `welcome` 在需要时进入 GitHub device flow
- `PUP_PROXY_TUN_ADDR`：可选的 `host:port`，用于代理 / TUN 探测

## 开发 🛠️

本地运行：

```sh
cargo run
```

运行测试：

```sh
cargo test
```

校验发布配置：

```sh
dist manifest --output-format=json --no-local-paths
```

## 发布 🚀

当前发布产物通过以下渠道分发：

- GitHub Releases：`https://github.com/pupkit-labs/pupkit-cli/releases`
- Homebrew tap：`pupkit-labs/homebrew-tap`
