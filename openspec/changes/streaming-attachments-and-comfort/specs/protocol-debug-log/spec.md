# protocol-debug-log

## ADDED Requirements

### Requirement: ZCODE_TUI_LOG 调试日志
`ZCODE_TUI_LOG=<文件路径>` 设置时,TUI SHALL 以追加方式把协议流量与关键
状态转换写入该文件:入站行记解码后的摘要(消息类别、方法/kind/reason、
id,内容截断),出站请求 MUST 只记方法名与 id;握手阶段推进、回合收尾、
降级 SHALL 各记一行。环境变量未设置时 MUST 零开销(启动判定一次,
运行路径只剩一个空判断);日志文件打开或写入失败 MUST 静默降级,
不影响任何功能。

#### Scenario: 记录一次流式回合
- **WHEN** ZCODE_TUI_LOG=/tmp/t.log 下跑一条流式 prompt
- **THEN** 日志含出站 `session/create`/`subscribe`/`send` 方法名行、入站摘要行与 turn finalize 行,追加式不清空

#### Scenario: 未设置时零痕迹
- **WHEN** 环境变量未设置
- **THEN** 不创建文件、不写任何内容,行为与本变更前一致

### Requirement: 日志敏感信息红线
调试日志 MUST NOT 序列化任何出站请求的 params(`session/create`/
`session/resume` 的 runtimeModel 携带 provider apiKey);入站摘要 MUST
截断且不含凭证类字段值。红线由构造方式保证:出站日志行由方法名白名单
拼接,结构上不触碰 params。

#### Scenario: resume 不泄漏凭证
- **WHEN** ZCODE_TUI_LOG 开启时发生带 runtimeModel 的 session/resume
- **THEN** 日志只有 `-> session/resume (id N)` 一类的方法名行,文件全文不含 apiKey 值
