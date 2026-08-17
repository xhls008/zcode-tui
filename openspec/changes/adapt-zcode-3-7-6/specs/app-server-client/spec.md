# app-server-client Specification (delta)

## ADDED Requirements

### Requirement: 0.16.3 运行偏好反向请求

客户端 MUST 在会话 create/resume 握手期间识别服务器请求
`session/requestRuntimePreferences`，并以原信封 id 回应完整运行偏好对象。
回应 MUST 包含 `nativeSearchEnhancementsEnabled:true`、`memoryEnabled:false`、
`askUserQuestionAutoResolutionEnabled:true` 与
`modelContextBudgetStrategy:"preflight-v1"`，不得添加 strict schema 未允许的
字段。该请求在活跃回合或 idle 阶段再次出现时 MUST 使用同一路径应答。

#### Acceptance Criteria

- Given 0.16.3 在 `session/create` 中发送字符串 id `server-1` 的运行偏好请求，
  when TUI 处理握手，then 立即回传同一 id 与四个必填字段，create 在 15 秒前完成。
- Given 请求 scope 为 `user-execution`，when TUI 应答，then 可省略可选的
  `integratedTerminalShell`，由内核按宿主环境选择 shell。
- Given 0.15.x 不发送该方法，when 建立会话，then 既有 create/subscribe/send
  字节形状与交互审批行为保持不变。
- Given 未知服务器请求，when 分派，then 不构造伪造结果；保持既有忽略纪律。
