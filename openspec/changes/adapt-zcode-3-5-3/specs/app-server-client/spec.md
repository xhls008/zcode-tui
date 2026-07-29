# app-server-client Specification (delta)

## ADDED Requirements

### Requirement: legacy 会话上的可选 V4 控制面协商

客户端 MUST 继续用 legacy `session/create|resume`、`session/subscribe`、
`session/send` 和 `session/event` 驱动正文流；取得 sessionId 后 MUST 尝试
`v4/conversation/subscribe`。订阅成功时 SHALL 缓存 frame 的 `revision`、
`logEpoch`、`rows.window[]`、`inputRouting` 与相关 `availability`；Method not
found MUST 被视为旧内核能力结果，不得中断 legacy 流。

#### Acceptance Criteria

- Given 3.5.3 返回订阅 ack 与 snapshot, when 会话建立, then V4 控制面标记
  available，legacy 正文仍由 session/event 增量渲染。
- Given 3.3.6 对订阅返回 Method not found, when 会话建立, then V4 标记
  unavailable，既有 legacy 控制路径仍可用且不显示致命错误。
- Given V4 frame 增加未知字段或 row kind, when 解码, then 已知状态被更新、
  未知数据被忽略，连接不中断。
- Given /new、切换 session 或连接断开, when 清理会话, then 旧 revision、
  logEpoch 与 rows 不得复用于新会话。

### Requirement: V4 命令遵循各自 CAS schema

每个 `v4/command` MUST 带唯一 commandId/clientId、当前 sessionId、命令
type/payload 与 issuedAt。`setFollowupMode` MUST 带最新 baseRevision；
`applyFileRewind` MUST 同时带最新 baseRevision/baseLogEpoch；`sendText` SHALL
按 3.5.3 schema 不强制附 CAS。客户端 MUST 以命令 status 与随后语义 frame
判定 accepted/rejected；已 accepted 的 `sendText` MUST NOT 被自动重放，避免
重复用户输入。

#### Acceptance Criteria

- Given snapshot revision 42/logEpoch E, when 编码 setFollowupMode, then
  envelope 使用 baseRevision 42 且无需 baseLogEpoch。
- Given 同一状态编码 applyFileRewind, then envelope 使用 baseRevision 42 与
  baseLogEpoch E。
- Given 编码 sendText, then 不要求 CAS 字段，delivery 由随后 frame 判定。
- Given 新 frame 把 revision 更新为 43, when 下一命令发送, then 使用 43。
- Given sendText 因 stale base 被拒, when 处理 ack, then 不自动发送第二份
  文本，原输入回到本地队列并显示明确诊断。

### Requirement: V4 控制失败不破坏 legacy 正文流

V4 订阅、frame 或 command 失败 SHALL 只禁用或回退对应控制能力；只要
legacy app-server 连接仍活着，客户端 MUST NOT kill 会话或把当前正文 turn
误判为失败。

#### Acceptance Criteria

- Given V4 command 返回协议错误而 session/event 仍继续, when 处理错误,
  then 错误被显示且正文继续增长。
- Given V4 控制面不可用, when 用户发送普通 prompt, then 仍使用最小
  `session/send {sessionId,content}`，不附加 strict schema 未允许的猜测字段。
