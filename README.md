# pup-cli-start-rust

把当前基于 Zsh 的欢迎页脚本能力沉淀为可维护、可测试、可发布的 Rust CLI。

当前仓库已经完成一轮文档化梳理，分析对象为 `~/.zsh_liupx_welcome.sh`。需求、现状实现和后续 Rust 化计划见 [docs/README.md](docs/README.md)。

当前仓库已完成 Rust 工程初始化，可先运行：

- `cargo run`
- `cargo run -- welcome`
- `cargo run -- details`
- `cargo run -- system-summary`
- `cargo run -- ai-tools`
- `cargo run -- ai-tools --skills`
- `cargo run -- ai-usage`
- `cargo run -- install`
- `cargo run -- services`

当前 Rust 版本已覆盖：

- 默认欢迎页（精简版）：ASCII 标题 + 网络 IP/Proxy + AI 快速一览（Claude model/24h/7d · Codex model/5h limit/weekly limit）+ Copilot model/用量占位；TTY 退化判断与显式 `welcome` 命令均已就绪
- `details` 子命令：完整 System Summary + AI Tools + AI Skills + AI Usage + Services 一次性输出
- 本地 `System Summary` 采集与盒状表格渲染，包括公网 IP 的超时查询与缓存回退、国家 emoji 展示，以及支持 TTY 的 Proxy 状态色块
- `ai-tools` 展示 Claude / Codex 的合并 AI 摘要：模型、计划类型、最近活跃时间、24h/7d/累计或会话 token，以及拆分为独立行的 Codex context / 5h / weekly limit 信息
- `ai-tools --skills` 可单独查看 Claude / Codex skills
- `ai-usage` 提供显式 AI 用量摘要；Claude 走本地 JSONL token 聚合，Codex 走 session JSONL / rate limit / plan 类型白名单读取
- `install` 可把当前二进制安装到 `~/.local/bin/pup`，并把 zsh / bash / fish 的自动启动钩子与常见 login profile 一并写入
- 本地 `services` 子命令，当前已覆盖 Linux systemd / SysV 与 macOS brew 的采集分支
- 仓库内确定性 render fixture，以及宽终端 / 窄终端 snapshot 回归
- 参考 shell 原型的宽终端 / 窄终端 welcome 与 Linux services fixture

当前仍待补齐：

- 更细的 shell 字段级兼容清单与更多平台 fixture
