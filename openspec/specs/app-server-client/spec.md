# app-server-client Specification

## Purpose
TBD - created by syncing change app-server-streaming. Update Purpose after archive.
## Requirements
### Requirement: 协议信封编解码
客户端 MUST 用换行分隔 JSON、信封 `{id, method, params}`(不含 `jsonrpc`
字段)与 app-server 通信;请求编码为紧凑单行 JSON,响应/通知按 `id`(响应)
或 `method`(通知)分派。**同时含 `method` 与 `id` 的行 MUST 识别为
服务器→客户端请求(第三类消息),其 `id` 兼容字符串与数字,分派给交互
处理器;对服务器请求的应答编码为 `{"id":<原信封id>,"result":{...}}`。**
编解码 MUST 为 lib.rs 纯函数,可单测。

#### Scenario: 编码请求
- **WHEN** 构造 `session/create` 请求,params 为工作区对象
- **THEN** 输出单行 `{"id":N,"method":"session/create","params":{...}}\n`,不含 jsonrpc 键

#### Scenario: 分派响应与通知
- **WHEN** 收到 `{"id":1,"result":{...}}` 与 `{"method":"state.updated","params":{...}}`
- **THEN** 前者按 id 关联到请求,后者按 method 路由为通知;无法解析的行安静忽略

#### Scenario: 分派服务器请求
- **WHEN** 收到 `{"id":"server-1","method":"interaction/requestUserInput","params":{...}}`
- **THEN** 识别为服务器→客户端请求并携带原信封 id 分派,不被忽略、不与 Response 混淆

#### Scenario: 应答服务器请求
- **WHEN** 用户完成交互,应答信封 id 为 "server-1" 的请求
- **THEN** 写出单行 `{"id":"server-1","result":{...}}`,id 原样回传(字符串)

### Requirement: 会话生命周期
客户端 SHALL 按 `session/create {workspace:{workspaceKey, workspacePath}}` →
`session/subscribe {sessionId, deliveryKind:"desktop-continuous"}` →
`session/send {sessionId, content}` 的顺序驱动;sessionId 从 create 结果的
`session.sessionId` 取出;取消发 `session/stop {sessionId}`。

#### Scenario: 建会话取回 sessionId
- **WHEN** 发送 session/create 并收到含 `session.sessionId` 的结果
- **THEN** 后续 subscribe/send/stop 复用该 sessionId

#### Scenario: 取消
- **WHEN** 运行中用户取消
- **THEN** 发送 session/stop;连接关闭时进程组兜底清理,无残留子进程

### Requirement: session/event delta 累加
客户端 SHALL 消费 `session/event` 通知的 `params.payload.{kind, delta,
done, assistantMessageId}`:`kind=text_delta` 时把 `delta` 累加进本回合
助手正文;`text_start/text_end/reasoning_delta/tool_input_delta/tool_call/
finish` 分派到对应状态;未知 kind MUST 安静忽略。`state.updated` 通知
携带 session 级 status/mode/model 与上下文水位,分开处理。

#### Scenario: 正文 token 累加
- **WHEN** 依次收到 kind=text_delta 的 delta "1","\n2",...
- **THEN** 本回合助手正文随之增长,供 transcript 增量渲染(真流式)

#### Scenario: 未知事件 kind
- **WHEN** 收到未见过的 kind 或结构异常的 payload
- **THEN** 安静忽略该事件,不中断流

### Requirement: 连接健壮性与协议校验
客户端 MUST 在 app-server 起不动、协议版本或 schema 不符、握手超时、连接
中断时判定为不可用并返回可降级的错误,MUST NOT panic 或卡死。

#### Scenario: app-server 不可用
- **WHEN** spawn app-server 失败或握手超时
- **THEN** 返回不可用错误,调用方降级(见 streaming-prompt),至多一条 dim 提示
