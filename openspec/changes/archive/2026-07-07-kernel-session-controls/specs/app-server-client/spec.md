# app-server-client

## MODIFIED Requirements

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
