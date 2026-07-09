# streaming-attachments-and-comfort

## Why

流式毕业(0.5.x)后 app-server 路径成了默认路径,但两个经典路径的能力在
流式下**静默丢失**:`@文件` 提及只剩纯文本(经典路径翻译成 `--attach`,
流式 send 只带 content),项目 `.mcp.json` 完全不被内核读取(2026-07-07
实测:内核 bundle 只在 plugin 加载里认 `.mcp.json`,裸 `session/create`
后 `mcp/list` 返回空 statuses)。两个都是真 bug。同批补上一组日用舒适度:
resume 历史回放、OSC52 复制、状态栏模型显示、回合完成铃、文件变更小结、
协议调试日志、session/close 收尾,以及 /update 的 feed 文件名加固。
全部协议形状已于 2026-07-07 对真内核 0.15.0 实弹钉死(见 design.md)。

## What Changes

- **修复:流式路径 @文件 附件**:`session/send` 带 `attachments[]`
  (bundle schema `Pwt`,kind 按扩展名分 image/file,`localPath` 即可,
  实测模型能读到内容;file 类 `sizeBytes` 必填)。握手首发与快路径都接。
- **修复:流式会话接 MCP 配置**:`session/create`/`resume` 附
  `mcpServers[]`(bundle schema `$xe`),由既有 `.mcp.json` + 用户级
  `~/.config/zcode/mcp.json` 加载器构造;实测传入后模型确实拿到
  `mcp__<name>__<tool>` 工具。
- **resume 历史回放**:resume 结果的 `messages[]`(实测
  `{info:{role}, parts:[{type:"text",text}…]}`)取末尾 ≤6 条
  user/assistant 以 dim 紧凑形式落 transcript(每条 ~400 字符截断),
  原 "resumed sess_…" 提示保留为小节头。
- **OSC52 复制**:`Ctrl+X` 后 `y` 或 `/copy` 把最后一条助手回复经 OSC52
  写系统剪贴板(base64 ~100KB 上限);tmux 需 `set -g set-clipboard on`
  (README 说明,不做 passthrough 包裹)。
- **状态栏模型/模式**:footer 右侧在水位/认证外显示
  `glm-5.1 · build`(SessionControls 缓存,首推送前回退 config mode)。
- **回合完成铃**:流式回合或经典 prompt 任务收尾且耗时 >30s 时响一声
  终端铃(BEL);配置 `notify = off` 关闭。
- **文件变更小结**:按回合统计 `checkpoint.created` 事件
  (params.type 直通解码,payload 带 fileCount/scope),收尾时 >0 则加
  dim 系统行 `N file(s) changed · /diff to review`。
- **协议调试日志**:`ZCODE_TUI_LOG=<路径>` 追加式行日志(入站摘要、
  出站只记方法名、握手/收尾/降级转换);**绝不落盘 params**
  (runtimeModel/apiKey 红线);未设时零开销。
- **session/close 收尾**:/new 丢弃活会话与 /exit 带活会话时尽力发
  `session/close {sessionId}`(params 实测 `{sessionId}` strict)。
- **加固 /update**:feed 提供的 deb 文件名过 `basename` 再拼下载路径,
  防恶意 feed 路径穿越(实现级加固,不改需求)。
- 文档:README 命令/快捷键/环境变量/配置段、/help、CHANGELOG;
  **README.en.md 补同步**近期功能(权限确认、会话控制、/usage /update、
  默认流式、steer、resume 修复)。

## Capabilities

### New Capabilities

- `streaming-attachments`: 流式路径 @文件 提及 → send attachments
  (kind/mimeType 按扩展名、localPath、file 类 sizeBytes 必填)
- `streaming-mcp-servers`: 两级 MCP 配置 → create/resume 的 mcpServers
  数组(stdio/http/sse 两形状、disabled 跳过、env/headers 键值对数组)
- `resume-history-replay`: resume 结果 messages 的紧凑历史回放
- `clipboard-copy`: OSC52 复制最后一条助手回复(Ctrl+X y、/copy、
  100KB 上限、tmux 文档说明)
- `turn-finish-comfort`: >30s 回合完成铃(notify=off 可关)+
  checkpoint 文件变更小结行
- `protocol-debug-log`: ZCODE_TUI_LOG 协议/状态调试日志(红线:不落
  params,零开销默认)

### Modified Capabilities

- `app-server-client`: 解码扩展——无 payload.kind 的 session/event 以
  params.type 直通(checkpoint.created 等);会话生命周期补 session/close
  尽力收尾(/new 丢会话、/exit 带活会话)
- `session-controls`: 状态栏右侧常驻当前模型与模式(controls 缓存,
  首推送前回退 config)

## Impact

- `src/lib.rs`:附件构造器(纯函数)、mcpServers 构造器(复用 McpConfig
  加载)、resume messages 解析、OSC52 序列构造、UiConfig 加 notify 键、
  decode_app_message 的 params.type 直通、AppServerTurn 计 checkpoint、
  session/close params、调试日志格式化(纯函数部分)——全部配单测
- `src/main.rs`:流式 send 两处接附件与 mcpServers、resume 回放渲染、
  leader 表加 y、/copy 本地命令、footer 右侧、BEL 与文件小结落点、
  日志 writer(启动一次判定)、teardown 与 /new 的 close、/update 脚本
  一行 basename
- `tests/core.rs` 单测;`tests/pty_smoke.py` 新增流式附件冒烟等场景
- README.md / README.en.md / CHANGELOG.md / help_text;无新依赖
  (OSC52 直写 stdout,base64 手撸或用既有依赖)
