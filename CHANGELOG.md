# Changelog

All notable changes to this project will be documented in this file.

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
