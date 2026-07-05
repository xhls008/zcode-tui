# Changelog

每个版本都会发布对应的 GitHub Release：打 `v*` tag 后 CI 跑全部质量门禁，
构建 x86_64-musl 静态 Linux 二进制（无需 Rust 工具链即可使用），连同
SHA256SUMS 和 install.sh 一起挂到 Release，notes 取自本文件对应版本段。

## [0.3.2] - 2026-07-05

### 新增

- 运行时工作区在工具 chip / reasoning 之外,增加**分步助手正文**:轮询
  db 里最新的 assistant 文本 part(join message 表按 role 过滤,排除用户
  prompt 回显),把正在成形的回答尾部实时显示在 composer 上方;仅运行时
  显示,turn 结束随工作区清场,权威结果仍来自 --json。

### 说明(内核限制,非本项目可改)

- 本改善只对**多步回合**(工具/reasoning/文本交错的 agentic 任务)有效:
  内核在每步结束时逐步落库,工作区因此有实时反馈。**单轮纯问答**
  (不调工具的一段文本)仍是整块——内核在生成结束时才一次性写入正文,
  运行期间 db 里没有可显示的中间态。真 token 级流式仍需 app-server。

## [0.3.1] - 2026-07-05

### 修复

- 代码块渲染加行号 gutter(dim 右对齐,像编辑器):所有非 diff 围栏代码块
  每行前显示行号;` ```diff ` 围栏保持 +/- 着色语义不变。
  （注:普通代码块没有 +/- 标记是设计使然——+/- 只对 diff 有意义,
  见 `/diff` 与 ` ```diff ` 围栏。）

### 已知限制(非本版引入)

- headless 内核**不逐 token 流式**输出正文:stdout 与 db 的 text 行都是
  整块在生成结束时落库(2026-07-05 复测)。运行期间的实时反馈仅限工具
  调用 chip 与 reasoning;纯问答(不调工具)在生成完成前只有 spinner。
  真正的 token 级流式需要接入内核 `app-server` 协议(评估中)。

## [0.3.0] - 2026-07-04

日用舒适度批(openspec 变更 session-picker-and-ui-comfort):
把 db 地基的红利与界面增强清单剩余项一次交付。

### 新增

- **`/sessions` 会话选择浮层**:列出最近内核会话(标题/目录/相对时间,
  当前目录的排前),↑↓ 选择、Enter 设为 `--resume`、Esc 关闭;
  db 降级时明确提示不可用
- **持久输入历史**:启动时读入内核 `input_history`(内核本就记录每条
  --prompt),Up/Down 跨进程可用;**Ctrl+R 反向搜索**浮层,子串过滤、
  新→旧,Enter 取回输入框
- **鼠标滚轮**滚动 transcript(±3 行);`ZCODE_TUI_NO_MOUSE=1` 或配置
  `mouse = off` 关闭;按住 Shift 仍可用终端原生选择
- **长输出折叠**:Tool/System/Diff/Error 单元超过 24 行默认折叠为头 8 行
  + `… (+N lines · Ctrl+O)`,Ctrl+O 展开/收起;助手回复永不折叠
- **配置文件** `~/.config/zcode-tui/config`(`ZCODE_TUI_CONFIG` 覆盖路径):
  行式 `key = value`,支持 11 个主题 token 十六进制覆盖与 `mouse` 开关;
  坏值/未知键静默忽略,配置永远不会阻止启动;NO_COLOR 优先级最高

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
