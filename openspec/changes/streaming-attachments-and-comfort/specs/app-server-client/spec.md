# app-server-client

## MODIFIED Requirements

### Requirement: 会话生命周期
客户端 SHALL 按 `session/create {workspace:{workspaceKey, workspacePath}}` →
`session/subscribe {sessionId, deliveryKind:"desktop-continuous"}` →
`session/send {sessionId, content}` 的顺序驱动;sessionId 从 create 结果的
`session.sessionId` 取出;取消发 `session/stop {sessionId}`。
**丢弃活会话时(/new 重开、带活会话的干净退出)客户端 SHALL 尽力发送
`session/close {sessionId}`(params 实测 strict,仅 sessionId)——
fire-and-forget:响应与错误一律静默,连接已死则跳过,MUST NOT 阻塞
丢弃/退出流程。**

#### Scenario: 建会话取回 sessionId
- **WHEN** 发送 session/create 并收到含 `session.sessionId` 的结果
- **THEN** 后续 subscribe/send/stop 复用该 sessionId

#### Scenario: 取消
- **WHEN** 运行中用户取消
- **THEN** 发送 session/stop;连接关闭时进程组兜底清理,无残留子进程

#### Scenario: /new 丢弃活会话发 close
- **WHEN** 存在活跃流式会话时用户执行 /new
- **THEN** 发送 session/close {sessionId} 后再清 app_session;close 失败不影响重开

#### Scenario: 退出时收尾
- **WHEN** 带活跃流式会话执行 /exit
- **THEN** 尽力发送 session/close 后正常退出,不等待响应

### Requirement: session/event delta 累加
客户端 SHALL 消费 `session/event` 通知的 `params.payload.{kind, delta,
done, assistantMessageId}`:`kind=text_delta` 时把 `delta` 累加进本回合
助手正文;`text_start/text_end/reasoning_delta/tool_input_delta/tool_call/
finish` 分派到对应状态;未知 kind MUST 安静忽略。`state.updated` 通知
携带 session 级 status/mode/model 与上下文水位,分开处理。
**payload 缺失 `kind` 的 session/event(checkpoint.created 等会话级事件)
MUST 以 `params.type` 直通为事件 kind 分派(payload 的 fileCount 一并
提取),不再当无法解析的行丢弃;type 也缺失时安静忽略。**

#### Scenario: 正文 token 累加
- **WHEN** 依次收到 kind=text_delta 的 delta "1","\n2",...
- **THEN** 本回合助手正文随之增长,供 transcript 增量渲染(真流式)

#### Scenario: 未知事件 kind
- **WHEN** 收到未见过的 kind 或结构异常的 payload
- **THEN** 安静忽略该事件,不中断流

#### Scenario: checkpoint.created 直通
- **WHEN** 收到 `{"method":"session/event","params":{"type":"checkpoint.created","payload":{"fileCount":1,…}}}`
- **THEN** 解码为 kind=checkpoint.created、file_count=1 的事件供回合统计,不被丢弃
