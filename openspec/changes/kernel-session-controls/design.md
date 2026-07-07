# kernel-session-controls · design

## Context

0.4.0 的 app-server 路径(变更 app-server-streaming,17/17 完成)只消费了
prompt 流:send → session/event 增量 → state.updated 终止。内核方法面里的
会话控制(setMode/setModel/setThoughtLevel/compact/steer)与服务器→客户端
交互请求(interaction/requestUserInput)未接。2026-07-07 spike(真内核
0.15.0,zod 错误探形 + 实弹调用)已钉死:

- `session/setMode {sessionId, mode∈plan|build|edit|yolo|auto}` → 实测成功,
  推 `state.updated reason:"mode_changed"`,patch 携带
  `mode.current / model.{available[],current,lastUsed} / permission.mode /
  thoughtLevel.{available[],current,enabled}` —— 一次推送即拿到全部控制面
  当前值与候选值,是 /model /think /mode 的权威数据源。
- `session/setModel {sessionId, model:{modelId,providerId}}`(shape 即
  `model.available[].ref`);`session/setThoughtLevel {sessionId,…}`;
  `session/compact {sessionId}`;`session/steer {sessionId, content}`。
- 权限交互:plan 模式下门禁工具(实测 ExitPlanMode 包装文件写)触发
  `interaction/requestUserInput`,**字符串信封 id** `"server-N"`,同
  `requestId` 以 ~1s/2s/4s/8s/10s 退避重发直到应答;params 全形已抓取
  (prompt/questions/options/requestId/schema/toolName/toolCallId/turnId);
  应答 `{"id":"server-N","result":{"requestId":…,"answers":{header:value}}}`
  → 重发停止、0.1s 后 `prompt_completed` 干净收尾。
- 已知缺口:approve 应答后回合直接结束,文件并未创建——approve 的完整语义
  (是否隐含切模式 + 需要客户端续跑)未钉死。
- 现状 bug:TUI 的 decode 只认数字 id 的 Response,交互请求被当垃圾行忽略
  → plan 模式回合挂到 600s 兜底。

## Goals / Non-Goals

**Goals:**

- 交互请求全链路:解码(字符串 id)→ requestId 去重 → 浮层 → 应答编码 → 回合推进
- /model、/think、/compact 本地命令 + /mode(Shift+Tab)升级为 setMode(仅 app-server 路径)
- 流式回合中 Enter → steer(仅 app-server 路径;失败退回排队)
- 控制面状态回显统一走 `mode_changed`/state 推送(不自行乐观更新)

**Non-Goals:**

- fork / rewind / goal / usage 等其余方法(下一批)
- `--prompt` 路径的任何行为变化;app-server 默认开启(仍 opt-in)
- 自定义权限策略(allowlist 等)——只做内核问什么答什么

## Decisions

1. **消息三分类而不是 hack**:`AppServerMessage` 加 `ServerRequest {id: serde_json::Value, method: String, params: Value}` 变体
   (id 原样保存,应答时原样回传,天然兼容字符串/数字)。备选:把字符串 id
   hash 成 u64 塞进 Response——被否,应答必须原样回传 id。
2. **去重在 lib 纯函数层**:`InteractionRequest::parse(params)` 返回结构体
   (request_id/prompt/questions/options/tool_name),TUI 侧用
   `HashMap<request_id, envelope_id>` 维护待应答项;重发只更新 envelope_id。
   应答优先用**最新**信封 id(spike 显示旧 id 也可能被接受,但最新最稳;
   实现期任务里有一发验证 spike)。
3. **浮层复用 session-picker 模式**:同样的 List 覆盖层 + ↑↓/Enter/Esc 键路由,
   不引入新 UI 框架。浮层打开期间流式渲染继续(回合还在跑)。
4. **Esc=拒绝的应答值**:questions.options 实测只有 approve 一项;拒绝语义
   实现期 spike 钉(候选:answers 填非法值/空、或直接 session/stop)。
   spec 里已写兜底:无协议级拒绝则关浮层 + session/stop。
5. **控制命令都是 fire-and-forget + 状态回显**:setModel/setMode/…发出后不
   阻塞 UI,回显一律等 state 推送(mode_changed 等),与流式路径的非阻塞
   纪律一致。失败(错误响应按请求 id 关联)→ push_error,会话不动。
6. **steer 的输入路由**:`handle_key` 的 Enter 分支在 `app_turn.is_some()`
   时改走 steer(而非 queued.push_back);握手中(app_connect)与 drain 中
   保持排队。备选:新快捷键触发 steer——被否,Codex 惯例就是直接打字。

## Risks / Trade-offs

- [approve 后不续跑,用户困惑] → 实现期 spike 钉死 approve 语义;若内核
  期望客户端在 approve 后自行 setMode+续跑,则在应答后按 schema.interaction
  == "plan_approval" 补 setMode(build) + 原 prompt 重发;spec 场景先只承诺
  "回合不挂起、门禁生效"。
- [重发应答竞态:应答旧信封 id 恰逢新重发在途] → 已应答 requestId 全部
  静默丢弃,即使再弹也不会重复应答。
- [控制方法在旧内核不存在] → 错误响应按 id 关联到该命令,push_error 即可,
  不触发降级(降级只留给连接级失败)。
- [steer 语义与用户预期(打断 vs 补充)不一致] → transcript 里把 steer
  输入标注清楚;Esc 取消路径保持不变可兜底。

## Open Questions(已由 1.4 spike 回答,2026-07-07)

- **approve 后内核是否期望客户端续跑 → 是**:实测 approve 应答后
  `prompt_completed` 到达时 mode 仍是 plan(不翻转),后续 prompt 的 Write
  工具被调用但文件不落地(门禁仍生效)。客户端必须自行
  setMode(build) + 队列续跑提示("Proceed with the approved plan."),
  已实现于 answer_interaction。
- **拒绝的协议级表达 → 不冒险**:options 只有 approve;应答值域外的值语义
  未知(可能被误读为批准)。Esc 采用 spec 兜底:关浮层 + session/stop
  (走既有取消/drain 路径),确定性且安全。
- **setThoughtLevel 字段名 → `thoughtLevel`**:实测
  `{sessionId, thoughtLevel:"disabled"}` → RESP ok +
  `state.updated reason:"thought_level_changed"`。
