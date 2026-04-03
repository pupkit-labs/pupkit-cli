# pupkit

把当前基于 Zsh 的欢迎页脚本能力沉淀为可维护、可测试、可发布的 Rust CLI。

当前仓库已经完成 welcome-only 收口，当前只保留欢迎页主入口与 GitHub 认证入口。

当前仓库已收口为只保留 welcome 欢迎页，可直接运行：

- `cargo run`
- `cargo run -- welcome`
- `cargo run -- auth`

当前 Rust 版本已覆盖：

- 默认欢迎页（精简版）：ASCII 标题 + 网络 IP/Proxy + AI 快速一览（Claude model/24h/7d · Codex model/5h limit/weekly limit · Copilot plan/quota/sessions）
- 默认执行与显式 `welcome` 都会走同一条欢迎页链路；另提供 `auth` 用于强制重新执行 GitHub device flow
- 欢迎页已接入公网 IP 的超时查询与缓存回退、国家 emoji 展示，以及支持 TTY 的 Proxy 状态色块
- Claude / Codex / Copilot 的本地摘要采集仍保留，用于组装欢迎页中的 AI Quick Look
- Copilot quota 现由 Rust 直接请求 `https://api.github.com/copilot_internal/user`；优先复用 `PUP_GITHUB_TOKEN` / `GITHUB_TOKEN` / `GH_TOKEN`，其次复用 `~/.local/share/pupkit/github_token`
- 所有文件缓存统一写入 `~/.local/share/pupkit/github_token`
- 如需在欢迎页链路里按需触发认证，可在交互终端下设置 `PUP_COPILOT_DEVICE_AUTH=1` 后运行 welcome；如需强制重新认证，直接运行 `cargo run -- auth`
- 仓库内保留 welcome 宽终端 / 窄终端 snapshot 回归

发布与分发：

- GitHub Releases：`https://github.com/pupkit-labs/pupkit-cli/releases`
- Homebrew tap：`brew install pupkit-labs/tap/pupkit`
- shell installer：

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/pupkit-labs/pupkit-cli/releases/latest/download/pupkit-installer.sh | sh
```

发布前需要在 `pupkit-labs/pupkit-cli` 配置 GitHub Actions secret：

- `HOMEBREW_TAP_TOKEN`

它应当是一个对 `pupkit-labs/homebrew-tap` 具有 `Contents: Read and write` 权限的 token。

当前仍待补齐：

- 更细的平台兼容清单与更多真实环境 fixture
