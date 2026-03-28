# pup-cli-start-rust

把当前基于 Zsh 的欢迎页脚本能力沉淀为可维护、可测试、可发布的 Rust CLI。

当前仓库已经完成一轮文档化梳理，分析对象为 `~/.zsh_liupx_welcome.sh`。需求、现状实现和后续 Rust 化计划见 [docs/README.md](docs/README.md)。

当前仓库已完成 Rust 工程初始化，可先运行：

- `cargo run`
- `cargo run -- welcome`
- `cargo run -- system-summary`
- `cargo run -- ai-tools`

当前 Rust 版本已覆盖：

- 欢迎页标题与基础终端退化判断
- 本地 `System Summary` 采集与盒状表格渲染
- Claude / Codex 模型与 skills 摘要，以及 `ai-tools` 子命令

当前仍待补齐：

- 公网 IP 采集与缓存
- 服务状态查看
- shell 输出 fixtures 与 snapshot 回归基线
