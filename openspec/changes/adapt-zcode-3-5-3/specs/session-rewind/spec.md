# session-rewind Specification (delta)

## MODIFIED Requirements

### Requirement: 回滚候选按协商协议产生

V4 可用时，TUI MUST 从最新 `v4/conversation/frame` 的 rows 产生含
`rowId` 与 `entityId` 的稳定候选，并按 availability 过滤不可操作目标；
不得把 legacy checkpoint target 发给 3.5.3。V4 不可用时 SHALL 保留现有
`checkpoint.created` 累积与 latestCheckpoint 候选。

#### Acceptance Criteria

- Given 3.5.3 snapshot 含可回滚 row, when 打开 /rewind, then 候选携带该
  rowId/entityId，后续请求不含 `{kind:"latestCheckpoint"}`。
- Given snapshot row 被 guard/availability 标为不可用, when 渲染 picker,
  then 该 row 不可确认并显示原因。
- Given 3.3.6 无 V4, when 收到 checkpoint.created, then 既有 checkpoint
  候选行为保持不变。
- Given 切换会话, when 打开 /rewind, then 不出现前一 session 的 rows 或
  checkpoints。

### Requirement: 回滚预览使用对应协议且始终先行

V4 可用时，确认候选 MUST 先调用
`v4/conversation/fileRewindPreview`，携带 sessionId、row target 与最新
revision/logEpoch；V4 不可用时继续调用 legacy
`session/previewFileRewind`。任一路径 MUST 在应用前展示 safe/unsafe 文件，
unsafe 或 guard.actionUnavailable MUST 阻止应用。

#### Acceptance Criteria

- Given 3.5.3 可操作 row, when 用户 Enter, then 发送 V4 preview 且 CAS base
  来自最新 frame，不调用 legacy previewFileRewind。
- Given preview 返回 unsafe 文件或 actionUnavailable, when 用户确认, then
  不发送 apply command、不修改磁盘并显示拒绝原因。
- Given 3.3.6 checkpoint 候选, when 用户 Enter, then 既有 legacy preview
  形状和 UI 保持不变。

### Requirement: scope 选择与安全应用

V4 文件回滚 MUST 通过 `v4/command` type `applyFileRewind` 应用 row target，
并以 command ack 与后续 frame 判定结果；不得在 V4 内核上调用已删除的
`session/applyFileRewind` 或 `session/rewind`。旧内核 SHALL 保留现有安全
legacy 文件/对话回滚流程。若 3.5.3 未验证对话 scope 的等价 V4 命令，UI
MUST 明确禁用该 scope，而不是猜测命令。

#### Acceptance Criteria

- Given 3.5.3 preview 安全且用户确认 workspace, when 应用, then 发 V4
  applyFileRewind command，成功后磁盘与新 frame 一致。
- Given V4 apply 被拒或 CAS 陈旧, when 处理 ack, then 不宣称成功、不调用
  legacy 兜底，保留会话可用。
- Given 3.5.3 对话回滚语义尚未钉死, when scope picker 打开, then
  conversation/both 不可选并解释限制。
- Given 3.3.6 legacy 会话, when 应用 workspace/conversation, then 当前
  preview-first、unsafe refusal 与 message-target 纪律保持不变。

### Requirement: 回执判定与失败纪律

TUI MUST 从所选协议的语义结果判成功。V4 使用 command ack、
`inputDisposition`/结果字段与后续 frame；legacy 继续使用既有
`rewind.triggered.strategy` 或 apply result。收到 result 信封、本地 pending
标记或乐观文案均不是充分成功条件。

#### Acceptance Criteria

- Given V4 command 信封有 result 但 ack rejected, when 呈现, then 按失败
  显示且不改变候选成功状态。
- Given removed legacy method 返回 Method not found, when 运行 3.5.3 PTY
  场景, then 测试失败且 UI 不显示回滚成功。
- Given 任一回滚控制请求失败, when 后续发送普通 prompt, then 同一
  app-server 会话仍可继续使用。
