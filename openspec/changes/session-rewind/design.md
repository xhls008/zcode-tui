# session-rewind · design

## Context

kernel-session-controls(已归档)接完控制面后,rewind 家族仍未接
(其 Non-Goals 明确留给下一批)。2026-07-07 spike(真内核 0.15.0 /
desktop 3.2.5,`~/.local/bin/zcode app-server`,spike_a.py / spike_a2.py /
spike_b.py 三发实弹)已钉死:

- **检查点事件**:build 模式下 Write 获准执行时,内核推
  `session/event`,`params.type == "checkpoint.created"`(判别键在
  params 层,payload 无 kind),payload 实抓:
  `{"checkpointId":"checkpoint_90c0d5df-…","messageId":"msg_…",
  "targetMessageId":"msg_…"(与 messageId 同值,指向该 turn 的用户消息),
  "toolMessageId":"msg_…"(该工具所在助手消息),"scope":"workspace",
  "snapshotRef":"zcode-artifact://sess_…/tool-result-…","diffRef":同
  snapshotRef,"fileCount":1}`。每次被门禁放行的 Write 各产生一个。
- **检查点语义是"工具执行前的前像"**:回滚到第 1 个检查点(创建 a.txt
  前)会**删除** a.txt(synthetic 通知原文
  "restoredFiles: 1 / delete …/a.txt");回滚到 latestCheckpoint
  (第 2 次 Write "two" 的前像)把 a.txt 从 "two\n" 原地还原为
  "one\n"——**磁盘确实还原,已验证**。
- **previewFileRewind 结果形状**(实抓):
  `{"canApply":true,"ignoredFiles":[],"safeFiles":[{"action":"restore",
  "operationCount":1,"path":"…/a.txt","toolNames":["Write"]}],
  "unsafeFiles":[],"sessionId":"sess_…","target":{"kind":"latestCheckpoint"}}`;
  外部篡改文件后变为 `{"canApply":false,"unsafeFiles":[{"operationCount":1,
  "path":"…","reason":"external_modified","toolNames":["Write"],
  "expectedHash":"27dd8e…","currentHash":"23bd61…"}],…}`(文件被删时
  currentHash 为字符串 `"missing"`)。
- **session/rewind 无视 canApply 强制应用**:外部篡改后直接
  rewind scope:"workspace" 依然成功并覆盖篡改内容("external tamper\n"
  被还原为 "one\n")。安全检查只存在于 preview / applyFileRewind。
- **applyFileRewind 结果形状**(实抓):`{"applied":false,"preview":
  {…同 previewFileRewind 结果…},"response":"File rewind was not applied
  because at least one file is unsafe."}`——尊重安全检查的"应用"变体。
- **rewind 的事件时序**(workspace 与 conversation 一致):
  `turn.started`(payload.input 形如 "/rewind checkpoint_…" /
  "/rewind latest" / "/rewind conversation msg_…")→ `rewind.triggered`
  (payload `{rewindId, scope, strategy:"active_chain", targetMessageId,
  targetCheckpointId?, restoredSnapshotRef?, createdMessageId,
  reason:"target_in_active_chain"}`)→ `turn.completed`(payload.response
  即结果文案)→ `state.updated reason:"session_rewound"`(patch 含
  mode/model/permission/thoughtLevel 全量控制面,revision 递增)。
- **rewind result**:`{response:"Rewound workspace to checkpoint …:
  restored 1 file.", snapshot:{messages, projection, protocol, runtime,
  session, settings, slashCommands, todos, todoGroups}}`(同
  session/create 结果形状)。workspace 回滚**不改写对话**,只追加一条
  `synthetic:true, source:"rewind", visibility:"model-only"` 的用户消息
  ("Workspace rewind applied. … Conversation history was not rewritten
  by this file restore.")。
- **conversation 回滚真收缩**:2 个回合后
  `session/rewind {target:{kind:"turn",turnIndex:0},scope:"conversation"}`
  → response "Rewound conversation to before message msg_…";synthetic
  通知含 `keptMessageCount: 0` 与 `rewoundPromptPreview`;随后
  `session/messages {sessionId}` 仅剩 1 条 synthetic rewind notice
  (turnIndex:0 = 回滚到**第一条用户消息之前**)。注意
  projection.turnCount 不回退(rewind 自身占一个 turn,累计计数)。
- **假成功**:`session/rewind` 目标 checkpointId 不存在 → 仍返回**成功
  信封**,`response:"Checkpoint checkpoint_does-not-exist was not
  found."`,`rewind.triggered` payload
  `{strategy:"unavailable", reason:"target_checkpoint_not_found"}`
  (无 targetMessageId),且 `state.updated reason:"session_rewound"`
  照发——不能以信封或 state 推送判成功。
- **错误形状**:`session/read {}` / `session/messages {}` /
  `session/applyFileRewind {}` 均为
  `{"code":-32602,"data":{"name":"ZodError","message":"[…path:[sessionId]
  (applyFileRewind 另有 path:[target])…]"},"message":"Invalid params"}`;
  三方法正参均只需 `{sessionId}`(applyFileRewind 加 `target`)。
  `session/read {sessionId}` 返回同 session/create 形状(权威全量),
  `session/messages {sessionId}` 只返回 `{messages}`。

## Goals / Non-Goals

**Goals:**

- checkpoint.created 事件解析(lib 纯函数)+ 会话内候选累积
- /rewind 浮层:目标选择 → previewFileRewind 预览 → scope 选择 → 应用 → 回执
- 假成功识别(rewind.triggered.strategy)与不安全文件强制覆盖警示
- conversation/both 回滚后的 transcript 收缩/标注

**Non-Goals:**

- fork / rewindCascade / goal / usage(下一批;rewindCascade 仅确认存在,
  未实弹)
- snapshotRef/diffRef(zcode-artifact:// URI)的取回与 diff 渲染
  (无已知取回方法,见 Open Questions)
- 自动/定时回滚、跨会话回滚;`--prompt` 路径任何变化

## Decisions

1. **候选来自客户端累积而非内核查询**:检查点只在 checkpoint.created
   通知里出现(session/read 结果无检查点清单,实测),所以 TUI 按会话
   累积 payload 作为 /rewind 候选,再补固定项 latestCheckpoint 兜底
   (重连/中途订阅导致累积缺失时仍可用)。备选:回滚前 session/read
   反查——被否,结果里没有检查点列表。
2. **判成功只看 rewind.triggered.strategy**:实测假成功(不存在的
   checkpointId)信封、response、session_rewound 推送全部照发,唯一可靠
   判别是 `rewind.triggered` payload `strategy=="unavailable"` +
   `reason`。lib 纯函数 `classify_rewind_outcome(triggered_payload,
   response_text)`,可单测。备选:匹配 response 文案——被否,文案不稳定
   且无 schema。
3. **默认 scope=workspace,预览先行**:UX 主诉求是"模型写坏了,把文件
   还回去";conversation/both 改写对话,风险更高,放在显式 scope 选择步。
   Enter 一律先 previewFileRewind(实测 <0.3s,无模型参与),预览页确认
   后才发 session/rewind——与"确认浮层"既有交互纪律一致。
4. **unsafe 时用 session/rewind 强制、不用 applyFileRewind**:实测
   session/rewind 无条件覆盖、applyFileRewind 尊重 canApply。取
   "preview 把关 + 用户显式'强制应用'后走 session/rewind"一条路,
   不引入第二条应用路径;applyFileRewind 仅作为实现期可选的
   "安全应用"备选记录在案。备选:安全时 applyFileRewind、不安全时
   session/rewind——被否,双路径回执形状不同(applied/preview/response
   vs response/snapshot),状态机复杂化无收益。
5. **conversation 回滚后的 transcript 处理**:以 result.snapshot.messages
   为权威重建会话视图(过滤 `visibility:"model-only"` 的 synthetic 消息,
   只标注一行"对话已回滚"),不自行按 turnIndex 裁剪本地缓存。备选:
   本地裁剪——被否,内核裁剪点(targetMessageId 之前)与本地 turn 边界
   可能有出入,snapshot 是现成权威。
6. **浮层复用 session-picker/权限浮层模式**:三态(目标列表 → 预览+scope
   → 结果),Esc 逐级回退;不引入新 UI 框架。回合进行中禁止 /rewind
   (spec 已定),避免与在途 turn 的事件流交织。
7. **检查点解析容错到 checkpointId 一个必选键**:实测 payload 8 键,
   但只有 checkpointId 是应用侧硬依赖(target 只需要它);其余全部
   Option,内核加减字段不破解析。

## Risks / Trade-offs

- [rewind 强制覆盖外部修改,用户误伤工作区] → 预览 canApply:false 时
  默认"取消"+ 显式"强制应用"两段确认(spec 已定);预览页展示
  expectedHash/currentHash 差异来源。
- [检查点是前像,用户可能预期"回到该次修改之后"] → 浮层文案按
  "还原到 <该次写入> 之前"措辞,预览页列出将被还原/删除的文件路径,
  避免语义误读(spike_a 实测回滚首个检查点会删文件)。
- [重连后候选累积丢失] → latestCheckpoint 固定项兜底;候选列表清空
  规则绑定 sessionId(切会话即清)。
- [conversation 回滚后本地水位/计数与内核不一致] → 一律以
  snapshot.projection(contextUsed/turnCount)与后续 state 推送为准
  刷新,不做本地推算;turnCount 累计不回退是内核语义,状态栏不拿它当
  "对话长度"。
- [session_rewound 推送会在假成功时照发] → 控制面缓存刷新无害(patch
  内容仍正确);成功/失败呈现只走 Decision 2 的判定,不挂在该推送上。
- [both scope 未实弹] → 实现期任务含一发 spike 先钉(见 tasks 1.4);
  spec 场景只对 workspace/conversation 两条实测路径作硬承诺。

## Open Questions

- **snapshotRef/diffRef(`zcode-artifact://…`)如何取回**:未发现读取
  该 URI 的方法(bundle 里未探到对应 method);预览页因此只能展示
  previewFileRewind 的文件清单,拿不到内容级 diff。留待后续 bundle
  探形,不阻塞本变更。
- **scope:"both" 的确切行为**:三值枚举已由 zod 探形钉死,但 both 未
  实弹(本次 spike 只打了 workspace 与 conversation);预期为两者叠加,
  实现期 tasks 1.4 spike 验证后再放开 UI 默认可选。
- **`session/rewindCascade` 的用途**:仅确认方法存在,参数/语义未探;
  疑似"回滚 + 裁剪后续分支"的组合操作,本变更不接。
- **checkpoint 事件是否覆盖 Edit/Bash 等其他写工具**:本次只实测了
  Write(权限门禁路径)。若 Edit 亦产生 checkpoint.created,解析层
  无需改动(payload 同型);若某些写路径不产生检查点,候选列表自然
  缺失——按 fileCount/toolNames 呈现,不做兜底伪造。
