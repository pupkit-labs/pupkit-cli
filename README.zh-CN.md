[English](./README.md) | [简体中文](./README.zh-CN.md)

# pupkit 🐾

`pupkit` 是一个以 welcome 为核心的 CLI 工具，用来在终端里快速展示本机环境信息和 AI 使用概览——并附带原生 macOS 灵动岛伴侣界面。

它会输出一个精简的欢迎页，包含：

- 公网 IP 和国家信息
- 代理状态
- Claude 用量速览
- Codex 限流速览
- GitHub Copilot 配额速览

在 macOS 上，pupkit 还提供 **PupkitShell** —— 一个原生 SwiftUI 叠加层，显示在屏幕刘海区域（灵动岛风格），实时展示 AI 会话状态、审批请求和工具活动。

当前对外功能面：

- `pupkit` —— 欢迎页 + 后台自动启动 daemon
- `pupkit welcome` —— 显式渲染欢迎页
- `pupkit start|stop|restart|status` —— 统一服务生命周期管理（daemon + shell）
- `pupkit daemon [start|stop|restart|status]` —— 单独管理 daemon
- `pupkit shell [start|stop|restart|status]` —— 单独管理 PupkitShell（仅 macOS）
- `pupkit auth` —— GitHub 设备流认证
- `pupkit update` —— 自更新（macOS 同时更新 PupkitShell）

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

这同时会在后台自动启动 daemon。在 macOS 上，daemon 会自动启动 PupkitShell（灵动岛叠加层）。

或者显式执行：

```sh
pupkit welcome
```

### 服务管理

一键管理 daemon + PupkitShell：

```sh
pupkit start     # 后台启动 daemon + shell
pupkit stop      # 停止 daemon + shell
pupkit restart   # 重启
pupkit status    # 查看运行状态
```

也可以单独管理 daemon 或 shell：

```sh
pupkit daemon start|stop|restart|status
pupkit shell start|stop|restart|status
```

> **注意**：从 macOS 菜单栏点击 PupkitShell 退出时，daemon 也会一起停止。使用 `pupkit start` 可以重新启动。

如果你需要重新刷新 GitHub 认证，用来获取 Copilot 配额：

```sh
pupkit auth
```

如果你是通过 shell installer 安装的，并且想更新到最新发布版本：

```sh
pupkit update
```

## 命令 🧭

### `start` / `stop` / `restart` / `status`

pupkit 服务的统一生命周期管理（daemon + PupkitShell）。

- `pupkit start` —— 后台启动 daemon；macOS 上同时启动 PupkitShell 并开启守护
- `pupkit stop` —— 停止 daemon 和 PupkitShell
- `pupkit restart` —— 重启
- `pupkit status` —— 查看 daemon 和 shell 运行状态

### `daemon [start|stop|restart|status]`

单独管理后台 daemon。daemon 的功能：

- 绑定 Unix socket 到 `~/.local/share/pupkit/pupkitd.sock`
- 写入 PID 文件到 `~/.local/share/pupkit/pupkitd.pid`
- 监听 Claude Code、Codex、Copilot 的 JSONL 文件以发现会话活动
- 接收来自 AI 工具 hook 的 bridge 事件
- 在 macOS 上自动发现并启动 **PupkitShell**（灵动岛叠加层）
- 运行**守护线程**，PupkitShell 崩溃后每 10s 检测并自动重启
- 如果 PupkitShell 本地不存在，会从最新 GitHub Release 自动下载

`pupkit daemon`（无子命令）等同于 `pupkit daemon start`（前台运行）。

### `shell [start|stop|restart|status]`（仅 macOS）

单独管理 PupkitShell：

- `pupkit shell start` —— 启动 PupkitShell 并重新启用守护线程
- `pupkit shell stop` —— 停止 PupkitShell 并暂停守护线程（不会自动重启）
- `pupkit shell restart` —— 重启 PupkitShell
- `pupkit shell status` —— 查看 PupkitShell 是否在运行

### `welcome`

渲染主欢迎页。

其中包括：

- ASCII 标题
- 带国家标识的公网 IP
- 代理状态
- Claude、Codex、Copilot 的 AI Quick Look

同时会在后台自动启动 daemon（如果尚未运行）。

### `auth`

强制重新走一次 GitHub device flow，并把 token 保存下来，供后续 Copilot 配额请求复用。

### `update`

通过 shell installer 把 `pupkit` 更新到最新 GitHub Release。

如果当前版本已经是最新 release，`update` 会直接退出，不再重复安装。

如果 `pupkit` 是通过 Homebrew 安装的，请改用 `brew upgrade pupkit`。

在 macOS 上，`update` 还会从 release archive 中下载 PupkitShell。

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
- `PUPKIT_SHELL_PATH`：指定 PupkitShell 二进制路径（仅 macOS）
- `PUP_COPILOT_API_PORT`：copilot-api 服务端口（默认：1414）

## PupkitShell — macOS 灵动岛 🏝️

在 macOS 上，pupkit 内含 **PupkitShell**，一个原生 SwiftUI 叠加层，显示在屏幕刘海区域（灵动岛风格），内容包括：

- 活跃的 AI 编码会话（Claude、Codex、Copilot）及工具专属像素图标
- 需要你关注的审批请求
- 会话活动和工具执行状态
- 各会话的用量指标

PupkitShell 会自动：
- 通过 Homebrew 和 release archive **打包分发**
- 首次 `pupkit daemon` 启动时如果不存在则**自动下载**
- daemon 启动时**自动拉起**（已在运行时跳过）

源码位于 `macos/PupkitShell/`（Swift Package Manager 项目，需要 macOS 14+）。

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
