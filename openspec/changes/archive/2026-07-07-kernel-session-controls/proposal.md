# kernel-session-controls

## Why

0.4.0 打通了 app-server 真流式,但内核会话控制面(权限交互、模型/思考级别切换、
上下文压缩、中途转向)在 TUI 里完全没有暴露——其中权限交互不只是缺功能:
**app-server 路径下 plan 模式的被门禁工具会触发 `interaction/requestUserInput`
服务器→客户端请求,当前 TUI 完全忽略它,回合挂起直到 600s 兜底**。
2026-07-07 协议 spike 已把全部四个方法的参数形状与交互时序实测钉死
(见 design.md),实现风险低。

## What Changes

- **工具权限确认**:处理 `interaction/requestUserInput` 服务器→客户端请求
  (字符串信封 id `"server-N"`,同 `requestId` 退避重发直到应答),TUI 弹
  确认浮层(↑↓ 选项 / Enter 确认 / Esc 拒绝),应答后回合正常推进;
  同时修复 plan 模式回合挂起。解码层扩展:兼容字符串 id 的服务器请求
  (现在 Response 只认 u64)。
- **/model 与 /think**:`session/setModel`(候选取 `state.updated` 的
  `model.available[].ref`)与 `session/setThoughtLevel`(值域
  `thoughtLevel.available`);`reason:"mode_changed"` 状态推送为权威回显。
- **/compact**:`session/compact {sessionId}`,接到 ≥80% 水位提示上
  (原来只能 /new 丢会话);压缩中状态显示与完成回执。
- **Steer 中途转向**:app-server 流式回合进行中输入 Enter 不再排队,而是
  `session/steer {sessionId, content}` 进当前回合;非 app-server 路径保留
  排队行为。
- **/mode 切换升级**:app-server 路径下 Shift+Tab / /mode 改走
  `session/setMode`(mode ∈ plan|build|edit|yolo|auto,已实测),
  不再只改下次 spawn 的 CLI 参数。

全部仅 app-server 路径;`--prompt` 路径行为不变。任何方法失败按既有降级
纪律:提示错误、不 kill 会话、不影响后续 prompt。

## Capabilities

### New Capabilities

- `tool-permission-approval`: 消费 `interaction/requestUserInput` 服务器→
  客户端请求(含字符串 id 解码、requestId 去重、退避重发容忍),确认浮层
  UI,应答信封,Esc 拒绝语义,plan 模式回合不再挂起
- `session-controls`: /model、/think、/compact 本地命令与 /mode 升级,
  基于 session/setModel、setThoughtLevel、compact、setMode;
  `mode_changed` 状态推送回显
- `turn-steering`: 流式回合中 Enter 即 steer(app-server 路径),
  非流式/非 app-server 路径保留排队

### Modified Capabilities

- `app-server-client`(app-server-streaming 变更引入,待归档):协议信封
  解码扩展——服务器→客户端请求(`{id: string, method, params}`)作为新
  消息类别分派,不再被当作无法解析的行忽略

## Impact

- `src/lib.rs`:`decode_app_message` 扩展(字符串 id 的服务器请求)、
  应答编码、`AppServerMessage` 新变体、interaction params 解析(纯函数,
  可单测);`AppServerConn` 无需改动
- `src/main.rs`:确认浮层(复用 session-picker 浮层模式)、
  `pump_app_turn` 分派 interaction 请求、/model /think /compact 命令、
  steer 输入路由、/mode 的 app-server 分支
- `tests/core.rs`:解码/编码/去重单测;`tests/pty_smoke.py`:plan 模式
  确认浮层冒烟(pyte `screen_seen` 断言)
- 无新依赖;`--prompt` 路径与既有降级纪律不动
