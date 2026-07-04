# Changelog

每个版本都会发布对应的 GitHub Release：打 `v*` tag 后 CI 跑全部质量门禁，
构建 x86_64-musl 静态 Linux 二进制（无需 Rust 工具链即可使用），连同
SHA256SUMS 和 install.sh 一起挂到 Release，notes 取自本文件对应版本段。

## [0.1.0] - 2026-07-04

首个发布版本：官方 ZCode 桌面包缺 `@zcode/tui` 期间的终端 TUI fallback，
第一使用场景是 SSH 无桌面环境。

### 核心

- Codex 范式终端界面：无边框流式 transcript、GLM 蓝单一强调色、
  markdown + syntect 语法高亮、diff 着色、显示宽度对齐的表格、CJK 感知折行
- 流式任务模型：prompt / shell / diff 走独立进程组子进程，按行实时输出，
  Esc/Ctrl+C 取消（killpg 无残留），忙时输入自动排队
- 会话连续性：首条 prompt 后自动 `--continue`；`/new` `/resume [sess_id]`
  `/mode`（Shift+Tab 循环）；欢迎框实时显示会话状态
- `/mcp` 配置管理：project/user 两级 scope，stdio/http/sse 三种 transport，
  `.mcp.json` 与 Claude Code 格式兼容
- `@文件` 提及（canonicalize 越界防护）、`/` 补全菜单、Ctrl+P 命令面板、
  Ctrl+X leader、`! cmd` shell escape、`$EDITOR` 长文编辑
- 认证：`/auth` 检测 env key 链与凭证文件（打码显示）、`/login` `/logout`
- 启动探测：内核版本、桌面包版本、官方更新 feed（与桌面端同渠道）提示

### SSH / 无桌面支持

- wrapper：Electron 因缺桌面库起不动时自动回退系统 Node（≥ 22.5，内核依赖
  `node:sqlite`），`ZCODE_FORCE_SYSTEM_NODE=1` 强制，均不可用时报错并给指引
- install.sh 新增 `--no-build`：配合 Release 二进制在无 cargo 的机器上
  只生成 wrapper
- README 新增 SSH / 无桌面环境指引（deb 免 root 解包、两条免浏览器登录路径）

### 已知限制

- headless `--prompt` 默认 yolo，edit/plan 模式的工具确认暂无人接住
  （需要内核 app-server 协议，按需评估）
- `zcode login` 的 OAuth 流程在无桌面机器上未实测；推荐 env API key
  或拷贝 `~/.zcode/v2/credentials.json`
- 仅 Linux x86_64；macOS / Windows 见设计文档第 7 节移植计划
