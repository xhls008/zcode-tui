# Agent TUI 功能调研（2026-07）

> 调研日期：2026-07-02 · 快照性文档，反映当日各工具状态，不随项目更新

为 `zcode-tui` 第二轮改进做的横向调研：主流 agent CLI 在补全、认证登录、MCP 配置上的做法，以及 Rust 做 TUI 的适用性评估。

## 一、补全 / Autocomplete

| 工具 | Slash 命令 | @文件提及 | 历史搜索 |
|---|---|---|---|
| Claude Code | `/` 打开命令菜单，输入即过滤；菜单统一内置命令、skills、插件命令、MCP prompts；Tab 接受、Esc 关闭、上下键导航 | `@` 触发文件路径弹窗，MCP resources 一并模糊搜索 | `Ctrl+R` 反向搜索，`Ctrl+S` 切换范围（session→project→全部） |
| Codex CLI | `/` 弹窗 + 即时过滤；任务运行中可用 Tab 把命令排队 | `@` 模糊文件搜索；`/mention` 显式附加 | 无 Ctrl+R，上箭头历史 + Esc-Esc 回溯 |
| Gemini CLI | `/` 建议对话框，Tab 接受，Ctrl+P/N 导航；自定义 `.toml` 命令和 MCP prompts 也进菜单 | `@<path>` 注入文件/目录内容，尊重 .gitignore | `Ctrl+R` 绑定 |
| OpenCode | slash 命令 + `tab` 补全、上下键导航列表 | `@` 模糊搜索项目文件并附加 | — |
| Aider | prompt_toolkit Tab 补全 40+ 命令和文件名 | `/add` 等命令式 | `Ctrl+R`（readline 习惯） |

**采纳**：输入 `/` 即弹实时建议菜单（前缀 > 子串 > 子序列三级模糊匹配），上下键选、Tab/Enter 接受、Esc 关闭；`@` 触发文件路径补全（跳过 .git/target/node_modules 等，扫描上限 4000 项）；提交时把存在的 `@路径` 自动翻译成 `--attach`。

## 二、认证 / 登录

| 工具 | 方式 | 凭证存储 | TUI 交互 |
|---|---|---|---|
| Claude Code | 订阅 OAuth 浏览器流 / `ANTHROPIC_API_KEY` / `apiKeyHelper` / `claude setup-token` 长期 token；文档明确优先级链 | macOS Keychain；Linux `~/.claude/.credentials.json` (0600) | `/login`、`/logout`；`/status` 显示当前认证方式 |
| Codex CLI | ChatGPT OAuth（localhost:1455 回调）/ API key / `codex login --device-auth` 设备码流 | `~/.codex/auth.json`，可选 OS keyring | `codex login status`；TUI 内 `/logout`、`/status` |
| Gemini CLI | Google OAuth（默认）/ `GEMINI_API_KEY` / Vertex ADC | `~/.gemini/oauth_creds.json` 自动刷新 | `/auth` 打开方式选择对话框 |
| OpenCode | `opencode auth login`，Anthropic OAuth、Copilot 设备码等 | `~/.local/share/opencode/auth.json` | `/connect` |
| Crush | 纯 env var / 交互输入 | 配置文件 | — |

**采纳**：认证本体属于 zcode CLI，fallback 只做三件事——`/auth` 本地检测（env 链 `ZCODE_API_KEY`→`ZHIPUAI_API_KEY`→`ZAI_API_KEY`，再查凭证文件候选路径，key 打码显示）；`/login` 挂起 TUI 交互式执行 `zcode auth login`（`ZCODE_TUI_LOGIN_CMD` 可覆盖）；`/logout` 同理非交互执行。登录态常驻顶栏和状态栏。

## 三、MCP 配置

| 工具 | 配置文件 | Scope | Transport | CLI/TUI 管理 |
|---|---|---|---|---|
| Claude Code | 项目 `.mcp.json`；local/user 在 `~/.claude.json` | local > project > user | stdio / http / sse(弃用) / ws | `claude mcp add [--transport] [--scope] / list / get / remove / add-json`；`/mcp` 面板带 OAuth 登录、启停 |
| Codex CLI | `~/.codex/config.toml` `[mcp_servers.*]` + 项目 `.codex/config.toml` | user + trusted project | stdio / streamable HTTP | `codex mcp add/list/login/logout`；`enabled`、工具过滤、超时字段 |
| Gemini CLI | `settings.json` 的 `mcpServers`（user/project/system） | project 优先 | stdio / sse / streamable HTTP | `gemini mcp add [-s scope] [-t transport]`、enable/disable；`/mcp auth` OAuth |
| OpenCode | `opencode.json` `"mcp"`，local/remote 类型 | — | stdio / remote | `opencode mcp list/auth/logout`，per-server `enabled` |
| Crush | `crush.json`，警告"配置即受信代码" | — | stdio / http / sse | — |

**采纳**：`.mcp.json` 保持 Claude Code 兼容格式（`mcpServers` + `type`/`command`/`args`/`env`/`url`/`headers`）；新增 `--transport http|sse` 远程 server、`--scope user`（`~/.config/zcode/mcp.json`，XDG 优先）、`add-json`/`get`/`enable`/`disable`（`disabled` 字段软开关）；`/mcp list` 合并展示两级 scope。运行态 `/mcp status` 仍转发给 zcode。

## 四、其他值得抄的习惯

- **流式非阻塞执行**：所有主流工具都在任务运行时保持 UI 可交互、可取消（Codex `Esc` 中断、排队消息）。→ 已实现：子进程 stdout/stderr 按行流式进 transcript，spinner + 耗时显示，Esc/Ctrl+C 取消，忙时输入自动排队。
- 粘贴保护（bracketed paste，多行粘贴不触发提交）→ 已实现。
- readline 式光标编辑（Left/Right/Home/End/Ctrl+A/E/W）→ 已实现。
- 未采纳（超出 fallback 定位）：主题系统、vim 模式、键位自定义文件、会话选择器、权限队列、LSP。

## 五、Rust 适合做 TUI 吗

结论：**适合，且本品类有旗舰级先例**。

- OpenAI **Codex CLI 从 TypeScript/Ink 整体重写为 Rust + ratatui**，官方理由：去掉 Node 运行时依赖（单一静态二进制）、GC 停顿不适合长会话流式输出、毫秒级启动、原生沙箱。
- ratatui 生态活跃（2026-06 仍在发版，v0.30.x；~21k stars）：gitui、bottom、yazi、atuin、television、bandwhich 等主流工具都在用；crossterm 后端 + tokio `EventStream` 的异步事件循环是成熟模式；输入组件有 tui-textarea / tui-input / tui-popup。
- 代价：比 Go（Bubble Tea 自带 filepicker/textinput 组件库）和 TS（Ink/React，Claude Code、Gemini CLI 在用）开发速度慢，补全弹窗这类高层组件要自己拼。
- 对本项目：fallback 定位追求零依赖、启动快、常驻稳定，正是 Rust 的强项。

## 原始报告来源

官方文档：code.claude.com/docs（interactive-mode / mcp / authentication / keybindings）、developers.openai.com/codex（cli / auth / mcp）、github.com/google-gemini/gemini-cli docs、opencode.ai/docs、github.com/charmbracelet/crush、aider.chat/docs、ratatui.rs、github.com/openai/codex codex-rs。
