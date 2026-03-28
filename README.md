# pup-cli-start-rust

把当前基于 Zsh 的欢迎页脚本能力沉淀为可维护、可测试、可发布的 Rust CLI。

当前仓库已经完成一轮文档化梳理，分析对象为 `~/.zsh_liupx_welcome.sh`。需求、现状实现和后续 Rust 化计划见 [docs/README.md](docs/README.md)。

当前仓库已完成 Rust 工程初始化，可先运行：

- `cargo run`
- `cargo run -- welcome`
- `cargo run -- system-summary`
- `cargo run -- ai-tools`
- `cargo run -- ai-tools --skills`
- `cargo run -- ai-usage`
- `cargo run -- services`

当前 Rust 版本已覆盖：

- 欢迎页标题与基础终端退化判断；显式 `welcome` 子命令可在非 TTY 下渲染
- 本地 `System Summary` 采集与盒状表格渲染，包括公网 IP 的超时查询与缓存回退
- 默认 `welcome` 与 `ai-tools` 均展示 Claude / Codex 的合并 AI 摘要：模型、计划类型、最近活跃时间、24h/7d/累计或会话 token，以及更直观的 Codex context/rate-limit 信息
- `ai-tools --skills` 可单独查看 Claude / Codex skills；skills 探测逻辑保留但不再默认展示
- `ai-usage` 继续提供显式 AI 用量摘要；Claude 走本地 JSONL token 聚合，Codex 走 session JSONL / rate limit / plan 类型白名单读取
- 本地 `services` 子命令，当前已覆盖 Linux systemd / SysV 与 macOS brew 的采集分支
- 仓库内确定性 render fixture，以及宽终端 / 窄终端 snapshot 回归
- 参考 shell 原型的宽终端 / 窄终端 welcome 与 Linux services fixture

当前仍待补齐：

- 更细的 shell 字段级兼容清单与更多平台 fixture
