# tasks — app-server-streaming

## 1. 前置 spike:钉死正文 patch 形状

- [x] 1.1 驱动真实 app-server 跑一条 prompt,钉死助手正文的流式机制(验证:spike 脚本输出真实样本)
  - 结论(2026-07-06 实测):真流式**可行且比预估简单**。正确序列:
    `session/create {workspace:{workspaceKey,workspacePath}}` → `session/subscribe
    {sessionId, deliveryKind:"desktop-continuous", includeSnapshot:false}` →
    `session/send {sessionId, content}`。正文经 **`session/event`** 通知流式推送
    (不是 state.updated——那只是 session 级状态 status/mode/model):
    `params.payload.{assistantMessageId, delta, done, kind}`,`kind` 为
    `text_start/text_delta/text_end/reasoning_delta/tool_input_delta/tool_call/finish`。
    累加 `text_delta` 的 `delta` 即得流式正文(实测重建出 `1\n2\n3\n4\n5`,首 delta 7.5s)。
    事件带 `eventId`/`eventSeq`,subscribe 支持 `afterSeq` 重放。
    `state.updated` 并行携带 status/mode/model/context 水位(权威进度来源)。

## 2. app-server-client(lib.rs 纯逻辑)

- [x] 2.1 信封 encode/decode:请求编码(无 jsonrpc)、响应/通知按 id/method 分派、坏行安静忽略(验证:单测 `app_request_envelope_has_no_jsonrpc`、`decode_dispatches_response_event_and_state`)
- [x] 2.2 会话状态模型 + 正文增量重建:`AppServerTurn` 按 kind 累加 text_delta、reasoning_delta 分开、未知 kind 忽略;`state.updated` 水位best-effort递归提取(验证:单测 `turn_accumulates_text_deltas_and_ignores_unknown`、`state_watermark_found_anywhere_in_tree`)
  - 注:工具/权限 patch 属阶段 2(设计 D3 明列),阶段 1 只落正文+reasoning+水位
- [x] 2.3 连接不可用/协议不符判定 → 可降级错误类型 `AppServerUnavailable`(验证:单测 `unavailable_reasons_display`、`app_server_opt_in_switch`)

## 3. app-server-client(main.rs 连接管理)

- [x] 3.1 spawn app-server 长驻子进程(进程组)、stdio 读线程、事件泵 `pump_app_turn` 进 80ms 主循环(与 JobEvent 泵同构)(验证:pty 冒烟 `stream_verify.py` 流式重建)
- [x] 3.2 会话生命周期接线:`app_open_session`(create→subscribe)、`session/send`;首条 create、后续复用 sessionId(`/new` 清空重建);取消 `cancel_current` 发 session/stop;Drop killpg 兜底(验证:pty 冒烟 + 取消路径)

## 4. streaming-prompt(接线与降级)

- [x] 4.1 `ZCODE_TUI_APP_SERVER=1` 开关:开启走 app-server,关闭完全走现有 --prompt(验证:单测 `app_server_opt_in_switch` + pty 冒烟 A 场景开关关不含 app-server 字样)
- [x] 4.2 正文增量渲染:text_delta → transcript 实时 append(真流式);reasoning 进 work panel;水位取自 `state.updated`(验证:pty `stream_verify.py` 逐步出现 `[4,5,16,18,30]`,状态栏 `streaming (app-server)`)
  - 注:工具 chip 走协议属阶段 2(payload 形状未在活内核钉死);阶段 1 流式路径不显 chip,不回退 db 轮询
- [x] 4.3 无缝降级:任一环节失败 → 本进程永久降级 `downgrade_app_server` + 当前 prompt 用 --prompt 重试一次 + dim 提示;断连保留半截正文(验证:pty `path_verify.py` B/B2 注入 app-server 断连,降级提示恰一次)

## 5. 收尾

- [x] 5.1 门禁:fmt --check / clippy -D warnings / cargo test(62 绿)全绿(验证:命令输出)
- [x] 5.2 pty 冒烟场景:开关开真流式(`stream_verify.py`)、开关关 --prompt 不变 + 降级 + 永久性(`path_verify.py`,fake kernel 确定性)+ 工具 chip/折叠(pty s12)(验证:脚本输出)
- [x] 5.3 文档:README 环境变量、设计文档、CHANGELOG 0.4.0 + Cargo.toml bump(验证:通读)

## 6. 阶段 2:工具 chip + 输出折叠(对齐 Codex/CC)

- [x] 6.1 活内核钉死工具事件 payload:`tool_input_start{toolName,toolCallId}` → `tool_input_delta` → `tool_call{input}` → `started` → `result{duration,result:{success,content}}` → `tool_result`(验证:spike 真实样本)
- [x] 6.2 **回合终止修正(关键 bug)**:内核无 `finish`/`done` 事件;回合以 `state.updated {reason:"prompt_completed"}` 结束。`app_state_is_turn_end` 判定;修掉流式回合永停 "streaming" 需等 600s 兜底(验证:单测 `state_update_marks_turn_end_on_prompt_completed` + pty `done (` 现身)
- [x] 6.3 lib:`AppServerEvent` 加 tool 字段、`AppToolCall`、`apply→TurnDelta`(Text/Reasoning/ToolStarted/ToolFinished/Done)、`tool_input_summary`(验证:单测 `turn_tracks_tool_calls_start_to_result`、`tool_input_summary_condenses_json_args`)
- [x] 6.4 main:运行中工具 spinner chip 进 work panel;`result` 到达落 `Tool` 条目(名·输入摘要·耗时 + 输出)可 Ctrl+O 折叠;文本/工具按序交错(text_index + written 偏移)(验证:pty 真实 Read 折叠 `… (+24 lines · Ctrl+O)` + `done (34s)`)
- [x] 6.5 天际线线框图随宽度自适应 `skyline_lines`(ZhiPU 在连续地平线)(验证:单测 `skyline_stretches_to_fill_width_exactly` + pyte 多宽度)
