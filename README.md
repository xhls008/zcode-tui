# zcode-tui

![ZCode 发布了，TUI 呢？](assets/zcode-no-tui-satire.png)

看到 ZCode 发布了，兴冲冲下载 Linux beta，结果包里主打的是桌面版；直接运行 `zcode`，想要一个像 Codex、Claude Code、Kimi Code 那样能在终端里干活的 TUI，结果没有。CLI help 里写着 `tui`，真敲 `zcode tui` 又提示缺 `@zcode/tui`。这体验就像菜单上写着牛肉面，端上来一碗热水，还问你是不是已经闻到香味了。

![菜单写着牛肉面，端上来一碗热水](assets/beef-noodle-hot-water-satire.png)

Kimi 有 TUI，Codex 有 TUI，Claude Code 有 TUI。ZCode 都发布了，Linux 用户想在终端里直接开干，竟然还要自己补一层。那就补：这个项目是一个 **Rust 写的 ZCode 终端 TUI fallback**，专门兜住官方 Linux 包缺少 `@zcode/tui` 的尴尬空洞。

这不是 ZCode 官方 TUI，也不伪装成官方实现。它是一个实用的终端壳：普通输入走官方 `zcode --prompt`，常用 slash 命令、MCP 配置、shell escape、命令面板和编辑器工作流在本地补齐。

## 功能

- Rust 编写，不依赖 Python 运行时。
- Ratatui + Crossterm 多面板终端界面。
- 极客风 terminal bus 顶栏，带 `智谱 @zcode` 标识。
- Transcript、Control Matrix、Prompt、Status 四块主区域。
- `Ctrl+P` 命令面板。
- OpenCode 风格 leader key：`Ctrl+X` 后接 `p/h/e/x/u/q`。
- `Tab` slash 命令建议和补全。
- `/help` 或空输入下 `?` 打开帮助。
- Up/Down 输入历史。
- PgUp/PgDn、Home、End 滚动。
- `Ctrl+G` 调 `$VISUAL` 或 `$EDITOR` 编辑长 prompt。
- `Ctrl+J` 多行输入。
- `! <cmd>` 本地 shell escape。
- 官方 `zcode tui` 因缺 `@zcode/tui` 失败时，可自动 fallback 到这个 TUI。
- 普通文本通过 `zcode --prompt` 发送给 ZCode。
- `/goal ...`、`/goal replace ...` 转发给 ZCode。
- `/skill <name> <task>` 转发给 ZCode。
- `/skills [list]` 调 `zcode skills list`。
- 本地 MCP 配置管理：
  - `/mcp list`
  - `/mcp config`
  - `/mcp add <name> <command> [args...]`
  - `/mcp remove <name>`
- `/mcp status` 等运行态 MCP 命令继续转发给 ZCode。

## 安装

构建 release 版本：

```bash
cargo build --release
```

直接运行：

```bash
./target/release/zcode-tui
```

或者让本地 ZCode wrapper 指向它：

```bash
export ZCODE_FALLBACK_TUI="$PWD/target/release/zcode-tui"
zcode tui
```

这台机器上的 `~/.local/bin/zcode` 已经打过补丁：先尝试官方 TUI；只有当 Linux 包报下面这个错误时，才 fallback 到本项目：

```text
Cannot find package '@zcode/tui'
```

## 命令

```text
text                         通过 zcode --prompt 发送 prompt
! <cmd>                      执行本地 shell 命令
/goal <text>                 转发给 ZCode goal 处理
/goal replace <text>         替换当前 ZCode goal
/skill <name> <task>         强制使用某个 ZCode skill
/skills [list]               通过 zcode skills list 列出 skills
/mcp list                    列出本地 .mcp.json 里的 MCP server
/mcp config                  打印本地 .mcp.json 路径
/mcp add <name> <cmd> [args] 添加/更新 MCP server
/mcp remove <name>           删除 MCP server
/mcp status                  作为 /mcp status 转发给 ZCode
/editor                      用 $VISUAL 或 $EDITOR 编辑当前输入
/clear                       清屏
/exit                        退出
```

## 快捷键习惯

```text
Ctrl+P                       命令面板
Ctrl+X then p                命令面板
Ctrl+X then h                帮助
Ctrl+X then e                外部编辑器
Ctrl+X then x                清空会话
Ctrl+X then u                清空输入
Ctrl+X then q                退出
Tab                          slash 命令补全/建议
Ctrl+G                       用 $VISUAL 或 $EDITOR 编辑当前输入
Ctrl+J                       插入换行
?                            空输入时打开帮助
```

## 参考

Claude Code 和 OpenCode 的官方使用习惯参考已经下载到本地：

- `docs/references/agent-tui-habits.md`
- `docs/references/raw/`

## 限制

这个 fallback 没有重建 ZCode 缺失的官方 `@zcode/tui` 包，也没有 ZCode 官方 TUI 可能拥有的内部实时会话模型。它做的是一件朴素但有用的事：读输入、分发 slash 命令、调用官方 CLI 路径、展示输出。

朴素归朴素，至少不会让 Linux 用户看到 `tui` 两个字后只能对着桌面版发呆。

## 开发

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features
```

## License

MIT
