# Changelog

每个版本都会发布对应的 GitHub Release：打 `v*` tag 后 CI 跑全部质量门禁，
构建 x86_64-musl 静态 Linux 二进制（无需 Rust 工具链即可使用），连同
SHA256SUMS 和 install.sh 一起挂到 Release，notes 取自本文件对应版本段。

## [Unreleased]

## [0.5.0] - 2026-07-07

### 新增(流式毕业,openspec 变更 streaming-graduation)

- **app-server 默认开启**:流式路径(真流式+权限确认+会话控制+steer)已是
  `--prompt` 的功能超集,二进制默认走它;`ZCODE_TUI_APP_SERVER=0/off/false`
  显式退回经典路径,`=1` 老写法继续有效。降级纪律不变:任一环节失败永久
  无缝回落 `--prompt`。
- **流式路径 /resume 修复**(真 bug):`--resume`、`/resume`、`/sessions`
  选择此前在流式下被无声忽略(永远新建会话)。现握手改发
  `session/resume {sessionId}`(实测返回含历史 messages/todos),续接提示
  显示历史条数;resume 失败自动回退新建,不阻塞 prompt。
- **/sessions 协议数据源**:连接活跃时用 `session/list` 填充选择器
  (running 会话带标注),替代流式下的 db 轮询;db 保留为经典路径回退。
- **steer 被拒不再丢输入**:steer 成功信封的 result 是
  `queued|rejected` union(内核 FKr,实测钉死);rejected(如
  turn_not_steerable)按 reason 提示并退回排队。
- **/update 内核自更新**:官方 feed 检查 → 版本比较(dpkg 语义)→ 下载
  deb → **sha512 校验(不过不装)** → 免密 sudo 可用则 `dpkg -i`,否则给
  免 root 解包指引;更新 Tip 首行引导 /update。
- **/usage [7d|30d]**:`session/usage`(会话 token 细分)+
  `usage/stats`(周期汇总,含缓存命中率与会话数)。
- **TODO 清单**:内核 todos(create/resume 结果与 state 推送)非空时显示在
  工作区(✓/· 状态,超 6 条折叠计数)。
- **内核 slash 命令并入补全**:create/resume 上报的 `slashCommands[]`
  (名称/描述/inputHint)合入 `/` 补全,本地实现优先、同名去重。

### 新增(内核会话控制面,openspec 变更 kernel-session-controls)

- **工具权限确认**:app-server 路径接住内核的**两种**服务器→客户端请求
  (字符串信封 id,同 requestId 退避重发,全部 2026-07-07 实测钉死):
  `interaction/requestUserInput`(plan 审批,answers 形状应答)与
  `interaction/requestPermission`(**build 模式下有副作用的工具如 Write**,
  应答为所选 option 的 response 原样——内核 schema strict)。确认浮层
  ↑↓ 选项、Enter 应答、Esc 拒绝(permission 走协议级 deny、回合继续;
  plan 审批无拒绝项则 session/stop);plan_approval 批准后 TUI 自动
  setMode(build) 并队列续跑提示(实测内核不自行翻模式/续跑);permission
  批准后工具在**同一回合**内继续执行。**同时修复**:此前这类请求被解码层
  当垃圾行丢弃——**流式路径默认(build)模式下任何写文件的回合都会挂到
  600s 兜底**,plan 模式同理。
- **/model**:浮层列出内核上报的可用模型(state 推送的
  `model.available[]`),Enter 发 `session/setModel`;当前模型带 ● 标记。
- **/think**:在内核上报的思考级别间循环(`session/setThoughtLevel`,
  字段名实测 `thoughtLevel`)。
- **/compact**:app-server 会话直连 `session/compact` 原地压缩、保住会话;
  高水位提示从"仅 /new"改为"/compact or /new?";无流式会话时照旧转发 CLI。
- **/mode 与 Shift+Tab 升级**:app-server 会话活跃时改发 `session/setMode`
  即刻生效(不再只影响下次 spawn);`--mode`/预设模式在流式握手时同步给
  新会话(此前流式路径完全忽略 --mode)。
- **Steer 中途转向**:流式回合进行中输入纯文本并 Enter 即
  `session/steer` 进当前回合(不再排队;slash/shell 命令与握手/drain 期
  仍排队);steer 失败自动退回排队,不丢输入。
- 控制面回显一律以内核 `state.updated`(`mode_changed` 等)为权威,
  命令失败按请求 id 关联报错、不影响会话;闲时也消费连接消息
  (新增 idle pump,此前推送会滞留到下一回合)。

### 修复

- **内核 stderr 警告不再打碎 `--json` 总结解析**(间歇性):`--prompt` 子进程
  的 stdout/stderr 由两个线程各自读行、汇入同一通道,一条 stderr 警告可能
  恰好插进多行 pretty-printed 总结 JSON 中间——两条解析分支全失效,原始 JSON
  整块漏进 transcript、上下文水位丢失。现在行事件携带来源流标记:总结缓冲
  只收 stdout,stderr 收尾时单独以 dim 系统条目呈现(取消时丢弃)。
  shell / diff 任务保持双流合并的实时视图不变。
- **pty 冒烟的状态栏断言改用 pyte 终端仿真**(测试基建):ratatui 是差分渲染,
  状态栏原地更新(如 "streaming (app-server)" → "done (8.3s)")会跳过未变
  的单元格,原始字节流里永远不出现完整的新文案——裸子串断言必然间歇性误报
  (s10/s12 的"回合未收尾"其实是采集伪影,TUI 本身行为正确,已用协议级
  探针证实 `prompt_completed` 正常收尾)。transcript 行是整行写入,裸子串
  断言依然可靠,维持不变。

### 新增

- **欢迎页真矢量 logo（终端图形协议）**:进入 alt-screen 后探测终端图形能力
  (`ratatui-image` 的 `Picker::from_query_stdio`),支持
  **Sixel / Kitty graphics / iTerm2 inline images** 时把 ZCODE logo 贴成
  **真图**(内嵌 480×231 PNG,清华紫矢量描边——ZCODE 块字 + 天坛/鸟巢/长城/
  清华二校门线稿 + ZhiPU 地平线,duotone 到 brand 紫与字标同色)。图像按会话
  List 的条目行定位,整块滚入视图时才绘制(避免协议半绘污染)。
- **不具备图形环境自动降级**:探测到 Halfblocks(无真图形协议)/ 探测失败 /
  解码失败 → 无缝退回既有**文本天际线**(UTF-8 走 braille 盲文点阵,否则
  wire 线框)。`ZCODE_TUI_SKYLINE=braille|wire` 强制走文本(跳过探测),
  `off` 关闭。默认 auto = 先试图形再降级。
  真图仅能由用户在支持的终端(kitty/sixel/iTerm2)肉眼验证——pty 冒烟无法
  渲染图形协议,只覆盖探测/降级/编译。

## [0.4.0] - 2026-07-06

### 新增

- **真流式(试验开关 `ZCODE_TUI_APP_SERVER=1`)**:接入内核的 `zcode
  app-server`(换行分隔 JSON 的 stdio 协议),prompt 走
  `session/create → session/subscribe(desktop-continuous) → session/send`,
  助手正文经 `session/event` 的 `text_delta` **逐 token 增量渲染**进
  transcript——**单轮纯问答也真流式**,补齐 0.3.2 说明里内核 `--prompt`
  只能整块返回的限制。app-server 是长驻子进程(进程组),会话跨 prompt 复用
  同一 sessionId,`/new` 重建;Esc/Ctrl+C 取消发 `session/stop`。
- reasoning 增量显示在 composer 上方工作区;上下文水位改从 `state.updated`
  权威事件提取(best-effort,路径随内核变化仍稳)。
- **工具 chip + 输出折叠**(流式路径,对齐 Codex / Claude Code):工具运行时
  在工作区显示 spinner chip;`result` 事件到达即把该次调用落进 transcript,
  成为可 Ctrl+O 折叠的 `Tool` 条目(工具名 · 输入摘要 · 耗时 + 输出正文),
  与正文按内核发出的先后顺序交错。工具事件按 `toolCallId` 关联
  (`tool_input_start → …→ result{result.content}`)。
- 回合终止判定修正:内核**不发** `finish`/`done` 事件;回合以
  `state.updated {reason:"prompt_completed"}` 结束(`app_state_is_turn_end`)。
  修掉了流式回合永远停在 "streaming"、要等 600s 兜底才收尾的问题。
  首开欢迎/更新页的 ZCODE 下方天际线:天坛三重檐叠塔 + 台基、鸟巢编织网壳、
  长城敌楼城垛 + 蜿蜒城墙、清华二校门中央高大三券洞 + 旗杆宝顶,四座地标坐在
  连续地平线上、ZhiPU 居中。两种渲染:**braille 盲文点阵**(`skyline_braille`,
  曲线更平滑,加粗笔画,默认走 UTF-8 环境)与**线框**(`skyline_lines`,最兼容
  回退);`ZCODE_TUI_SKYLINE=braille|wire|off` 可强制。天际线与 ZCODE 字标
  **同宽居中**为一个整体 logo(对齐参考设计图)。

### 修复(对抗性审查)

- **取消后陈旧事件污染下一回合**:Esc 取消后复用同一 session,被停回合的
  尾部事件(含其 `prompt_completed`)会漏进**下一个** prompt——提前收尾新回合、
  丢掉真正的回答。现引入 drain 态:取消后吞掉停回合尾部直到其终止事件,
  之后才允许下一 prompt(超时兜底则强制重建 session)。
- **更新提示中途插入错位索引**:启动探针回合中途报更新时
  `apply_startup_report` 在顶部插入两行,后移全部索引,但未同步在流的
  `text_index`/任务 `log_index`/展开集——流式 token 会写进错误条目。现按插入
  数校正所有活动索引。
- **同步握手冻结 UI 最长 40s**:首条 prompt 的 `session/create`+`subscribe`
  两次 20s 阻塞调用改为**主循环非阻塞状态机**(按 id 关联响应),握手期间
  UI 照常渲染、Esc 可取消;40s 看门狗超时降级。
- **子进程不回收成僵尸**:`AppServerConn::cancel`/`Drop` 在 kill 后补
  `wait()` 回收,不再残留 `<defunct>`。
- **异常终止态不再苦等 600s**:新增 `app_state_turn_error`,`state.updated`
  的 error/failed/aborted/… 即以提示收尾该回合(保留半截),不拖到 600s 兜底。
- **`idle`/`ready` 不再误判回合结束**:仅 `prompt_completed` 与
  `status:completed` 算终止;复用 session 上先于 token 的 `idle`/`ready`
  过渡态不再把回合提前收成 "(no output)"。
- **游离错误响应不误伤健康回合**:回合记录其 `session/send` 的 id,只有
  **本回合**该 id 的错误响应才中止——取消遗留的 `session/stop` 错误响应
  到达时不再降级正在流式的回合。

### 说明

- **默认关闭**,是 opt-in 试验路径。任一环节失败(app-server 起不动、握手
  超时、schema 不符、连接中断)→ 本进程**永久无缝降级**回 `--prompt` +
  一条 dim 提示;当前 prompt 用 `--prompt` 重试一次,用户永不卡死。开关未设
  时代码路径与 0.3.x 完全一致。
- 流式路径阶段 1 只落正文 + reasoning + 水位;工具 chip 走协议属阶段 2
  (payload 形状待在活内核钉死)。

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
