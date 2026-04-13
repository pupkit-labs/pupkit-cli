# Changelog

All notable changes to this project will be documented in this file.

## 1.0.1 - 2026-04-13

### Added / 新增

- **Shell watchdog**: daemon now monitors PupkitShell and automatically restarts it if it exits. / **Shell 守护**：daemon 现在监控 PupkitShell，如果退出会自动重启。
- **Daemon management**: `pupkit daemon [start|stop|restart|status]` subcommands for lifecycle control. / **Daemon 管理**：新增 `pupkit daemon [start|stop|restart|status]` 子命令用于生命周期控制。
- **Shell management**: `pupkit shell [start|stop|restart|status]` subcommands for PupkitShell control. / **Shell 管理**：新增 `pupkit shell [start|stop|restart|status]` 子命令用于 PupkitShell 控制。
- **Unified lifecycle**: `pupkit start|stop|restart|status` top-level commands manage daemon + shell together; `start` spawns daemon in background. / **统一生命周期**：新增 `pupkit start|stop|restart|status` 顶级命令，同时管理 daemon 和 shell；`start` 在后台启动 daemon。
- **PID file**: daemon writes `~/.local/share/pupkit/pupkitd.pid` for reliable process management. / **PID 文件**：daemon 写入 PID 文件以支持可靠的进程管理。

### Changed / 改进

- `pupkit daemon` (no subcommand) is backward-compatible, equivalent to `pupkit daemon start`. / `pupkit daemon`（无子命令）向后兼容，等同于 `pupkit daemon start`。

## 1.0.0 - 2026-04-13

### Added / 新增

- **PupkitShell bundling**: daemon auto-discovers and launches PupkitShell (macOS Dynamic Island overlay) on startup. / **PupkitShell 绑定**：daemon 启动时自动发现并启动 PupkitShell（macOS 灵动岛叠加层）。
- **Auto-download**: if PupkitShell is not found locally, daemon downloads it from the latest GitHub release. / **自动下载**：如果本地找不到 PupkitShell，daemon 会从最新 GitHub release 自动下载。
- **Welcome auto-starts daemon**: running `pupkit` (no args) now auto-starts the daemon in background if not already running. / **Welcome 自动启动 daemon**：运行 `pupkit`（无参数）时，如果 daemon 未在运行，会自动在后台启动。
- **`pupkit update` includes PupkitShell**: on macOS, the update command also downloads PupkitShell from the release archive. / **`pupkit update` 包含 PupkitShell**：macOS 上 update 命令同时从 release archive 下载 PupkitShell。
- **CI integration**: release workflow builds PupkitShell with Swift and injects it into macOS archives; Homebrew formula patched to install PupkitShell. / **CI 集成**：发布流程使用 Swift 构建 PupkitShell 并注入 macOS archive；Homebrew formula 自动 patch 以安装 PupkitShell。

### Changed / 改进

- Refactored `serve_forever()` into `bind()` + `accept_loop()` for orchestration. / 将 `serve_forever()` 拆分为 `bind()` + `accept_loop()` 以支持编排逻辑。
- `DaemonConfig` now includes `shell_binary_path` with auto-resolution (env var → sibling binary). / `DaemonConfig` 新增 `shell_binary_path`，支持自动解析（环境变量 → 同目录二进制）。

### Docs / 文档

- Updated README (EN/CN) with daemon, PupkitShell, and auto-start documentation. / 更新中英文 README，新增 daemon、PupkitShell 和自动启动文档。
- Expanded copilot-instructions.md with daemon architecture and shell_launcher docs. / 扩展 copilot-instructions.md，增加 daemon 架构和 shell_launcher 文档。

## 0.0.6 - 2026-04-07

### Changed / 改进

- Added a visual progress bar and remaining percentage text to GitHub Copilot quota rows in the welcome view. / 为 welcome 页面中的 GitHub Copilot quota 行增加可视化进度条和剩余百分比文本。

### Testing / 测试

- Added renderer tests covering the new Copilot quota formatting and welcome output. / 为新的 Copilot quota 格式和 welcome 输出补充渲染测试。

## 0.0.5 - 2026-04-05

### Changed / 改进

- Made `pupkit update` check the latest published GitHub Release before reinstalling. / 调整 `pupkit update`，在重新安装前先检查 GitHub 上的最新已发布版本。
- Added an explicit "already up to date" fast path to avoid reinstalling the same version. / 新增“已经是最新版本”的快速退出逻辑，避免重复安装同一版本。

### Docs / 文档

- Clarified in the READMEs that `update` exits early when the current version already matches the latest release. / 在 README 中补充说明：若当前版本已经匹配最新 release，`update` 会直接退出。

## 0.0.4 - 2026-04-05

### Added / 新增

- Added `pupkit update` to update shell-installer installs to the latest GitHub Release. / 新增 `pupkit update` 命令，用于把通过 shell installer 安装的版本更新到最新 GitHub Release。
- Added install-source detection so Homebrew installs are redirected to `brew upgrade pupkit` and source-build paths are rejected with a clear message. / 新增安装来源识别，对 Homebrew 安装明确提示使用 `brew upgrade pupkit`，对源码构建路径给出清晰提示并拒绝直接更新。
- Added command parsing and tests for the new `update` command. / 为 `update` 命令补充了解析逻辑和测试覆盖。

### Docs / 文档

- Documented the new `update` command in both English and Chinese READMEs. / 在中英文 README 中补充了 `update` 命令说明。

## 0.0.3 - 2026-04-05

### Security / 安全

- Hardened Copilot token handling so GitHub tokens no longer appear in `curl` command-line arguments. / 加固 Copilot token 处理流程，避免 GitHub token 出现在 `curl` 命令行参数中。
- Tightened local token cache permissions to `0700` for directories and `0600` for files. / 收紧本地 token 缓存权限，将目录设为 `0700`、文件设为 `0600`。

### Testing / 测试

- Added tests covering the security-sensitive Copilot token handling changes. / 为 Copilot token 的安全改动补充了测试覆盖。

## 0.0.2 - 2026-04-03

### Changed / 改进

- Polished the `welcome` screen rendering and refined the overall layout. / 优化 `welcome` 页面的终端渲染效果和整体布局。
- Added gradient styling to the title and improved AI Quick Look table styling. / 为标题加入渐变色效果，并优化 AI Quick Look 表格样式。
- Improved rendering stability across different terminal widths. / 改进不同终端宽度下的显示稳定性。

## 0.0.1 - 2026-04-03

### Added / 新增

- Initial public release of `pupkit`, a welcome-first terminal CLI for quickly surfacing local environment details and AI usage at a glance. / `pupkit` 首个公开版本发布，提供以 `welcome` 为核心的终端首页体验，用于快速查看本机环境和 AI 使用概览。
- Added shell setup guidance for zsh, bash, and fish, including optional auto-run integration. / 补充 zsh、bash、fish 的 shell 配置指引，并提供可选的自动运行集成说明。
