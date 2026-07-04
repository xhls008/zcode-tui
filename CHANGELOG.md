# Changelog

每个版本都会发布对应的 GitHub Release：打 `v*` tag 后 CI 跑全部质量门禁，
构建 x86_64-musl 静态 Linux 二进制（无需 Rust 工具链即可使用），连同
SHA256SUMS 和 install.sh 一起挂到 Release，notes 取自本文件对应版本段。

## [0.2.0] - 2026-07-04

准流式进度与认证体验(openspec 变更 db-live-progress-and-auth-screen,
基于设计文档 §5.1 的流式 spike 实测)。

### 新增

- **实时工具进度**:prompt 运行期间只读轮询内核会话库(`db.sqlite`),
  工具调用实时渲染为 chip(运行中 spinner → 完成 ✓ + 耗时 / 失败 ✗),
  最新 reasoning 以 dim 单行显示在工作区;仅运行时显示,turn 结束清场,
  不进 transcript;schema 不识别 / 文件缺失 / 读取失败一律整组降级,
  行为回到纯 spinner
- **上下文水位**:prompt 通道改用 `--json` 整块总结对象为权威结果
  (response 走 markdown,解析失败自动纯文本降级),状态栏显示
  `ctx 9k/200k (4%)`,≥80% 提示 /new
- **未登录启动屏**:清华紫 ZCODE 字标(块体亮紫 + 描边暗紫阴影层)、
  底部鸟巢/长城/天坛轮廓、三条免浏览器登录路径引导;NO_COLOR 全退化
- `/login` 在无桌面环境(无 DISPLAY/WAYLAND_DISPLAY)自动附 `--no-browser`

### 修复

- `/auth` 三态检测:内核硬性要求 `~/.zcode/cli/config.json`,仅有
  env API key 时如实显示"部分配置"并给补齐命令,不再误报已认证

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
- 内核要求 `~/.zcode/cli/config.json` 模型配置，仅设 env API key 不够；
  免浏览器登录用 `zcode login <zai|bigmodel>-coding-plan-api-key <key>`
  或 `zcode login --no-browser`（详见 README 的 SSH / 无桌面环境一节）
- 仅 Linux x86_64；macOS / Windows 见设计文档第 7 节移植计划
