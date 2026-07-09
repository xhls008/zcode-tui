# session-rewind Specification(delta)

## ADDED Requirements

### Requirement: 检查点事件的解析与累积
解码层 MUST 把 `session/event` 通知中 `params.type == "checkpoint.created"`
的 payload 解析为检查点记录——字段为实测形状:`checkpointId`、`messageId`、
`targetMessageId`、`toolMessageId`、`scope`、`snapshotRef`、`diffRef`、
`fileCount`;缺 `checkpointId` 时返回 None 容错,不得让整行解码失败。
解析为 lib.rs 纯函数,可单测;TUI 按当前会话累积为回滚候选列表
(新会话 / 切换会话时清空)。

#### Scenario: 实测 payload 被解析入列
- **WHEN** 收到 `{"method":"session/event","params":{"type":"checkpoint.created","payload":{"checkpointId":"checkpoint_…","messageId":"msg_…","toolMessageId":"msg_…","scope":"workspace","snapshotRef":"zcode-artifact://…","diffRef":"zcode-artifact://…","fileCount":1},…}}`
- **THEN** 解析出检查点记录并追加进当前会话的候选列表,其余 session/event 消费(delta 累加等)不受影响

#### Scenario: 缺关键字段容错
- **WHEN** payload 缺 `checkpointId`
- **THEN** 解析返回 None,该通知被安静跳过,不报错、不中断事件流

### Requirement: /rewind 目标选择浮层
TUI SHALL 提供 `/rewind` 本地命令(收录进 slash 补全与 /help):打开目标
选择浮层,候选为本会话累积的检查点(新→旧,显示 checkpointId 短形式、
fileCount 与捕获顺序)加固定项 `latestCheckpoint`(最近检查点);
↑↓ 选择、Enter 进入预览、Esc 关闭浮层。回合流式进行中 `/rewind`
SHALL 被拒绝并提示(先 Esc 取消或等回合结束),不与在途 turn 竞争。

#### Scenario: 打开浮层
- **WHEN** app-server 会话空闲,本会话已捕获 2 个检查点,用户输入 /rewind
- **THEN** 浮层列出 latestCheckpoint 与 2 个检查点(新→旧),↑↓ 可选,Esc 关闭后无任何请求发出

#### Scenario: 无检查点
- **WHEN** 本会话未捕获任何检查点,用户输入 /rewind
- **THEN** 提示"本会话暂无检查点",不打开空浮层、不发请求

#### Scenario: 回合进行中拒绝
- **WHEN** 流式回合进行中用户输入 /rewind
- **THEN** 提示回合结束后再试,不发任何 rewind 家族请求

### Requirement: 回滚预览(previewFileRewind)
在浮层中 Enter 选中目标后,TUI MUST 先发
`session/previewFileRewind {sessionId, target}`,并按实测结果形状渲染预览:
`canApply`、`safeFiles[{action, path, operationCount, toolNames}]`、
`unsafeFiles[{path, reason, expectedHash, currentHash, operationCount,
toolNames}]`、`ignoredFiles`。`canApply == false` 时(实测 reason 如
`external_modified`)文件 scope 的应用 MUST 被本地拒绝并警示——实测裸
`session/rewind` 会**强制覆盖**磁盘上的外部修改(无视 canApply),因此
文件回滚一律走尊重安全检查的 `session/applyFileRewind`,不提供强制覆盖
路径(conversation scope 不动文件,仍可用)。

#### Scenario: 安全预览
- **WHEN** 选中 latestCheckpoint,预览返回 `{"canApply":true,"safeFiles":[{"action":"restore","operationCount":1,"path":"…/a.txt","toolNames":["Write"]}],"unsafeFiles":[],…}`
- **THEN** 预览页列出将被还原的文件与来源工具,Enter 即可应用

#### Scenario: 不安全预览拒绝文件回滚
- **WHEN** 预览返回 `canApply:false`,unsafeFiles 含 `{"reason":"external_modified","expectedHash":"…","currentHash":"…"}`
- **THEN** 预览页警示该文件已被会话外修改、文件 scope 应用被拒绝(不发 applyFileRewind、绝不发文件 scope 的 session/rewind);conversation scope 仍可选

#### Scenario: 预览失败不中断会话
- **WHEN** previewFileRewind 返回错误响应(如 -32602 Invalid params)
- **THEN** push_error 显示错误,浮层退回目标选择,会话与后续 prompt 不受影响

### Requirement: scope 选择与应用
确认应用时 TUI SHALL 允许选择 scope ∈ {workspace, conversation, both}
(默认 workspace)。文件回滚(workspace 与 both 的文件段)MUST 走
`session/applyFileRewind {sessionId, target}`(结果
`{applied, preview, response}` 实测钉死,`applied` 为权威判据;拒绝时
`applied:false` 携带 unsafeFiles 与原因)。对话回滚(conversation 与
both 的对话段)MUST 把 picker 目标翻译为
`{kind:"message", messageId:<checkpoint.targetMessageId>}` 后发
`session/rewind {sessionId, target, scope:"conversation"}`——实测
(2026-07-09)checkpoint 类目标会被内核**无视 scope 强制转为文件回滚**
(rewind.triggered 回报 scope:"workspace" 并删除了外部修改过的文件),
仅 message 目标尊重 conversation scope;拿不到 messageId 时对话段
MUST 被拒绝并提示,不得发 checkpoint 目标。both = applyFileRewind
成功后再链 conversation 段,文件段被拒则不动对话。成功后把 `response`
一行文案落 transcript,`state.updated reason=="session_rewound"` 的
patch 刷新控制面缓存;conversation 回滚成功后明确标注对话已回滚,且
本会话候选检查点列表按目标裁剪。

#### Scenario: workspace 回滚落盘
- **WHEN** 对 latestCheckpoint 以 scope workspace 应用(applyFileRewind),该检查点为最近一次 Write 的前像
- **THEN** 磁盘文件内容原地还原(实测 a.txt 由 "two" 还原为 "one"),transcript 显示应用回执文案,随后收到 reason=="session_rewound" 的状态推送

#### Scenario: 文件段被拒不碰对话(both)
- **WHEN** scope both 应用时 applyFileRewind 返回 `{"applied":false,"response":"File rewind was not applied because at least one file is unsafe.",…}`
- **THEN** 按失败呈现拒绝原因与 unsafeFiles,不再发 conversation 段的 session/rewind,会话保持可用

#### Scenario: conversation 回滚收缩对话且不碰文件
- **WHEN** 选中检查点后以 conversation scope 应用,TUI 发 `{kind:"message", messageId:<该检查点的 targetMessageId>}` + scope:"conversation"
- **THEN** rewind.triggered 回报 scope:"conversation"(实测),对话收缩(session/messages 仅剩 synthetic rewind notice),磁盘文件分毫未动(实测外部篡改的内容原样保留),TUI 标注对话已回滚

#### Scenario: 无 messageId 时对话段被拒
- **WHEN** conversation/both scope 应用时该目标对应的检查点缺 targetMessageId
- **THEN** 提示对话回滚不可用,不发任何 session/rewind(checkpoint 目标会被内核强制转文件回滚,实测),文件段(both)也不发

### Requirement: 回执判定与失败纪律
TUI MUST NOT 以"收到 result 信封"作为回滚成功的判据:实测目标检查点不存在
时内核仍返回**成功信封**,`response` 为 "Checkpoint … was not found.",
且 `rewind.triggered` 事件 payload 为
`{strategy:"unavailable", reason:"target_checkpoint_not_found"}`。成功判定
SHALL 基于 `rewind.triggered` payload 的 `strategy != "unavailable"`
(或等价地识别失败 reason),失败按错误样式呈现文案。任何 rewind 家族
方法的错误响应(实测 -32602 Invalid params / ZodError)遵循既有控制命令
纪律:push_error 报告、不 kill 会话、不影响后续 prompt。

#### Scenario: 假成功按失败呈现
- **WHEN** session/rewind 目标 checkpointId 不存在,收到成功信封但 rewind.triggered payload 为 `{"strategy":"unavailable","reason":"target_checkpoint_not_found"}`
- **THEN** TUI 按失败样式显示 "Checkpoint … was not found.",不宣称回滚成功,候选列表不变

#### Scenario: 方法错误不 kill 会话
- **WHEN** rewind 家族请求返回 `{"error":{"code":-32602,"message":"Invalid params",…}}`
- **THEN** push_error 显示错误,会话保持可用,后续 prompt 正常发送

### Requirement: 仅 app-server 路径
/rewind 及全部 rewind 家族调用 MUST 仅存在于 app-server 流式路径;
非 app-server 会话(`--prompt` 路径,含降级后)输入 /rewind SHALL 仅
提示"需要 app-server 会话",不发任何请求,其余行为与本变更前完全一致。

#### Scenario: --prompt 路径提示不可用
- **WHEN** ZCODE_TUI_APP_SERVER 未启用或已降级,用户输入 /rewind
- **THEN** 仅提示需要 app-server 会话,无浮层、无请求,既有路径行为不变
