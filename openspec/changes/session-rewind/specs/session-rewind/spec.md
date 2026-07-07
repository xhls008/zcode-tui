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
`external_modified`)确认项 MUST 默认落在"取消",并显式警示:继续应用会
**强制覆盖**磁盘上的外部修改(实测 `session/rewind` 不做安全检查,照常
还原);用户必须显式选择"强制应用"才能继续。

#### Scenario: 安全预览
- **WHEN** 选中 latestCheckpoint,预览返回 `{"canApply":true,"safeFiles":[{"action":"restore","operationCount":1,"path":"…/a.txt","toolNames":["Write"]}],"unsafeFiles":[],…}`
- **THEN** 预览页列出将被还原的文件与来源工具,确认项默认在"应用"

#### Scenario: 不安全预览需显式强制
- **WHEN** 预览返回 `canApply:false`,unsafeFiles 含 `{"reason":"external_modified","expectedHash":"…","currentHash":"…"}`
- **THEN** 预览页警示该文件已被外部修改、继续将强制覆盖;确认项默认在"取消",仅当用户显式选中"强制应用"并 Enter 才发 session/rewind

#### Scenario: 预览失败不中断会话
- **WHEN** previewFileRewind 返回错误响应(如 -32602 Invalid params)
- **THEN** push_error 显示错误,浮层退回目标选择,会话与后续 prompt 不受影响

### Requirement: scope 选择与应用
确认应用时 TUI SHALL 允许选择 scope ∈ {conversation, workspace, both}
(默认 workspace),发
`session/rewind {sessionId, target, scope}`;成功后把 result 的
`response` 一行文案落 transcript,并以 `state.updated
reason=="session_rewound"` 的 patch 刷新控制面缓存(mode/model/
thoughtLevel)。conversation/both 的回滚 SHALL 同步收缩本地 transcript
的会话视图或明确标注"对话已回滚至 …"(以 result.snapshot.messages 为准),
且本会话候选检查点列表按目标裁剪。

#### Scenario: workspace 回滚落盘
- **WHEN** 对 latestCheckpoint 以 scope:"workspace" 应用,该检查点为最近一次 Write 的前像
- **THEN** 磁盘文件内容原地还原(实测 a.txt 由 "two" 还原为 "one"),transcript 显示 "Rewound workspace to checkpoint …: restored 1 file.",随后收到 reason=="session_rewound" 的状态推送

#### Scenario: conversation 回滚收缩对话
- **WHEN** 以 target {kind:"turn", turnIndex:0}、scope:"conversation" 应用
- **THEN** 内核对话回滚到首条用户消息之前(实测 keptMessageCount:0,session/messages 仅剩 1 条 synthetic rewind notice),TUI 标注对话已回滚,不把 model-only 的 synthetic 消息当普通消息渲染

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
