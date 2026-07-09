# zcode-tui

[中文](README.md) | [English](README.en.md) | [Releases](https://github.com/xhls008/zcode-tui/releases) | [Design](docs/2026-07-04-design.md)

![zcode-tui effect preview](assets/zcode-tui-effect-preview.png)

> **非官方声明**：`zcode-tui` 不是 ZCode / 智谱官方项目，也未获得官方背书。
> 它是社区/个人维护的 Linux 终端 fallback，用来补齐官方包当前缺失的 TUI 体验。

`zcode-tui` 是一个 **Rust 写的 ZCode 终端 TUI fallback**，专门兜住官方 Linux
包缺少 `@zcode/tui` 的尴尬空洞。它面向 SSH、tmux、无桌面服务器和纯键盘
工作流：普通输入走官方 `zcode --prompt`，常用 slash 命令、MCP 配置、
shell escape、命令面板、会话选择、流式输出和编辑器工作流在本地补齐。

它不伪装成官方实现，只是一个实用的终端壳。

## 项目主题

官方 Linux 包把 `tui` 命令写进 help，却没有把 terminal TUI runtime 一起交付。
这个项目的主题很简单：**官方不干，那就自己造**。

它不追求复刻桌面端，也不等一个不知道何时补齐的 `@zcode/tui`。目标是给 SSH、
tmux、无桌面服务器和纯键盘工作流一个能立即使用的终端界面：少一点发布会式
想象，多一点能跑、能补全、能流式输出、能接住日常工作的工程实现。

## 功能

设计参考了 Claude Code、Codex CLI、Gemini CLI、OpenCode、Crush 的使用习惯，
调研记录见 `docs/references/tui-research-2026-07.md`。

**视觉与渲染**

- Codex 风布局：无边框流式 transcript，用户消息和输入框用浅色底横条 +
  `›` 提示符，助手回复 `•` 开头平铺，会话信息横幅进 transcript，
  底部一行 dim 快捷键提示（运行任务时变 spinner 工作行）。
- 智谱风配色：GLM 蓝单一强调色 + 冷灰中性色阶，行内代码蓝色文字、
  引用绿色，语义绿/红只用于 diff 与错误；
  `--no-color` 或 `NO_COLOR` 时退化为无色。
- **语法高亮**：围栏代码块按语言用 syntect 着色（Codex 同款方案，
  base16-ocean 主题）+ dim 行号 gutter；` ```diff ` 围栏按
  +绿/−红/@@蓝 渲染（+/- 是 diff 专属语义，普通代码块只有行号）。
- **启动欢迎框**：Codex 风圆角信息框，显示内核/TUI 版本、目录、
  mode、auth 及对应的切换命令提示。
- **官方更新检测**：启动时后台读取 `/opt/ZCode` 的 electron-updater
  配置并拉取官方 `latest-linux.yml`（与桌面端同一发布渠道），发现新版
  时显示天坛/鸟巢/长城/清华校门元素的 ZCODE ASCII logo 和 Tip 提示
  （含 changelog 链接与 deb 直链）；`ZCODE_TUI_NO_UPDATE_CHECK=1` 关闭。
- **Markdown 渲染**：助手回复按 markdown 渲染——标题、粗斜体、行内代码、
  围栏代码块、列表、引用、**表格（按显示宽度对齐列）**、分隔线
  （pulldown-cmark，Codex CLI 同款）。
- **`/diff [args]`**：git diff 语法着色（+绿 −红 @@蓝 文件头加粗），
  如 `/diff --staged`、`/diff HEAD~1`。
- **工具调用可视化**：当 zcode 以 `--json` 输出 JSONL 事件时，
  `tool_use`/`tool_result` 渲染为紧凑 chip（`⚙ bash {...}` / `↳ 结果`），
  文本事件正常走 markdown；纯文本输出不受影响。

**会话**

- **多轮连续对话**：首条 prompt 成功后自动带 `--continue`，TUI 里的对话
  在内核会话中延续；`/new` 重开（上下文重置）、`/resume [sess_id]` 恢复
  最近或指定会话、`/mode` 或 `Shift+Tab` 切换权限模式
  （build/edit/plan/yolo），欢迎框实时显示会话状态。
- **`/sessions` 会话选择器**：浮层列出最近内核会话（标题/目录/相对时间，
  当前目录的排前），↑↓ 选择、Enter 接续、Esc 关闭。
- **resume 历史回放**：流式续接成功后，在 "resumed sess_…" 提示下用 dim
  紧凑行回放最近 ≤6 条对话（`›` 用户 / `·` 助手，每条 ~400 字符截断），
  接上话头不用翻旧账。
- **会话善后**：`/new` 丢弃活跃流式会话与 `/exit` 退出时尽力发
  `session/close`，不留悬挂会话（fire-and-forget，失败静默）。

**交互**

- Rust + Ratatui + Crossterm，单一静态二进制，毫秒级启动。
- CJK 感知：中文按 2 列宽度折行与定位光标，表格按显示宽度对齐。
- **非阻塞流式执行**：`zcode --prompt` 和 `! <cmd>` 的输出按行实时进 transcript，
  spinner 显示耗时，`Esc`/`Ctrl+C` 取消，忙时新输入自动排队。
- **实时进度**：prompt 运行期间只读轮询内核会话库（`~/.zcode/cli/db/db.sqlite`，
  内核边跑边写），工作区实时显示工具 chip（运行中 spinner → 完成 ✓ + 耗时 /
  失败 ✗）、最新 reasoning、以及**正在成形的助手正文**（多步回合里逐步落库）；
  仅运行时显示、结束即清场；schema 不识别或库缺失时整组自动降级。
- **真流式（默认开启）**：接内核 `zcode app-server`
  协议（`session/create → subscribe → send`），助手正文经 `session/event`
  的 `text_delta` 逐 token 增量渲染进 transcript——**单轮纯问答也真流式**，
  补齐上面 db 轮询对单轮无中间态的空缺。任一环节失败（起不动 /
  握手超时 / schema 不符 / 断连）→ 本进程永久无缝降级回 `--prompt` + 一条
  dim 提示，当前 prompt 用 `--prompt` 重试一次，用户永不卡死；需要经典
  `--prompt` 路径时设 `ZCODE_TUI_APP_SERVER=0`。
- **工具权限确认（app-server 路径）**：build 模式下有副作用的工具（写文件等）
  与 plan 模式的计划审批会弹**确认浮层**（↑↓ 选项 / Enter 应答 / Esc 拒绝），
  批准后工具同回合继续执行；plan 计划批准后自动切 build 并续跑。
  edit/plan/build 模式的权限门禁在流式路径上**真正生效**（不再是 headless
  一律 yolo）。
- **会话控制（app-server 路径）**：`/model` 浮层切换模型、`/think` 循环思考
  级别、`/compact` 原地压缩上下文保住会话、`/mode`/Shift+Tab 即刻切换活跃
  会话的权限模式；**流式回合进行中直接输入文本＝转向（steer）当前回合**，
  不用取消重来。
- **上下文水位**：prompt 通道用 `--json` 总结对象作为权威结果（response 走
  markdown 渲染，解析失败自动降级纯文本），状态栏常驻 `ctx 9k/200k (4%)`
  用量显示，≥80% 提示 `/compact` 或 `/new`。
- **实时补全菜单**：输入 `/` 即弹出建议（前缀 > 子串 > 子序列模糊匹配），
  上下键选择、`Tab`/`Enter` 接受、`Esc` 关闭。
- **`@文件` 提及**：输入 `@` 补全项目内路径（跳过 .git/target/node_modules）；
  提交时存在的 `@路径` 在经典路径翻译成 `--attach`，在流式路径翻译成
  `session/send` 的 `attachments[]`（图片扩展名走 image、其余按扩展名给
  mimeType，`localPath` 直引本机文件）——两条路径都能让模型读到文件。
- readline 式光标编辑：Left/Right、Home/End、`Ctrl+A/E`、`Ctrl+W`、Delete。
- **持久输入历史**：启动时读入内核 `input_history`（内核记录每条 --prompt），
  Up/Down 跨进程可用；`Ctrl+R` 反向搜索（子串过滤、新→旧、Enter 取回）。
- **鼠标滚轮**回看 transcript（±3 行/格）；`ZCODE_TUI_NO_MOUSE=1` 或配置
  `mouse = off` 关闭捕获；按住 Shift 可用终端原生文本选择。
- **长输出折叠**：工具/系统/diff/错误单元超过 24 行默认折叠为头 8 行 +
  `… (+N lines · Ctrl+O)`，`Ctrl+O` 展开/收起；助手回复永不折叠。
- **OSC52 复制**：`Ctrl+X` 后 `y` 或 `/copy` 把最后一条助手回复写进系统
  剪贴板（OSC52 直发终端，SSH 远程也能落到本地剪贴板；负载 ~100KB 截断）。
  tmux 里需 `set -g set-clipboard on`。
- **回合完成铃**：流式回合或经典 prompt 任务收尾且耗时 >30s 响一声终端铃
  （BEL），切走干别的也知道跑完了；配置 `notify = off` 关闭，取消不响。
- **文件变更小结**：流式回合里被门禁工具落盘会产生 checkpoint，收尾时
  显示 `N file(s) changed · /diff to review`，改没改文件一眼可知。
- **状态栏模型/模式**：footer 右侧常驻当前模型与权限模式
  （如 `glm-5.1 · build`，取内核状态推送；首推送前只显示模式）。
- bracketed paste：多行粘贴不会误触发提交。
- `Ctrl+P` 命令面板；OpenCode 风格 leader key：`Ctrl+X` 后接 `p/h/e/x/u/q`。
- `Ctrl+G` 或 `/editor` 调 `$VISUAL`/`$EDITOR` 编辑长 prompt；`Ctrl+J` 多行输入。
- Up/Down 输入历史；PgUp/PgDn 回看滚动（自动跟随最新输出）。

**认证**

- `/auth` 三态检测：内核硬性要求 `~/.zcode/cli/config.json`（实测 0.15.0），
  以它为"已配置"的判据；只有 env key（`ZCODE_API_KEY` 链，打码显示）或凭证
  文件时如实显示"部分配置"并给出补齐命令，不再误报已登录。
- **未登录启动屏**：清华紫 ZCODE 字标（块体亮紫 + 描边暗紫阴影），底部
  鸟巢/长城/天坛轮廓线，列出三条免浏览器登录路径；`NO_COLOR` 全退化。
- `/login` 挂起 TUI，交互式执行 `zcode login`（Z.AI OAuth，可用
  `ZCODE_TUI_LOGIN_CMD` 覆盖）；无桌面环境（无 `DISPLAY`/`WAYLAND_DISPLAY`）
  自动附 `--no-browser`，OAuth URL 直接打印在终端。
- `/logout` 执行 `zcode logout`（可用 `ZCODE_TUI_LOGOUT_CMD` 覆盖）。
- 顶栏和状态栏常驻显示当前认证方式。

**MCP 配置**（`.mcp.json` 与 Claude Code 格式兼容）

- 两级 scope：项目 `.mcp.json` 和用户级 `~/.config/zcode/mcp.json`（`--scope user`）。
- **流式会话真生效**：内核自身不读项目 `.mcp.json`（实测 0.15.0）；TUI 在
  `session/create`/`resume` 时把两级配置翻译成协议的 `mcpServers[]` 传给
  内核（disabled 跳过、同名项目级优先），模型在会话内拿到
  `mcp__<name>__<tool>` 工具。
- stdio 与远程 server：`/mcp add --transport http|sse <name> <url>`。
- `/mcp add-json`、`/mcp get`、`/mcp enable`、`/mcp disable`（软开关，不删配置）。
- `/mcp status` 等运行态命令继续转发给 ZCode。

**路由**

- 普通文本通过 `zcode --prompt` 发送给 ZCode。
- `/goal ...`、`/goal replace ...`、`/skill <name> <task>` 转发给 ZCode。
- `/skills [list]` 调 `zcode skills list`；`/status` 显示会话/认证/MCP 概览。
- `! <cmd>` 本地 shell escape。
- 官方 `zcode tui` 因缺 `@zcode/tui` 失败时，可自动 fallback 到这个 TUI。

## 安装 / 更新

**方式一：下载 Release 二进制（SSH 服务器推荐，无需 Rust 工具链）**

每个版本都会发布 [GitHub Release](https://github.com/xhls008/zcode-tui/releases)，
附带静态链接的 Linux x86_64 二进制（musl，任何发行版开箱可用）和校验和：

```bash
mkdir -p ~/.local/bin
curl -fL -o ~/.local/bin/zcode-tui \
  https://github.com/xhls008/zcode-tui/releases/latest/download/zcode-tui-x86_64-unknown-linux-musl
chmod +x ~/.local/bin/zcode-tui
```

需要 `zcode` wrapper 的话，再取同一 Release 里的 `install.sh` 以 `--no-build`
模式生成（不需要 cargo）：

```bash
curl -fLO https://github.com/xhls008/zcode-tui/releases/latest/download/install.sh
bash install.sh --no-build
```

**方式二：从源码构建**

一条命令完成构建和安装（更新同样跑它）：

```bash
./install.sh
```

它会把 release 二进制装到 `~/.local/bin/zcode-tui`，并生成 `~/.local/bin/zcode`
wrapper（带管理标记，重复运行幂等；已存在的非托管 wrapper 会先备份）。托管
wrapper 默认沿用 fallback TUI 的 app-server 真流式；需要回到经典 `--prompt`
路径时可 `ZCODE_TUI_APP_SERVER=0 zcode`。
其他用法：

```bash
./install.sh --prefix /usr/local   # 装到别的前缀
./install.sh --no-wrapper          # 只装 zcode-tui 二进制
./install.sh --uninstall           # 卸载
```

也可以手动构建直接运行：

```bash
cargo build --release
./target/release/zcode-tui
```

或让 wrapper 指向自定义位置的 fallback：

```bash
export ZCODE_FALLBACK_TUI="$PWD/target/release/zcode-tui"
zcode tui
```

这台机器上的 `~/.local/bin/zcode` 是一个 wrapper，串起官方包和本项目：

- 官方 Linux 桌面包（`/opt/ZCode`，deb 来自 `cdn-zcode.z.ai`）内嵌了 headless CLI
  内核 `resources/glm/zcode.cjs`（`--prompt`、`--attach`、`login`、`skills` 等都在）。
- 内核需要 `node:sqlite`（Node ≥ 22.5），wrapper 优先用 Electron 自带的 Node
  （`ELECTRON_RUN_AS_NODE=1 /opt/ZCode/zcode`）运行它；Electron 起不动时
  （无桌面机器缺 GUI 库）自动回退到系统 Node（需 ≥ 22.5），
  `ZCODE_FORCE_SYSTEM_NODE=1` 可强制走系统 Node。
- `zcode tui` 或裸 `zcode` 前先探测 `@zcode/tui` 能否解析；官方 Linux 包
  报下面这个错误时，才 fallback 到本项目：

```text
Cannot find package '@zcode/tui'
```

首次使用前需要登录：`zcode login`（Z.AI OAuth），或在 TUI 里执行 `/login`。

## SSH / 无桌面环境

这是本项目的核心使用场景：在没有桌面的服务器上通过 SSH 直接用终端跟 ZCode
干活。TUI 本身是纯终端程序（无剪贴板、通知、浏览器依赖，ratatui + crossterm
在 SSH/tmux 里和 vim 一样工作），要打通的只有内核引导这一环：

1. **拿到内核**：无桌面机器不必安装 deb，解包即可（无需 root）：

   ```bash
   dpkg-deb -x ZCode-<ver>.deb ~/.local/opt/zcode/<ver>/
   ```

   wrapper 自动探测 `$ZCODE_APP` → `/opt/ZCode` → `~/.local/opt/zcode/*/opt/ZCode`。

2. **运行内核**：极简服务器往往缺 Electron 加载所需的桌面库（libgtk-3、
   libnss3 等，即使 `ELECTRON_RUN_AS_NODE=1` 也要先过动态链接）。wrapper 会
   探测 Electron 能否启动，起不动时自动回退到系统 Node（需 ≥ 22.5，内核依赖
   `node:sqlite`）。两条路满足其一即可：装 Node ≥ 22.5，或
   `apt install libgtk-3-0t64 libnss3`（只装库，不需要桌面会话）。

3. **登录**：内核硬性要求 `~/.zcode/cli/config.json`（模型配置），只设
   `ZCODE_API_KEY` 环境变量是**不够**的（实测 0.15.0）。免浏览器的路，
   按推荐顺序：

   - Coding Plan API key 一条命令配置：
     `zcode login bigmodel-coding-plan-api-key <key>`（智谱国内）或
     `zcode login zai-coding-plan-api-key <key>`（Z.AI 国际）；
   - `zcode login --no-browser`：打印 OAuth URL，任意设备打开完成授权；
   - 从已登录机器**同时拷贝** `~/.zcode/cli/config.json` 和
     `~/.zcode/v2/credentials.json` 两个文件。

## 命令

```text
text                         通过 zcode --prompt 发送 prompt
@<path>                      提及 cwd 内的文件，自动 --attach（越界/符号链接逃逸会被拒绝）
! <cmd>                      执行本地 shell 命令
/goal <text>                 转发给 ZCode goal 处理
/goal replace <text>         替换当前 ZCode goal
/skill <name> <task>         强制使用某个 ZCode skill
/skills [list]               通过 zcode skills list 列出 skills
/login                       挂起 TUI 交互式登录（zcode login）
/logout                      登出（zcode logout）
/auth                        显示本地认证状态（env key / 凭证文件）
/status                      会话、认证、MCP 概览
/mcp list                    列出项目级 + 用户级 MCP server
/mcp config                  打印两级 MCP 配置路径
/mcp add <name> [--] <cmd> [args]
                             添加 stdio MCP server；--scope/--transport 须在
                             命令名之前，-- 之后的参数原样归 server
/mcp add --transport http|sse <name> <url>
                             添加远程 MCP server
/mcp add-json <name> <json>  从 JSON 添加 server
/mcp get <name>              以 JSON 显示某个 server
/mcp enable|disable <name>   启用/禁用（不删配置）
/mcp remove <name>           删除 MCP server
/mcp ... --scope user        操作 ~/.config/zcode/mcp.json
/mcp status                  作为 /mcp status 转发给 ZCode
/mode [build|edit|plan|yolo] 查看/切换权限模式（Shift+Tab 循环）；
                             app-server 流式路径下即刻作用于活跃会话
/model                       切换会话模型（浮层选择内核上报的候选；
                             app-server 流式路径）
/think                       循环思考级别 enabled/disabled（app-server）
/compact                     原地压缩会话上下文、保住会话（app-server 会话
                             直连内核 compact；否则转发 CLI）
/usage [7d|30d]              显示当前会话与周期 token 用量（app-server）
/update                      从官方 feed 更新 ZCode 内核（下载 + sha512 校验）
/copy                        复制最后一条助手回复到系统剪贴板（OSC52；
                             tmux 需 set -g set-clipboard on）
/resume [sess_id]            恢复最近（不带参数）或指定会话；流式路径续接
                             后回放最近对话
/sessions                    浮层选择最近会话并接续
/new                         重开会话，上下文重置
/diff [args]                 git diff 语法着色（--staged、路径等）
/ide [path]                  在 IDE 中打开 cwd 或指定路径
                             （自动探测 code/cursor/zed/subl/idea，
                             可用 ZCODE_TUI_IDE_CMD 覆盖）
/editor                      用 $VISUAL 或 $EDITOR 编辑当前输入
/clear                       清屏
/exit                        退出
```

## 快捷键习惯

```text
Ctrl+P                       命令面板
Ctrl+X then p/h/e/x/u/y/q    leader：面板/帮助/编辑器/清会话/清输入/
                             复制最后回复(OSC52)/退出
Tab / Up / Down              建议菜单：接受 / 选择
Shift+Tab                    循环切换权限模式
Enter                        发送；菜单导航中则接受选中项；流式回合进行中
                             发送纯文本＝转向当前回合（steer，app-server；
                             slash 命令仍排队）
Esc                          关闭菜单弹窗 → 取消运行中任务 → 退出；
                             权限确认浮层上＝拒绝
Up / Down / Enter            权限确认浮层：选择选项 / 应答（plan 模式下被
                             门禁的工具会弹出该浮层）
Left/Right Home/End          输入光标移动
Ctrl+A / Ctrl+E              行首 / 行尾
Ctrl+W                       删除前一个词
Ctrl+G                       用 $VISUAL 或 $EDITOR 编辑当前输入
Ctrl+J                       插入换行
Ctrl+R                       反向搜索输入历史
Ctrl+O                       展开 / 折叠最近的长输出
PgUp / PgDn / 鼠标滚轮       回看滚动 / 跟随最新输出
?                            空输入时打开帮助
```

## 环境变量

```text
ZCODE_TUI_ZCODE_BIN          zcode 二进制路径（默认 zcode）
ZCODE_TUI_LOGIN_CMD          覆盖 /login 执行的命令
ZCODE_TUI_LOGOUT_CMD         覆盖 /logout 执行的命令
ZCODE_TUI_IDE_CMD            覆盖 /ide 启动的 IDE 命令
ZCODE_TUI_NO_UPDATE_CHECK    置 1 关闭启动时的官方更新检测
ZCODE_TUI_APP_SERVER         默认启用真流式（走 zcode app-server，逐 token
                             流式；失败无缝降级回 --prompt）。工具权限确认
                             浮层、/model /think /compact、/mode 即刻切换与
                             steer 中途转向都跑在这条路径上。设 0/off/false/no
                             关闭；1/true/on 继续兼容旧 wrapper
ZCODE_API_KEY 等             /auth 检测的 API key 环境变量链
ZCODE_TUI_NO_MOUSE           置 1 关闭鼠标捕获
ZCODE_TUI_SKYLINE            欢迎页 ZCODE logo 渲染：默认探测终端图形协议
                             （Sixel/Kitty/iTerm2），支持则贴真图（清华紫矢量
                             logo）；不支持自动降级为文本天际线——UTF-8 环境走
                             braille 盲文点阵（曲线更平滑），否则 wire 线框。
                             braille / wire 可强制走文本天际线（跳过图形探测），
                             off 关闭天际线。若点阵显示成方块/发虚，设 wire
ZCODE_TUI_CONFIG             配置文件路径（默认 ~/.config/zcode-tui/config）
ZCODE_TUI_LOG                设为文件路径开启协议调试日志（追加式：入站
                             摘要、出站只记方法名、握手/收尾/降级转换；
                             绝不记录请求 params——runtimeModel/apiKey
                             不落盘；未设时零开销）
ZCODE_APP                    wrapper：指定 ZCode 桌面包目录（覆盖自动探测）
ZCODE_FALLBACK_TUI           wrapper：指定 fallback TUI 二进制路径
ZCODE_FORCE_SYSTEM_NODE      wrapper：置 1 强制用系统 Node 运行内核
```

## 配置文件

`~/.config/zcode-tui/config`（行式 `key = value`，`#` 行为注释；坏值和
未知键静默忽略，配置永远不会阻止启动）：

```text
# 主题 token 覆盖。默认是 GLM 蓝 + 冷灰终端风；官方不提供 TUI 主题，
# 这里就把颜色控制权留给终端用户。
# 可配置 token：accent accent_dim text dim good bad frame code_bg band_bg brand brand_dim
accent = #6088ff
# 关闭鼠标捕获
mouse = off
# 关闭 >30s 回合完成铃（默认开启）
notify = off
```

`NO_COLOR`/`--no-color` 优先级最高，设置后所有颜色（含自定义）全部退化。

## 设计与参考

- **`docs/2026-07-04-design.md`** — 权威设计文档：定位原则（在 ZCode 上做加法
  不替代）、系统架构、流式任务模型、视觉语言、能力边界、跨平台设计、路线图
- `docs/references/tui-research-2026-07.md` — 补全 / 认证 / MCP / Rust 适用性横向调研
- `docs/references/agent-tui-habits.md` 与 `docs/references/raw/` — 原始文档快照

## 限制

这个 fallback 没有重建 ZCode 缺失的官方 `@zcode/tui` 包，也没有 ZCode 官方 TUI 可能拥有的内部实时会话模型。它做的是一件朴素但有用的事：读输入、分发 slash 命令、调用官方 CLI 路径、展示输出。

## 开发

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features
```

## 背景与吐槽

看到 ZCode 发布了，兴冲冲下载 Linux beta，结果包里主打的是桌面版；直接运行
`zcode`，想要一个像 Codex、Claude Code、Kimi Code 那样能在终端里干活的
TUI，结果没有。CLI help 里写着 `tui`，真敲 `zcode tui` 又提示缺 `@zcode/tui`。

![ZCode 发布了，TUI 呢？](assets/zcode-no-tui-satire.png)

Kimi 有 TUI，Codex 有 TUI，Claude Code 有 TUI。ZCode 都发布了，Linux 用户
想在终端里直接开干，竟然还要自己补一层。那就补。

这体验就像菜单上写着牛肉面，端上来一碗热水，还问你是不是已经闻到香味了。

![菜单写着牛肉面，端上来一碗热水](assets/beef-noodle-hot-water-satire.png)

朴素归朴素，至少不会让 Linux 用户看到 `tui` 两个字后只能对着桌面版发呆。

## License

MIT
