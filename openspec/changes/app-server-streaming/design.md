# design — app-server-streaming

## Context

逆向实测(记忆 [[zcode-app-server-protocol]]):`zcode app-server` 是换行分隔
JSON 的 stdio 协议,信封 `{id, method, params}`(非 JSON-RPC,带 jsonrpc 被拒),
MCP 衍生。已跑通:`session/create {workspace:{workspaceKey,workspacePath}}` →
`session/subscribe {sessionId}` → `session/send {sessionId, content}` →
`{accepted:true}`,流式经 `state.updated {patch, reason, revision, scope}` 通知
按修订号增量推送。内核内部是 Anthropic 式 text_delta,app-server 投影成状态补丁。

现有约束不变:lib.rs 纯逻辑可单测;prompt 子进程进程组取消;GLM 蓝单一强调色;
「官方 TUI 发布即退位」「不主动追协议」两条边界纪律。

## Goals / Non-Goals

**Goals:**
- 单轮纯问答也能真流式(token 增量实时渲染)
- 试验开关隔离 + 失败无缝降级 `--prompt`,默认行为零变化
- 一个可复用、可单测的协议客户端地基(后续边界功能共用)

**Non-Goals:**
- 不做 setModel/compact/rewind/fork/steer/权限中继(各自后续立项)
- 不默认启用;稳定前 `--prompt` 始终是默认与兜底
- 不引第三方 MCP/ACP crate(协议是定制变体,自研最小客户端更可控)

## Decisions

**D1. 协议客户端自研,不引 crate。** 信封是定制的(非标准 JSON-RPC),
MCP crate 反而不匹配;最小手写编解码 + serde_json 足够,依赖零新增,
也符合「单一静态二进制」定位。

**D2. lib.rs 放纯逻辑,main.rs 放 IO。** lib 层:信封 encode/decode、
`state.updated` patch 应用到会话状态模型、降级判定——全部无 IO、可单测。
main 层:spawn app-server 子进程、读写 stdio、把协议事件泵进 80ms 主循环
(与现有 JobEvent 泵同构)。

**D3. 直接 delta 流,不是状态补丁(1.1 spike 修正)。**
实测:正文经 **`session/event`** 通知直接吐 token delta——
`params.payload.{kind, delta, done, assistantMessageId}`,`kind=text_delta`
时累加 `delta` 即流式正文(像 Anthropic `content_block_delta`,比预估的
CRDT patch 模型简单得多)。本地维护 `AppServerTurn { message_id,
assistant_text, tool_calls }`,按 `kind` 分派累加。事件带 `eventSeq`;
subscribe 用 `deliveryKind:"desktop-continuous"` 开流,`afterSeq` 可重放。
`state.updated`(独立通知)携带 session 级 status/mode/model/context 水位,
是工具/水位的权威来源。正文增长直接增量 append 进 transcript(真流式)。

**D4. 试验开关 + 降级是一等公民。** `ZCODE_TUI_APP_SERVER=1` 才启用;
未设则完全走现有 `prompt_command_for` + `--prompt`。启用后任一环节
(spawn 失败、握手超时、schema 不认、连接断) → push 一条 dim 系统提示 +
**本进程永久降级**回 `--prompt`,当前这条 prompt 用 `--prompt` 重试一次。
用户永不卡死。

**D5. 取消:`session/stop {sessionId}`。** app-server 是长驻子进程(进程组),
prompt 期间不 spawn 新进程。Esc/Ctrl+C → 发 `session/stop`;连接进程组
killpg 仅在退出/降级时兜底清理。

**D6. 会话连续性复用协议原生会话。** app-server 的 sessionId 就是内核会话;
`--continue`/`--resume` 语义映射到 `session/resume`。首条 prompt
`session/create`,后续复用同一 sessionId(与现有「首条成功后 continue」
状态机对齐)。

**D7. 权威事件优先于 db 轮询。** 开关开启时,工具 chip/reasoning/上下文水位
改吃 `state.updated`(权威、无锁、无 schema 白名单风险);db 轮询仅在
`--prompt` 路径继续。两条路径互不干扰。

## Risks / Trade-offs

- [协议未公开、随内核升级变] → 试验开关 + 默认 --prompt + 失败降级,
  风险完全隔离在 opt-in 路径;协议版本号校验,不认即降级
- [patch shape 逆向不全] → 阶段 1 只消费正文 patch(已实测的最小闭环),
  工具/权限 patch 阶段 2 再加;未知 patch 字段安静忽略,不崩
- [长驻子进程生命周期] → 进程组 + Drop 兜底 killpg;握手/心跳超时降级
- [与现有 --prompt 路径行为分叉] → 开关关闭时代码路径完全不变,
  单测覆盖两条路径

## Open Questions

- `state.updated` 正文 patch 的确切字段路径(assistant 消息内容在 patch 里
  的 JSON pointer)需在阶段 1 PoC 里用真实会话抓一次钉死——已列为 tasks 第一项。
- `session/send` 后正文 patch 的推送节奏(每 token 还是每 N token)待实测,
  影响渲染重绘频率;不影响架构。
